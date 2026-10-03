//! Wavefront OBJ reader: `v`, `vn`, `f` (any polygon, fan-triangulated,
//! negative indices allowed) and `o`/`g` groups, which become named nodes.
//! Materials and texture coordinates are ignored.

use std::collections::HashMap;

use glam::{Quat, Vec3};

use super::{ImportedModel, MAX_VERTICES, ModelFormat, ModelNode, ModelPrimitive};
use crate::error::SceneError;
use crate::mesh::Mesh;
use crate::scene::{Rgb, limits};

fn err(line: usize, msg: impl Into<String>) -> SceneError {
    SceneError::Import(format!("OBJ line {line}: {}", msg.into()))
}

struct Group {
    name: String,
    mesh: Mesh,
    /// (position index, normal index or usize::MAX) → vertex.
    map: HashMap<(usize, usize), u32>,
    has_normal: Vec<bool>,
}

impl Group {
    fn new(name: String) -> Self {
        Group {
            name,
            mesh: Mesh::default(),
            map: HashMap::new(),
            has_normal: Vec::new(),
        }
    }
}

pub(super) fn import_obj(name: &str, bytes: &[u8]) -> Result<ImportedModel, SceneError> {
    let text = String::from_utf8_lossy(bytes);
    let mut v: Vec<Vec3> = Vec::new();
    let mut vn: Vec<Vec3> = Vec::new();
    let mut groups: Vec<Group> = vec![Group::new(name.to_string())];
    let mut tris = 0usize;
    for (ln, line) in text.lines().enumerate() {
        let ln = ln + 1;
        let line = line.split('#').next().unwrap_or("").trim();
        let mut it = line.split_whitespace();
        let Some(tag) = it.next() else { continue };
        match tag {
            "v" | "vn" => {
                let nums: Vec<f32> = it
                    .take(3)
                    .map(|s| s.parse::<f32>())
                    .collect::<Result<_, _>>()
                    .map_err(|_| err(ln, "bad number"))?;
                if nums.len() < 3 || nums.iter().any(|x| !x.is_finite()) {
                    return Err(err(ln, "expected three finite numbers"));
                }
                let p = Vec3::new(nums[0], nums[1], nums[2]);
                if tag == "v" {
                    v.push(p);
                } else {
                    vn.push(p.normalize_or_zero());
                }
                if v.len() + vn.len() > MAX_VERTICES * 2 {
                    return Err(err(ln, "too many vertices"));
                }
            }
            "o" | "g" => {
                let gname: String = it.collect::<Vec<_>>().join(" ").chars().take(200).collect();
                let gname = if gname.is_empty() {
                    format!("group{}", groups.len())
                } else {
                    gname
                };
                if groups.last().is_some_and(|g| g.mesh.indices.is_empty()) {
                    if let Some(g) = groups.last_mut() {
                        g.name = gname;
                    }
                } else {
                    groups.push(Group::new(gname));
                }
            }
            "f" => {
                let g = groups.last_mut().expect("at least one group");
                let mut poly: Vec<u32> = Vec::new();
                for tok in it {
                    let mut parts = tok.split('/');
                    let vi = resolve(parts.next().unwrap_or(""), v.len())
                        .ok_or_else(|| err(ln, "bad vertex index"))?;
                    let _vt = parts.next();
                    let ni = match parts.next() {
                        Some(s) if !s.is_empty() => {
                            resolve(s, vn.len()).ok_or_else(|| err(ln, "bad normal index"))?
                        }
                        _ => usize::MAX,
                    };
                    let next = g.mesh.positions.len() as u32;
                    let idx = *g.map.entry((vi, ni)).or_insert_with(|| {
                        g.mesh.positions.push(v[vi]);
                        g.mesh
                            .normals
                            .push(if ni == usize::MAX { Vec3::ZERO } else { vn[ni] });
                        g.has_normal.push(ni != usize::MAX);
                        next
                    });
                    poly.push(idx);
                }
                if poly.len() < 3 {
                    return Err(err(ln, "face needs at least three vertices"));
                }
                for i in 2..poly.len() {
                    g.mesh.indices.push([poly[0], poly[i - 1], poly[i]]);
                }
                tris += poly.len() - 2;
                if tris > limits::MAX_ASSET_TRIANGLES {
                    return Err(err(ln, "too many triangles"));
                }
            }
            _ => {}
        }
    }
    let mut nodes = Vec::new();
    let mut primitives = Vec::new();
    for g in groups.into_iter().filter(|g| !g.mesh.indices.is_empty()) {
        let mut mesh = g.mesh;
        if g.has_normal.iter().any(|h| !h) {
            let given = mesh.normals.clone();
            mesh.compute_normals();
            for (i, has) in g.has_normal.iter().enumerate() {
                if *has && given[i] != Vec3::ZERO {
                    mesh.normals[i] = given[i];
                }
            }
        }
        mesh.sanitize();
        nodes.push(ModelNode {
            name: g.name,
            parent: None,
            translation: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
            primitives: vec![primitives.len()],
            skin: None,
        });
        primitives.push(ModelPrimitive {
            mesh,
            color: Rgb([204, 204, 204]),
            double_sided: false,
            albedo: None,
            joints: Vec::new(),
            weights: Vec::new(),
        });
    }
    Ok(ImportedModel {
        name: name.to_string(),
        format: ModelFormat::Obj,
        nodes,
        primitives,
        skins: Vec::new(),
        byte_size: bytes.len(),
    })
}

/// 1-based (or negative, relative) OBJ index → 0-based.
fn resolve(s: &str, len: usize) -> Option<usize> {
    let i: i64 = s.parse().ok()?;
    let r = if i > 0 {
        i - 1
    } else if i < 0 {
        len as i64 + i
    } else {
        return None;
    };
    (r >= 0 && (r as usize) < len).then_some(r as usize)
}
