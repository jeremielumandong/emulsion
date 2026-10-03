//! Tessellating a scene into world-space triangles for rendering and picking.
//!
//! [`PreparedScene`] is the cache the viewport keeps between frames: rebuild
//! it when objects change; camera-only changes can reuse it.

use std::sync::Arc;

use glam::{Mat4, Vec3};
use rayon::prelude::*;

use crate::character::{PosedCharacter, evaluate};
use crate::error::SceneError;
use crate::import::AssetLibrary;
use crate::mannequin::{self, head_center, head_radii, part_matrix};
use crate::math::Aabb;
use crate::mesh::{self, Mesh};
use crate::scene::{Environment, LightKind, ObjectId, ObjectKind, Prop, Scene, limits};
use crate::skeleton::Bone;
use crate::texture::MeshAlbedo;

/// What part of an object a triangle belongs to (for picking).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartLabel {
    /// The object as a whole (built-in props).
    Whole,
    /// A mannequin bone.
    Bone(Bone),
    /// A node or skin joint of an imported model, by name.
    Node(String),
}

/// Renderable geometry of (part of) one scene object, in world space.
#[derive(Debug, Clone)]
pub struct PreparedMesh {
    pub id: ObjectId,
    /// Linear-light base colour.
    pub color: Vec3,
    pub double_sided: bool,
    /// Whether the mesh casts shadows.
    pub casts_shadows: bool,
    /// Texture coordinates, vertex colours and texture (imported models),
    /// multiplying `color`.
    pub albedo: Option<Arc<MeshAlbedo>>,
    pub mesh: Mesh,
    /// Index into `parts` for each triangle.
    pub triangle_parts: Vec<u32>,
    pub parts: Vec<PartLabel>,
    pub bounds: Aabb,
}

/// A directional light resolved for shading.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreparedLight {
    /// Unit vector pointing from surfaces toward the light.
    pub to_light: Vec3,
    pub intensity: f32,
    pub kind: LightKind,
    /// A key light with shadows switched on.
    pub casts_shadows: bool,
}

/// A scene ready to render or pick.
#[derive(Debug, Clone)]
pub struct PreparedScene {
    pub meshes: Vec<PreparedMesh>,
    pub lights: Vec<PreparedLight>,
    pub environment: Environment,
    pub characters: Vec<PosedCharacter>,
    /// Face-feature strokes (world space) per character id.
    pub face_strokes: Vec<(ObjectId, Vec<Vec<Vec3>>, f32)>,
    pub bounds: Aabb,
    /// Problems that did not stop preparation (e.g. a missing model asset,
    /// drawn as a placeholder box).
    pub warnings: Vec<String>,
}

impl PreparedScene {
    pub fn triangle_count(&self) -> usize {
        self.meshes.iter().map(|m| m.mesh.triangle_count()).sum()
    }

    /// The posed character with this id.
    pub fn character(&self, id: ObjectId) -> Option<&PosedCharacter> {
        self.characters.iter().find(|c| c.id == id)
    }

    /// World bounds of one object.
    pub fn object_bounds(&self, id: ObjectId) -> Aabb {
        self.meshes
            .iter()
            .filter(|m| m.id == id)
            .fold(Aabb::EMPTY, |b, m| b.union(&m.bounds))
    }
}

enum Built {
    Meshes(
        Vec<PreparedMesh>,
        Option<Box<PosedCharacter>>,
        Option<(Vec<Vec<Vec3>>, f32)>,
        Option<String>,
    ),
    Light(PreparedLight),
    Nothing,
}

