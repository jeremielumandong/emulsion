//! Model import (C7, C10): glTF 2.0 (`.gltf` with embedded or sibling
//! buffers, `.glb`) and Wavefront OBJ, parsed by hand with bounded sizes.
//! Bad files return [`SceneError::Import`]; nothing here panics on input.
//!
//! An [`ImportedModel`] keeps the node hierarchy, so a [`crate::ModelRef`] can
//! pose it by node/joint name; glTF skins are deformed with linear blend
//! skinning.

mod gltf;
mod obj;

use std::collections::{BTreeMap, HashMap};
use std::path::{Component, Path};
use std::sync::Arc;

use glam::{Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::error::SceneError;
use crate::math::Aabb;
use crate::mesh::Mesh;
use crate::scene::{Rgb, limits};
use crate::skeleton::JointRotation;
use crate::texture::MeshAlbedo;

/// Maximum vertices in one imported model.
pub const MAX_VERTICES: usize = 4_000_000;
/// Maximum nodes in one imported model.
pub const MAX_NODES: usize = 20_000;

/// A file format the importer understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelFormat {
    Gltf,
    Glb,
    Obj,
}

impl ModelFormat {
    /// Guesses the format from a file extension, falling back to content
    /// sniffing.
    pub fn detect(extension: Option<&str>, bytes: &[u8]) -> Option<ModelFormat> {
        if bytes.starts_with(b"glTF") {
            return Some(ModelFormat::Glb);
        }
        match extension.map(|e| e.to_ascii_lowercase()).as_deref() {
            Some("glb") => Some(ModelFormat::Glb),
            Some("gltf") => Some(ModelFormat::Gltf),
            Some("obj") => Some(ModelFormat::Obj),
            _ => {
                let head = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]);
                if head.trim_start().starts_with('{') {
                    Some(ModelFormat::Gltf)
                } else if head
                    .lines()
                    .any(|l| l.starts_with("v ") || l.starts_with("f "))
                {
                    Some(ModelFormat::Obj)
                } else {
                    None
                }
            }
        }
    }
}

/// A node of an imported model (also the joints of its skins).
#[derive(Debug, Clone, PartialEq)]
pub struct ModelNode {
    pub name: String,
    pub parent: Option<usize>,
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
    /// Indices into [`ImportedModel::primitives`] drawn at this node.
    pub primitives: Vec<usize>,
    pub skin: Option<usize>,
}

/// One drawable piece of geometry.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelPrimitive {
    pub mesh: Mesh,
    pub color: Rgb,
    pub double_sided: bool,
    /// Texture coordinates, vertex colours and base-colour texture, when the
    /// file has any (multiplies `color`).
    pub albedo: Option<Arc<MeshAlbedo>>,
    /// Per-vertex skin joints (indices into the skin's joint list), or empty.
    pub joints: Vec<[u16; 4]>,
    /// Per-vertex skin weights, or empty.
    pub weights: Vec<[f32; 4]>,
}

/// A glTF skin.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelSkin {
    /// Node index of each joint.
    pub joints: Vec<usize>,
    pub inverse_bind: Vec<Mat4>,
}

/// An imported model.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedModel {
    pub name: String,
    pub format: ModelFormat,
    /// Nodes in an order where parents come before children.
    pub nodes: Vec<ModelNode>,
    pub primitives: Vec<ModelPrimitive>,
    pub skins: Vec<ModelSkin>,
    /// Size of the source data in bytes.
    pub byte_size: usize,
}

/// Geometry of a model after posing, in model space.
#[derive(Debug, Clone)]
pub struct PosedPrimitive {
    pub mesh: Mesh,
    pub color: Rgb,
    pub double_sided: bool,
    /// Per-vertex albedo (vertex order matches `mesh`).
    pub albedo: Option<Arc<MeshAlbedo>>,
    /// Node index per triangle (the mesh node, or the dominant skin joint).
    pub triangle_nodes: Vec<u32>,
}

impl ImportedModel {
    pub fn triangle_count(&self) -> usize {
        self.nodes
            .iter()
            .flat_map(|n| n.primitives.iter())
            .map(|&p| self.primitives[p].mesh.triangle_count())
            .sum()
    }

    /// Names of all nodes that are skin joints (posable bones).
    pub fn joint_names(&self) -> Vec<String> {
        let mut seen = std::collections::BTreeSet::new();
        for s in &self.skins {
            seen.extend(s.joints.iter().copied());
        }
        seen.into_iter()
            .map(|i| self.nodes[i].name.clone())
            .collect()
    }

