//! glTF 2.0 reader: JSON and binary containers, embedded (data URI), GLB
//! and resolver-provided buffers; triangle primitives (lists, strips, fans),
//! node transforms, base colours and skins. Sparse accessors and required
//! extensions (Draco, meshopt…) are refused with an error.

use std::collections::BTreeSet;

use base64::Engine;
use glam::{Mat4, Quat, Vec3};
use serde_json::Value;

use super::{
    ImportedModel, MAX_NODES, MAX_VERTICES, ModelFormat, ModelNode, ModelPrimitive, ModelSkin,
    Resolver,
};
use crate::error::SceneError;
use crate::mesh::Mesh;
use crate::scene::Rgb;

fn err(msg: impl Into<String>) -> SceneError {
    SceneError::Import(msg.into())
}

/// Parses a `.glb` container.
pub(super) fn import_glb(
    name: &str,
    bytes: &[u8],
    resolve: &mut Resolver<'_>,
) -> Result<ImportedModel, SceneError> {
    let u32_at = |o: usize| -> Result<u32, SceneError> {
        bytes
            .get(o..o + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or_else(|| err("truncated GLB"))
    };
    if bytes.get(0..4) != Some(b"glTF") {
        return Err(err("not a GLB file"));
    }
    if u32_at(4)? != 2 {
        return Err(err("only glTF 2.0 is supported"));
    }
    let total = (u32_at(8)? as usize).min(bytes.len());
    let mut off = 12;
    let mut json: Option<&[u8]> = None;
    let mut bin: Option<Vec<u8>> = None;
    while off + 8 <= total {
        let len = u32_at(off)? as usize;
        let kind = u32_at(off + 4)?;
        let start = off + 8;
        let end = start
            .checked_add(len)
            .filter(|e| *e <= total)
            .ok_or_else(|| err("GLB chunk out of bounds"))?;
        match kind {
            0x4E4F_534A if json.is_none() => json = Some(&bytes[start..end]),
            0x004E_4942 if bin.is_none() => bin = Some(bytes[start..end].to_vec()),
            _ => {}
        }
        off = end + (4 - end % 4) % 4;
    }
    let json = json.ok_or_else(|| err("GLB has no JSON chunk"))?;
    let mut m = import_gltf_json(name, json, bin, resolve, ModelFormat::Glb)?;
    m.byte_size = bytes.len();
    Ok(m)
}

/// Parses glTF JSON (with an optional GLB binary chunk).
pub(super) fn import_gltf_json(
    name: &str,
    json: &[u8],
    glb_bin: Option<Vec<u8>>,
    resolve: &mut Resolver<'_>,
    format: ModelFormat,
) -> Result<ImportedModel, SceneError> {
    let doc: Value =
        serde_json::from_slice(json).map_err(|e| err(format!("bad glTF JSON: {e}")))?;
    let version = doc
        .pointer("/asset/version")
        .and_then(Value::as_str)
        .unwrap_or("2.0");
    if !version.starts_with('2') {
        return Err(err(format!("glTF version {version} is not supported")));
    }
    if let Some(req) = doc.get("extensionsRequired").and_then(Value::as_array)
        && !req.is_empty()
    {
        let names: Vec<&str> = req.iter().filter_map(Value::as_str).collect();
        return Err(err(format!(
            "required extensions are not supported: {}",
            names.join(", ")
        )));
    }

    // Buffers.
    let mut glb_bin = glb_bin;
    let mut buffers: Vec<Vec<u8>> = Vec::new();
    for (i, b) in arr(&doc, "buffers").iter().enumerate() {
        let len = b.get("byteLength").and_then(Value::as_u64).unwrap_or(0) as usize;
        let data = match b.get("uri").and_then(Value::as_str) {
            Some(uri) if uri.starts_with("data:") => {
                let comma = uri.find(',').ok_or_else(|| err("malformed data URI"))?;
                if !uri[..comma].ends_with(";base64") {
                    return Err(err("only base64 data URIs are supported"));
                }
                base64::engine::general_purpose::STANDARD_PAD_INDIFFERENT
                    .decode(uri[comma + 1..].trim())
                    .map_err(|e| err(format!("bad base64 in buffer {i}: {e}")))?
            }
            Some(uri) => resolve(uri)?,
            None if i == 0 => glb_bin.take().ok_or_else(|| err("buffer 0 has no data"))?,
            None => return Err(err(format!("buffer {i} has no data"))),
        };
        if data.len() < len {
            return Err(err(format!("buffer {i} is shorter than its byteLength")));
        }
        buffers.push(data);
    }
    let ctx = Ctx {
        doc: &doc,
        buffers: &buffers,
    };

    // Materials.
    let materials: Vec<(Rgb, bool)> = arr(&doc, "materials")
        .iter()
        .map(|m| {
            let c = m
                .pointer("/pbrMetallicRoughness/baseColorFactor")
                .and_then(Value::as_array)
                .map(|a| [0, 1, 2].map(|i| a.get(i).and_then(Value::as_f64).unwrap_or(1.0) as f32))
                .unwrap_or([0.8, 0.8, 0.8]);
            let to_srgb = |v: f32| {
                let v = if v.is_finite() {
                    v.clamp(0.0, 1.0)
                } else {
                    0.8
                };
                let s = if v <= 0.003_130_8 {
                    v * 12.92
                } else {
                    1.055 * v.powf(1.0 / 2.4) - 0.055
                };
                (s * 255.0).round() as u8
            };
            (
                Rgb(c.map(to_srgb)),
                m.get("doubleSided")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            )
        })
        .collect();

    // Meshes → primitives.
    let mut primitives: Vec<ModelPrimitive> = Vec::new();
    let mut mesh_prims: Vec<Vec<usize>> = Vec::new();
    let mut vertex_total = 0usize;
    for (mi, mesh) in arr(&doc, "meshes").iter().enumerate() {
        let mut ids = Vec::new();
        for p in mesh
            .get("primitives")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[])
        {
            let mode = p.get("mode").and_then(Value::as_u64).unwrap_or(4);
            if !(4..=6).contains(&mode) {
                continue; // points and lines are not drawn
            }
            let attrs = p
                .get("attributes")
                .ok_or_else(|| err(format!("mesh {mi} primitive has no attributes")))?;
            let Some(pos_acc) = attrs.get("POSITION").and_then(Value::as_u64) else {
                continue;
            };
            let positions: Vec<Vec3> = ctx
                .read_f32(pos_acc as usize, 3)?
                .as_chunks::<3>()
                .0
                .iter()
                .map(|c| Vec3::from_array(*c))
                .collect();
            if positions.iter().any(|v| !v.is_finite()) {
                return Err(err(format!("mesh {mi} has non-finite positions")));
            }
            vertex_total += positions.len();
            if vertex_total > MAX_VERTICES {
                return Err(err(format!("more than {MAX_VERTICES} vertices")));
            }
            let n = positions.len();
            let normals: Vec<Vec3> = match attrs.get("NORMAL").and_then(Value::as_u64) {
                Some(a) => ctx
                    .read_f32(a as usize, 3)?
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .map(|c| Vec3::from_array(*c))
                    .collect(),
                None => Vec::new(),
            };
            let indices: Vec<u32> = match p.get("indices").and_then(Value::as_u64) {
                Some(a) => ctx.read_u32(a as usize)?,
                None => (0..n as u32).collect(),
            };
            let tris = triangulate(&indices, mode);
            let joints: Vec<[u16; 4]> = match attrs.get("JOINTS_0").and_then(Value::as_u64) {
                Some(a) => ctx
                    .read_u32_n(a as usize, 4)?
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|c| c.map(|v| v.min(u16::MAX as u32) as u16))
                    .collect(),
                None => Vec::new(),
            };
            let weights: Vec<[f32; 4]> = match attrs.get("WEIGHTS_0").and_then(Value::as_u64) {
                Some(a) => ctx
                    .read_f32(a as usize, 4)?
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|c| c.map(|w| if w.is_finite() { w.max(0.0) } else { 0.0 }))
                    .collect(),
                None => Vec::new(),
            };
            let (color, double_sided) = p
                .get("material")
                .and_then(Value::as_u64)
                .and_then(|m| materials.get(m as usize).copied())
                .unwrap_or((Rgb([204, 204, 204]), false));
            let mut mesh = Mesh {
                positions,
                normals,
                indices: tris,
            };
            mesh.sanitize();
            ids.push(primitives.len());
            primitives.push(ModelPrimitive {
                mesh,
                color,
                double_sided,
                joints: if joints.len() == n {
                    joints
                } else {
                    Vec::new()
                },
                weights: if weights.len() == n {
                    weights
                } else {
                    Vec::new()
                },
            });
        }
        mesh_prims.push(ids);
    }

    // Skins.
    let node_count = arr(&doc, "nodes").len();
    if node_count > MAX_NODES {
        return Err(err(format!("more than {MAX_NODES} nodes")));
    }
    let mut skins = Vec::new();
    for s in arr(&doc, "skins") {
        let joints: Vec<usize> = s
            .get("joints")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_u64)
                    .map(|v| v as usize)
                    .collect()
            })
            .unwrap_or_default();
        if joints.iter().any(|&j| j >= node_count) {
            return Err(err("skin joint out of range"));
        }
        let inverse_bind = match s.get("inverseBindMatrices").and_then(Value::as_u64) {
            Some(a) => ctx
                .read_f32(a as usize, 16)?
                .as_chunks::<16>()
                .0
                .iter()
                .map(Mat4::from_cols_array)
                .collect(),
            None => vec![Mat4::IDENTITY; joints.len()],
        };
        skins.push(ModelSkin {
            joints,
            inverse_bind,
        });
    }

    // Nodes: keep those reachable from the default scene, parents first.
    let raw_nodes = arr(&doc, "nodes");
    let mut parent = vec![None::<usize>; node_count];
    for (i, n) in raw_nodes.iter().enumerate() {
        for c in n
            .get("children")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[])
        {
            let c = c
                .as_u64()
                .map(|v| v as usize)
                .filter(|&c| c < node_count && c != i)
                .ok_or_else(|| err("bad child index"))?;
            if parent[c].is_some() {
                return Err(err("a node has two parents"));
            }
            parent[c] = Some(i);
        }
    }
    let scene_idx = doc.get("scene").and_then(Value::as_u64).unwrap_or(0) as usize;
    let roots: Vec<usize> = match arr(&doc, "scenes")
        .get(scene_idx)
        .and_then(|s| s.get("nodes"))
        .and_then(Value::as_array)
    {
        Some(a) => a
            .iter()
            .filter_map(Value::as_u64)
            .map(|v| v as usize)
            .filter(|&v| v < node_count)
            .collect(),
        None => (0..node_count).filter(|&i| parent[i].is_none()).collect(),
    };
    // Breadth-first order, guarding against cycles.
    let mut order: Vec<usize> = Vec::new();
    let mut visited = BTreeSet::new();
    let mut queue: std::collections::VecDeque<usize> = roots.into_iter().collect();
    while let Some(i) = queue.pop_front() {
        if !visited.insert(i) {
            continue;
        }
        order.push(i);
        for c in raw_nodes[i]
            .get("children")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[])
        {
            if let Some(c) = c.as_u64().map(|v| v as usize).filter(|&c| c < node_count) {
                queue.push_back(c);
            }
        }
    }
    // Skin joints outside the scene graph still need nodes.
    for s in &skins {
        for &j in &s.joints {
            if visited.insert(j) {
                order.push(j);
            }
        }
    }
    let mut remap = vec![usize::MAX; node_count];
    for (new, &old) in order.iter().enumerate() {
        remap[old] = new;
    }
    let mut nodes = Vec::with_capacity(order.len());
    for &old in &order {
        let n = &raw_nodes[old];
        let (t, r, s) = node_trs(n)?;
        let prims = n
            .get("mesh")
            .and_then(Value::as_u64)
            .and_then(|m| mesh_prims.get(m as usize))
            .cloned()
            .unwrap_or_default();
        let skin = n
            .get("skin")
            .and_then(Value::as_u64)
            .map(|v| v as usize)
            .filter(|&v| v < skins.len());
        let name = n
            .get("name")
            .and_then(Value::as_str)
            .map(|s| s.chars().take(200).collect())
            .unwrap_or_else(|| format!("node{old}"));
        let p = parent[old].map(|p| remap[p]).filter(|&p| p != usize::MAX);
        nodes.push(ModelNode {
            name,
            parent: p,
            translation: t,
            rotation: r,
            scale: s,
            primitives: prims,
            skin,
        });
    }
    // A parent reached only via a skin may come after its child; fix ordering by
    // dropping such parent links (rare, and they would break the parents-first rule).
    for (i, node) in nodes.iter_mut().enumerate() {
        if node.parent.is_some_and(|p| p >= i) {
            node.parent = None;
        }
    }
    for s in &mut skins {
        for j in &mut s.joints {
            *j = remap[*j];
        }
    }
    Ok(ImportedModel {
        name: name.to_string(),
        format,
        nodes,
        primitives,
        skins,
        byte_size: json.len() + buffers.iter().map(Vec::len).sum::<usize>(),
    })
}

