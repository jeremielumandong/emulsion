//! `emulsion-scene` — 3D sets for the Storyboard Shot Generator (phase 11).
//!
//! # Design note
//!
//! This crate has no UI and no GPU dependency. It owns the data model of a
//! Shot Generator set and everything that can be computed from it, so the
//! viewport, snapshots, exports, MCP tools and tests all share one
//! implementation and get identical pixels.
//!
//! **Model.** A [`Scene`] is plain serde data (metres, Y up, ground at
//! y = 0, objects face +Z): a list of [`SceneObject`]s with stable
//! [`ObjectId`]s that are never reused, one shot [`Camera`] and an
//! [`Environment`] (sky, ground, grid, horizon, ambient). Objects are
//! characters ([`Character`]: mannequin sliders, [`Pose`], IK targets,
//! look-at, face), props ([`Prop`]: a parametric [`BuiltinProp`] or a
//! [`ModelRef`] to an imported model) and directional lights. Every struct
//! uses `#[serde(default)]` so older files load; [`Scene::validate`] enforces
//! finiteness and the [`limits`]. Imported models live outside the scene in
//! an [`AssetLibrary`] keyed by the id a `ModelRef` names; the host stores the
//! files (e.g. in the project) and refills the library on load.
//!
//! **Characters.** Mannequins are generated procedurally from sliders
//! ([`mannequin::generate`]): ellipsoids and tapered capsules rigidly attached
//! to a fixed 31-bone [`Bone`] skeleton. Every bone's rest frame is aligned
//! with the character frame, so joint angles ([`JointRotation`], degrees,
//! `Rx·Rz·Ry`) are readable and mirror by negating y and z. Evaluation
//! ([`character::evaluate`]) runs FK, ground lock, two-bone IK (hinge elbows
//! and knees, elbow-circle search that respects joint limits) and head
//! look-at. Poses are data: the library ([`PosePreset`]) is authored in code
//! (partly with IK) but serializes like any custom pose.
//!
//! **Rendering.** [`prepare`] tessellates a scene into world-space triangles
//! ([`PreparedScene`]) — cache it while only the camera moves. [`render`] is a
//! tiled CPU rasterizer (rayon strips; deterministic) with a z-buffer,
//! culling, near clipping, toon/clay/outline/silhouette styles, key-light
//! shadows (an orthographic shadow map fitted to the shadow casters, PCF
//! filtered; shadowed toon surfaces take the lowest band), textured and
//! vertex-coloured albedo for imported models ([`MeshAlbedo`]), contour
//! lines from id/depth/normal discontinuities, drawn faces, grid and
//! horizon. It outputs straight-alpha RGBA8 for any size.
//!
//! **Shots.** [`frame_shot`] turns a [`ShotSpec`] (size ECU…EWS, angle preset,
//! side, optional body-part focus, lens) into a camera that frames the subject
//! by anatomy (crown to the size's cut line, with fixed headroom).
//! [`explore_shots`] proposes varied setups; [`text_to_shot`] parses a phrase
//! with data-driven vocabulary tables ([`text::vocabulary`]) and builds a
//! framed scene.
//!
//! **Interop.** [`render_to_rgba`], [`project_point`] (attach 2D layers, C12),
//! [`pick`]/[`pick_joint`] (viewport selection), gizmo setters on [`Scene`]
//! (`translate`, `rotate_about`, `set_scale`, `set_joint`, `set_ik_target`,
//! `bake_pose`) and [`Camera::top_view`]/[`Camera::side_view`] for the
//! orthographic V6 views.
//!
//! Import ([`import_file`], [`import_bytes`]) reads glTF 2.0 (`.gltf`/`.glb`,
//! meshes, node transforms, materials' base colour and base-colour texture
//! (PNG/JPEG via the `image` crate, bounded decoded size), vertex colours,
//! skins posed by joint name) and OBJ by hand with bounded sizes; bad input
//! returns
//! [`SceneError`], never panics.

pub mod camera;
pub mod character;
pub mod error;
pub mod import;
pub mod interop;
pub mod mannequin;
pub mod math;
pub mod mesh;
pub mod pose;
pub mod prepare;
pub mod props;
pub mod render;
pub mod scene;
pub mod shot;
pub mod skeleton;
pub mod text;
pub mod texture;

pub use camera::{
    Camera, FilmBack, Projection, ScreenPoint, View, focal_length_for_fov, horizontal_fov_deg,
};
pub use character::{PosedCharacter, pose_character};
pub use error::SceneError;
pub use import::{AssetLibrary, ImportedModel, ModelFormat, import_bytes, import_file};
pub use interop::{
    PickHit, SurfaceFrame, attachment_matrix, joint_handles, pick, pick_joint, project_point,
    render_to_rgba,
};
pub use mannequin::{MannequinKind, MannequinParams};
pub use math::{Aabb, Rotation, Transform};
pub use mesh::Mesh;
pub use pose::{FacePreset, HandShape, Limb, Pose, PosePreset, pose_library};
pub use prepare::{PartLabel, PreparedScene, prepare};
pub use props::{BuiltinProp, PropKind};
pub use render::{RenderOptions, RenderStyle, RgbaImage, render};
pub use scene::{
    Character, Environment, IkTarget, Light, LightKind, ModelRef, ObjectId, ObjectKind, Prop, Rgb,
    Scene, SceneObject, limits,
};
pub use shot::{
    CameraAngle, Framing, ShotProposal, ShotSide, ShotSize, ShotSpec, explore_shots, frame_shot,
};
pub use skeleton::{Bone, JointRotation};
pub use text::{GeneratedShot, ShotDescription, parse_shot, text_to_shot};
pub use texture::{MeshAlbedo, Texture, Wrap};