    /// Node index by name.
    pub fn node_index(&self, name: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.name == name)
    }

    /// Global node matrices with extra local rotations applied by name.
    pub fn node_matrices(&self, rotations: &BTreeMap<String, JointRotation>) -> Vec<Mat4> {
        let mut g = Vec::with_capacity(self.nodes.len());
        for n in &self.nodes {
            let extra = rotations
                .get(&n.name)
                .map(|r| r.to_quat())
                .unwrap_or(Quat::IDENTITY);
            let local =
                Mat4::from_scale_rotation_translation(n.scale, n.rotation * extra, n.translation);
            let m = match n.parent {
                Some(p) if p < g.len() => g[p] * local,
                _ => local,
            };
            g.push(m);
        }
        g
    }

    /// Poses the model (skins deformed) in model space.
    pub fn posed(&self, rotations: &BTreeMap<String, JointRotation>) -> Vec<PosedPrimitive> {
        let g = self.node_matrices(rotations);
        let mut out = Vec::new();
        for (ni, node) in self.nodes.iter().enumerate() {
            for &pi in &node.primitives {
                let prim = &self.primitives[pi];
                let skin = node.skin.and_then(|s| self.skins.get(s));
                let skinned = skin.is_some()
                    && prim.joints.len() == prim.mesh.positions.len()
                    && prim.weights.len() == prim.mesh.positions.len();
                let mut mesh = prim.mesh.clone();
                let mut triangle_nodes = vec![ni as u32; mesh.indices.len()];
                if let (true, Some(skin)) = (skinned, skin) {
                    let mats: Vec<Mat4> = skin
                        .joints
                        .iter()
                        .enumerate()
                        .map(|(k, &j)| {
                            g.get(j).copied().unwrap_or(Mat4::IDENTITY)
                                * skin.inverse_bind.get(k).copied().unwrap_or(Mat4::IDENTITY)
                        })
                        .collect();
                    for v in 0..mesh.positions.len() {
                        let (js, ws) = (prim.joints[v], prim.weights[v]);
                        let total: f32 = ws.iter().sum();
                        if total <= 1e-6 {
                            continue;
                        }
                        let mut m = Mat4::ZERO;
                        for k in 0..4 {
                            if ws[k] > 0.0
                                && let Some(jm) = mats.get(js[k] as usize)
                            {
                                m += *jm * (ws[k] / total);
                            }
                        }
                        mesh.positions[v] = m.transform_point3(mesh.positions[v]);
                        mesh.normals[v] = m.transform_vector3(mesh.normals[v]).normalize_or_zero();
                    }
                    // Label each triangle with the joint carrying most of its
                    // three vertices' weight.
                    for (t, tn) in mesh.indices.iter().zip(triangle_nodes.iter_mut()) {
                        let mut acc: [(u16, f32); 12] = [(0, 0.0); 12];
                        let mut n = 0;
                        for &vi in t {
                            let (js, ws) = (prim.joints[vi as usize], prim.weights[vi as usize]);
                            for k in 0..4 {
                                if ws[k] <= 0.0 {
                                    continue;
                                }
                                match acc[..n].iter_mut().find(|a| a.0 == js[k]) {
                                    Some(a) => a.1 += ws[k],
                                    None => {
                                        acc[n] = (js[k], ws[k]);
                                        n += 1;
                                    }
                                }
                            }
                        }
                        let best = acc[..n].iter().fold(None::<(u16, f32)>, |b, a| match b {
                            Some(b) if b.1 >= a.1 => Some(b),
                            _ => Some(*a),
                        });
                        if let Some(j) = best.and_then(|b| skin.joints.get(b.0 as usize)) {
                            *tn = *j as u32;
                        }
                    }
                } else {
                    mesh.transform(&g[ni]);
                }
                out.push(PosedPrimitive {
                    mesh,
                    color: prim.color,
                    double_sided: prim.double_sided,
                    albedo: prim.albedo.clone(),
                    triangle_nodes,
                });
            }
        }
        out
    }

    /// Bounds of the rest pose in model space.
    pub fn bounds(&self) -> Aabb {
        self.posed(&BTreeMap::new())
            .iter()
            .fold(Aabb::EMPTY, |b, p| b.union(&p.mesh.bounds()))
    }

    fn check_limits(&self) -> Result<(), SceneError> {
        if self.nodes.len() > MAX_NODES {
            return Err(SceneError::Import(format!(
                "{} nodes (max {MAX_NODES})",
                self.nodes.len()
            )));
        }
        let tris = self.triangle_count();
        if tris > limits::MAX_ASSET_TRIANGLES {
            return Err(SceneError::Import(format!(
                "{tris} triangles (max {})",
                limits::MAX_ASSET_TRIANGLES
            )));
        }
        let verts: usize = self.primitives.iter().map(|p| p.mesh.positions.len()).sum();
        if verts > MAX_VERTICES {
            return Err(SceneError::Import(format!(
                "{verts} vertices (max {MAX_VERTICES})"
            )));
        }
        if tris == 0 {
            return Err(SceneError::Import("the file has no triangles".into()));
        }
        Ok(())
    }
}

/// Resolves a relative URI (glTF external buffer) to bytes.
pub type Resolver<'a> = dyn FnMut(&str) -> Result<Vec<u8>, SceneError> + 'a;