fn arr<'a>(doc: &'a Value, key: &str) -> &'a [Value] {
    doc.get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn node_trs(n: &Value) -> Result<(Vec3, Quat, Vec3), SceneError> {
    let floats = |key: &str, len: usize| -> Option<Vec<f32>> {
        let a = n.get(key)?.as_array()?;
        (a.len() == len).then(|| a.iter().map(|v| v.as_f64().unwrap_or(0.0) as f32).collect())
    };
    if let Some(m) = floats("matrix", 16) {
        let m = Mat4::from_cols_slice(&m);
        if !m.is_finite() {
            return Err(err("non-finite node matrix"));
        }
        let (s, r, t) = m.to_scale_rotation_translation();
        return Ok((t, r.normalize(), s));
    }
    let t = floats("translation", 3)
        .map(|v| Vec3::new(v[0], v[1], v[2]))
        .unwrap_or(Vec3::ZERO);
    let r = floats("rotation", 4)
        .map(|v| Quat::from_xyzw(v[0], v[1], v[2], v[3]))
        .filter(|q| q.length_squared() > 1e-12)
        .map(Quat::normalize)
        .unwrap_or(Quat::IDENTITY);
    let s = floats("scale", 3)
        .map(|v| Vec3::new(v[0], v[1], v[2]))
        .unwrap_or(Vec3::ONE);
    if !(t.is_finite() && r.is_finite() && s.is_finite()) {
        return Err(err("non-finite node transform"));
    }
    Ok((t, r, s))
}