/// Validates and tessellates a scene.
pub fn prepare(scene: &Scene, assets: &AssetLibrary) -> Result<PreparedScene, SceneError> {
    scene.validate()?;
    let built: Vec<Built> = scene
        .objects
        .par_iter()
        .map(|o| {
            if !o.visible {
                return Built::Nothing;
            }
            let world = o.transform.matrix();
            let color = o.color.to_linear();
            match &o.kind {
                ObjectKind::Light(l) => {
                    let dir = o.transform.forward();
                    Built::Light(PreparedLight {
                        to_light: -dir.normalize_or_zero(),
                        intensity: l.intensity,
                        kind: l.kind,
                        casts_shadows: o.casts_shadows && l.kind == LightKind::Key,
                    })
                }
                ObjectKind::Character(c) => {
                    let mm = mannequin::generate(&c.body);
                    let posed = evaluate(o.id, c, &world, &mm);
                    let mut mesh = Mesh::default();
                    let mut tri_parts = Vec::new();
                    let mut parts = Vec::new();
                    for part in &mm.parts {
                        let m = world * part_matrix(&posed.bones[part.bone.index()]);
                        let pi = match parts.iter().position(|p| *p == PartLabel::Bone(part.bone)) {
                            Some(i) => i,
                            None => {
                                parts.push(PartLabel::Bone(part.bone));
                                parts.len() - 1
                            }
                        };
                        mesh.append_transformed(&part.mesh, &m);
                        tri_parts
                            .extend(std::iter::repeat_n(pi as u32, part.mesh.triangle_count()));
                    }
                    let strokes = face_strokes(&posed, c.face);
                    let pm = finish(o.id, color, None, mesh, tri_parts, parts);
                    Built::Meshes(vec![pm], Some(Box::new(posed)), Some(strokes), None)
                }
                ObjectKind::Prop(Prop::Builtin(b)) => {
                    let mut mesh = b.mesh();
                    mesh.transform(&world);
                    let n = mesh.triangle_count();
                    Built::Meshes(
                        vec![finish(
                            o.id,
                            color,
                            None,
                            mesh,
                            vec![0; n],
                            vec![PartLabel::Whole],
                        )],
                        None,
                        None,
                        None,
                    )
                }
                ObjectKind::Prop(Prop::Model(r)) => match assets.get(&r.asset) {
                    Some(model) => {
                        let posed = model.posed(&r.joint_rotations);
                        let out = posed
                            .into_iter()
                            .map(|p| {
                                let mut mesh = p.mesh;
                                mesh.transform(&world);
                                // Map node indices to compact part labels.
                                let mut parts: Vec<PartLabel> = Vec::new();
                                let mut map = std::collections::HashMap::new();
                                let tri_parts = p
                                    .triangle_nodes
                                    .iter()
                                    .map(|&n| {
                                        *map.entry(n).or_insert_with(|| {
                                            let name = model
                                                .nodes
                                                .get(n as usize)
                                                .map(|n| n.name.clone())
                                                .unwrap_or_default();
                                            parts.push(PartLabel::Node(name));
                                            parts.len() as u32 - 1
                                        })
                                    })
                                    .collect();
                                // Imported models keep their own colours, tinted by the object colour.
                                let tint = color / crate::scene::Rgb::PROP.to_linear();
                                let mut pm = finish(
                                    o.id,
                                    p.color.to_linear() * tint.min(Vec3::splat(4.0)),
                                    p.albedo,
                                    mesh,
                                    tri_parts,
                                    parts,
                                );
                                pm.double_sided = p.double_sided;
                                pm
                            })
                            .collect();
                        Built::Meshes(out, None, None, None)
                    }
                    None => {
                        let mut mesh = mesh::cuboid_on_ground(Vec3::splat(0.5));
                        mesh.transform(&world);
                        let n = mesh.triangle_count();
                        Built::Meshes(
                            vec![finish(
                                o.id,
                                color,
                                None,
                                mesh,
                                vec![0; n],
                                vec![PartLabel::Whole],
                            )],
                            None,
                            None,
                            Some(format!(
                                "model asset `{}` is missing; showing a placeholder",
                                r.asset
                            )),
                        )
                    }
                },
            }
        })
        .collect();

    let mut out = PreparedScene {
        meshes: Vec::new(),
        lights: Vec::new(),
        environment: scene.environment,
        characters: Vec::new(),
        face_strokes: Vec::new(),
        bounds: Aabb::EMPTY,
        warnings: Vec::new(),
    };
    for b in built {
        match b {
            Built::Light(l) => out.lights.push(l),
            Built::Meshes(ms, posed, strokes, warn) => {
                if let (Some(p), Some((s, w))) = (&posed, strokes) {
                    out.face_strokes.push((p.id, s, w));
                }
                if let Some(p) = posed {
                    out.characters.push(*p);
                }
                out.warnings.extend(warn);
                for mut m in ms {
                    m.casts_shadows = scene.object(m.id).is_none_or(|o| o.casts_shadows);
                    out.bounds = out.bounds.union(&m.bounds);
                    out.meshes.push(m);
                }
            }
            Built::Nothing => {}
        }
    }
    let tris = out.triangle_count();
    if tris > limits::MAX_TRIANGLES {
        return Err(SceneError::Limit(format!(
            "{tris} triangles (max {})",
            limits::MAX_TRIANGLES
        )));
    }
    Ok(out)
}

fn finish(
    id: ObjectId,
    color: Vec3,
    albedo: Option<Arc<MeshAlbedo>>,
    mesh: Mesh,
    triangle_parts: Vec<u32>,
    parts: Vec<PartLabel>,
) -> PreparedMesh {
    let bounds = mesh.bounds();
    PreparedMesh {
        id,
        color,
        double_sided: false,
        casts_shadows: true,
        albedo,
        mesh,
        triangle_parts,
        parts,
        bounds,
    }
}

/// Face strokes in world space and the head height (used to size lines).
fn face_strokes(p: &PosedCharacter, face: crate::pose::FacePreset) -> (Vec<Vec<Vec3>>, f32) {
    let hh = p.rig.head_height;
    let r = head_radii(hh);
    let c = head_center(hh);
    let m: Mat4 = p.world * part_matrix(&p.bones[Bone::Head.index()]);
    let strokes = face
        .strokes()
        .into_iter()
        .map(|s| {
            s.into_iter()
                .map(|uv| {
                    let (u, v) = (uv.x.clamp(-0.95, 0.95), uv.y.clamp(-0.95, 0.95));
                    let z = (1.0 - u * u - v * v).max(0.0).sqrt();
                    let local = c + Vec3::new(u * r.x, v * r.y, z * r.z) * 1.01;
                    m.transform_point3(local)
                })
                .collect()
        })
        .collect();
    let scale = p.world.transform_vector3(Vec3::Y).length();
    (strokes, hh * scale)
}