/// Imports a model from memory. `resolve` loads external glTF buffers
/// (pass `None` to allow only embedded data).
pub fn import_bytes(
    name: &str,
    format: ModelFormat,
    bytes: &[u8],
    resolve: Option<&mut Resolver<'_>>,
) -> Result<ImportedModel, SceneError> {
    if bytes.len() > limits::MAX_ASSET_BYTES {
        return Err(SceneError::Import(format!(
            "file is {} bytes (max {})",
            bytes.len(),
            limits::MAX_ASSET_BYTES
        )));
    }
    let mut none = |uri: &str| -> Result<Vec<u8>, SceneError> {
        Err(SceneError::Import(format!(
            "external buffer `{uri}` not available"
        )))
    };
    let resolve: &mut Resolver<'_> = match resolve {
        Some(r) => r,
        None => &mut none,
    };
    let model = match format {
        ModelFormat::Glb => gltf::import_glb(name, bytes, resolve)?,
        ModelFormat::Gltf => gltf::import_gltf_json(name, bytes, None, resolve, ModelFormat::Gltf)?,
        ModelFormat::Obj => obj::import_obj(name, bytes)?,
    };
    model.check_limits()?;
    Ok(model)
}

/// Imports a model file from disk; external glTF buffers are read from the
/// same folder (paths that leave the folder are refused).
pub fn import_file(path: &Path) -> Result<ImportedModel, SceneError> {
    let meta = std::fs::metadata(path).map_err(|e| SceneError::Import(e.to_string()))?;
    if meta.len() as usize > limits::MAX_ASSET_BYTES {
        return Err(SceneError::Import("file is too large".into()));
    }
    let bytes = std::fs::read(path).map_err(|e| SceneError::Import(e.to_string()))?;
    let ext = path.extension().and_then(|e| e.to_str());
    let format = ModelFormat::detect(ext, &bytes)
        .ok_or_else(|| SceneError::Import("unknown model format".into()))?;
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("model")
        .to_string();
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut budget = limits::MAX_ASSET_BYTES.saturating_sub(bytes.len());
    let mut resolve = |uri: &str| -> Result<Vec<u8>, SceneError> {
        let rel = percent_decode(uri);
        let rel_path = Path::new(&rel);
        if rel_path
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
        {
            return Err(SceneError::Import(format!(
                "buffer path `{uri}` leaves the model folder"
            )));
        }
        let p = dir.join(rel_path);
        let len = std::fs::metadata(&p)
            .map_err(|e| SceneError::Import(format!("{uri}: {e}")))?
            .len() as usize;
        if len > budget {
            return Err(SceneError::Import("model buffers are too large".into()));
        }
        budget -= len;
        std::fs::read(&p).map_err(|e| SceneError::Import(format!("{uri}: {e}")))
    };
    let mut m = import_bytes(&name, format, &bytes, Some(&mut resolve))?;
    m.byte_size = limits::MAX_ASSET_BYTES - budget;
    Ok(m)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) =
                u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Imported models available to scenes, keyed by the asset id a
/// [`crate::ModelRef`] names.
#[derive(Debug, Clone, Default)]
pub struct AssetLibrary {
    models: HashMap<String, ImportedModel>,
}

impl AssetLibrary {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds (or replaces) a model under `id`, enforcing the total byte limit.
    pub fn insert(
        &mut self,
        id: impl Into<String>,
        model: ImportedModel,
    ) -> Result<(), SceneError> {
        let id = id.into();
        let others: usize = self
            .models
            .iter()
            .filter(|(k, _)| **k != id)
            .map(|(_, m)| m.byte_size)
            .sum();
        if others + model.byte_size > limits::MAX_ASSET_BYTES * 4 {
            return Err(SceneError::Limit("asset library is full".into()));
        }
        self.models.insert(id, model);
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&ImportedModel> {
        self.models.get(id)
    }

    pub fn remove(&mut self, id: &str) -> Option<ImportedModel> {
        self.models.remove(id)
    }

    pub fn ids(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.models.keys().map(String::as_str).collect();
        v.sort_unstable();
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_decoding() {
        assert_eq!(percent_decode("a%20b.bin"), "a b.bin");
        assert_eq!(percent_decode("bad%2"), "bad%2");
    }

    #[test]
    fn detect_formats() {
        assert_eq!(
            ModelFormat::detect(None, b"glTF\x02\0\0\0"),
            Some(ModelFormat::Glb)
        );
        assert_eq!(
            ModelFormat::detect(Some("OBJ"), b""),
            Some(ModelFormat::Obj)
        );
        assert_eq!(
            ModelFormat::detect(None, b"  {\"asset\":{}}"),
            Some(ModelFormat::Gltf)
        );
        assert_eq!(
            ModelFormat::detect(None, b"# x\nv 0 0 0\n"),
            Some(ModelFormat::Obj)
        );
        assert_eq!(ModelFormat::detect(None, b"\x00\x01"), None);
    }
}