fn triangulate(idx: &[u32], mode: u64) -> Vec<[u32; 3]> {
    match mode {
        5 => (2..idx.len())
            .map(|i| {
                if i % 2 == 0 {
                    [idx[i - 2], idx[i - 1], idx[i]]
                } else {
                    [idx[i - 1], idx[i - 2], idx[i]]
                }
            })
            .collect(),
        6 => (2..idx.len())
            .map(|i| [idx[0], idx[i - 1], idx[i]])
            .collect(),
        _ => idx.as_chunks::<3>().0.to_vec(),
    }
}

struct Ctx<'a> {
    doc: &'a Value,
    buffers: &'a [Vec<u8>],
}

struct Access<'a> {
    data: &'a [u8],
    base: usize,
    stride: usize,
    count: usize,
    ctype: u64,
    ncomp: usize,
    normalized: bool,
}

impl Ctx<'_> {
    fn access(&self, index: usize, want: usize) -> Result<Option<Access<'_>>, SceneError> {
        let a = arr(self.doc, "accessors")
            .get(index)
            .ok_or_else(|| err(format!("accessor {index} missing")))?;
        if a.get("sparse").is_some() {
            return Err(err("sparse accessors are not supported"));
        }
        let ncomp = match a.get("type").and_then(Value::as_str).unwrap_or("") {
            "SCALAR" => 1,
            "VEC2" => 2,
            "VEC3" => 3,
            "VEC4" => 4,
            "MAT4" => 16,
            "MAT2" => 4,
            "MAT3" => 9,
            t => return Err(err(format!("accessor type `{t}` not supported"))),
        };
        if ncomp != want {
            return Err(err(format!(
                "accessor {index} has {ncomp} components, expected {want}"
            )));
        }
        let ctype = a.get("componentType").and_then(Value::as_u64).unwrap_or(0);
        let csize = match ctype {
            5120 | 5121 => 1,
            5122 | 5123 => 2,
            5125 | 5126 => 4,
            _ => return Err(err(format!("component type {ctype} not supported"))),
        };
        let count = a.get("count").and_then(Value::as_u64).unwrap_or(0) as usize;
        if count > MAX_VERTICES * 3 {
            return Err(err("accessor too large"));
        }
        let Some(view_idx) = a.get("bufferView").and_then(Value::as_u64) else {
            return Ok(None); // all zeros per the spec
        };
        let v = arr(self.doc, "bufferViews")
            .get(view_idx as usize)
            .ok_or_else(|| err("buffer view missing"))?;
        let buf = v
            .get("buffer")
            .and_then(Value::as_u64)
            .and_then(|b| self.buffers.get(b as usize))
            .ok_or_else(|| err("buffer missing"))?;
        let view_off = v.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
        let view_len = v.get("byteLength").and_then(Value::as_u64).unwrap_or(0) as usize;
        let view_end = view_off
            .checked_add(view_len)
            .filter(|e| *e <= buf.len())
            .ok_or_else(|| err("buffer view out of bounds"))?;
        let elem = csize * ncomp;
        let stride = v
            .get("byteStride")
            .and_then(Value::as_u64)
            .map(|s| s as usize)
            .filter(|&s| s >= elem)
            .unwrap_or(elem);
        let acc_off = a.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
        if count > 0 {
            let last = acc_off
                .checked_add(
                    (count - 1)
                        .checked_mul(stride)
                        .ok_or_else(|| err("accessor overflow"))?,
                )
                .and_then(|x| x.checked_add(elem))
                .ok_or_else(|| err("accessor overflow"))?;
            if last > view_len {
                return Err(err(format!("accessor {index} reads past its buffer view")));
            }
        }
        Ok(Some(Access {
            data: &buf[view_off..view_end],
            base: acc_off,
            stride,
            count,
            ctype,
            ncomp,
            normalized: a
                .get("normalized")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        }))
    }

    fn count(&self, index: usize) -> usize {
        arr(self.doc, "accessors")
            .get(index)
            .and_then(|a| a.get("count"))
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .min((MAX_VERTICES * 3) as u64) as usize
    }

    fn read_f32(&self, index: usize, ncomp: usize) -> Result<Vec<f32>, SceneError> {
        let Some(a) = self.access(index, ncomp)? else {
            return Ok(vec![0.0; self.count(index) * ncomp]);
        };
        let mut out = Vec::with_capacity(a.count * a.ncomp);
        for e in 0..a.count {
            for c in 0..a.ncomp {
                let raw = component(&a, e, c);
                out.push(if a.normalized {
                    match a.ctype {
                        5120 => (raw / 127.0).max(-1.0),
                        5121 => raw / 255.0,
                        5122 => (raw / 32767.0).max(-1.0),
                        5123 => raw / 65535.0,
                        _ => raw,
                    }
                } else {
                    raw
                });
            }
        }
        Ok(out)
    }

    fn read_u32_n(&self, index: usize, ncomp: usize) -> Result<Vec<u32>, SceneError> {
        let Some(a) = self.access(index, ncomp)? else {
            return Ok(vec![0; self.count(index) * ncomp]);
        };
        if a.ctype == 5126 {
            return Err(err("integer accessor has float components"));
        }
        let mut out = Vec::with_capacity(a.count * a.ncomp);
        for e in 0..a.count {
            for c in 0..a.ncomp {
                out.push(component_u32(&a, e, c));
            }
        }
        Ok(out)
    }

    fn read_u32(&self, index: usize) -> Result<Vec<u32>, SceneError> {
        self.read_u32_n(index, 1)
    }
}

fn offset(a: &Access<'_>, e: usize, c: usize) -> usize {
    let csize = match a.ctype {
        5120 | 5121 => 1,
        5122 | 5123 => 2,
        _ => 4,
    };
    a.base + e * a.stride + c * csize
}

fn component(a: &Access<'_>, e: usize, c: usize) -> f32 {
    let o = offset(a, e, c);
    let d = a.data;
    match a.ctype {
        5120 => d[o] as i8 as f32,
        5121 => d[o] as f32,
        5122 => i16::from_le_bytes([d[o], d[o + 1]]) as f32,
        5123 => u16::from_le_bytes([d[o], d[o + 1]]) as f32,
        5125 => u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]) as f32,
        _ => f32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]),
    }
}

fn component_u32(a: &Access<'_>, e: usize, c: usize) -> u32 {
    let o = offset(a, e, c);
    let d = a.data;
    match a.ctype {
        5120 | 5121 => d[o] as u32,
        5122 | 5123 => u16::from_le_bytes([d[o], d[o + 1]]) as u32,
        _ => u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]),
    }
}
