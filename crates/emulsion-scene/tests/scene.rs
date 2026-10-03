//! Integration tests: scene model, framing, renderer, import, text to shot.

use std::collections::BTreeMap;
use std::path::PathBuf;

use emulsion_scene::*;
use glam::Vec3;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Writes `img` as raw RGBA to `$EMULSION_SCENE_DUMP/<name>.rgba` (for eyeballing).
fn dump(name: &str, img: &RgbaImage) {
    if let Ok(dir) = std::env::var("EMULSION_SCENE_DUMP") {
        let p = PathBuf::from(dir).join(format!("{name}_{}x{}.rgba", img.width, img.height));
        let _ = std::fs::write(p, &img.pixels);
    }
}

fn demo_scene() -> Scene {
    let mut s = Scene::new();
    s.add_character(
        "Mia",
        Character::of(MannequinKind::AdultFemale).with_pose(PosePreset::Wave),
        Vec3::new(-0.8, 0.0, 0.0),
        20.0,
    );
    s.add_character(
        "Tom",
        Character::of(MannequinKind::AdultMale).with_pose(PosePreset::Point),
        Vec3::new(0.8, 0.0, 0.3),
        -25.0,
    );
    s.add_character(
        "Kid",
        Character::of(MannequinKind::Child).with_pose(PosePreset::Sit),
        Vec3::new(0.0, 0.0, -1.2),
        0.0,
    );
    s.add_prop(
        "Table",
        Prop::builtin(PropKind::Table),
        Vec3::new(0.0, 0.0, 1.4),
        0.0,
    );
    s
}

// ---------------------------------------------------------------- model

#[test]
fn scene_serde_round_trip() {
    let s = demo_scene();
    s.validate().unwrap();
    let json = s.to_json();
    let back = Scene::from_json(&json).unwrap();
    assert_eq!(back, s);
    // Deterministic serialization.
    assert_eq!(back.to_json(), json);
}

#[test]
fn old_and_minimal_json_loads_with_defaults() {
    let json = r#"{
        "objects": [
            {"id": 3, "type": "character"},
            {"id": 5, "name": "Crate", "type": "prop", "source": "builtin", "kind": "box"},
            {"id": 7, "type": "light", "transform": {"rotation": {"yaw": 10, "pitch": -40, "roll": 0}}}
        ]
    }"#;
    let mut s: Scene = serde_json::from_str(json).unwrap();
    s.normalize();
    s.validate().unwrap();
    assert_eq!(s.next_id, 8);
    let c = s.character(ObjectId(3)).unwrap();
    assert!(c.ground_lock);
    assert_eq!(c.body.kind, MannequinKind::AdultNeutral);
    // New ids never collide.
    let id = s.add("Another", ObjectKind::Prop(Prop::builtin(PropKind::Sphere)));
    assert_eq!(id, ObjectId(8));
    s.remove(id);
    assert_eq!(
        s.add("Again", ObjectKind::Prop(Prop::builtin(PropKind::Sphere))),
        ObjectId(9)
    );
    // Unknown fields from newer writers are ignored.
    let s2: Scene = serde_json::from_str(r#"{"objects": [], "future_field": 1}"#).unwrap();
    s2.validate().unwrap();
}

#[test]
fn guide_example_json_loads() {
    let doc = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/guides/storyboard-3d.md"
    ))
    .unwrap();
    let start = doc.find("```json").unwrap() + 7;
    let end = start + doc[start..].find("```").unwrap();
    let s = Scene::from_json(&doc[start..end]).unwrap();
    assert_eq!(s.objects.len(), 3);
    assert_eq!(
        s.character(ObjectId(1)).unwrap().pose.right_hand,
        HandShape::Point
    );
    let img = render_to_rgba(&s, &AssetLibrary::new(), 64, 36, RenderStyle::Toon).unwrap();
    assert_eq!(img.pixels.len(), 64 * 36 * 4);
}

#[test]
fn validation_rejects_bad_scenes() {
    let mut s = demo_scene();
    s.objects[0].transform.scale = Vec3::new(0.0, 1.0, 1.0);
    assert!(s.validate().is_err());

    let mut s = demo_scene();
    s.objects[1].id = s.objects[0].id;
    assert!(s.validate().is_err());

    let mut s = demo_scene();
    s.camera.focal_length_mm = 0.5;
    assert!(s.validate().is_err());

    let mut s = demo_scene();
    s.objects[0].transform.position.x = f32::NAN;
    assert!(s.validate().is_err());

    let mut s = demo_scene();
    s.character_mut(s.character_ids()[0]).unwrap().body.build = 7.0;
    assert!(s.validate().is_err());
    s.normalize();
    s.validate().unwrap();

    let mut s = Scene::new();
    for i in 0..limits::MAX_OBJECTS + 1 {
        s.add(
            &format!("b{i}"),
            ObjectKind::Prop(Prop::builtin(PropKind::Box)),
        );
    }
    assert!(matches!(s.validate(), Err(SceneError::Limit(_))));
    assert!(Scene::from_json("{ not json").is_err());
}

#[test]
fn gizmo_setters() {
    let mut s = demo_scene();
    let id = s.character_ids()[0];
    assert!(s.translate(id, Vec3::new(1.0, 0.0, 0.0)));
    assert!((s.object(id).unwrap().transform.position.x - 0.2).abs() < 1e-5);
    assert!(s.rotate_about(id, Vec3::Y, 30.0));
    let (yaw, _, _) = s.object(id).unwrap().transform.rotation.to_yaw_pitch_roll();
    assert!((yaw - 50.0).abs() < 1e-3);
    assert!(s.set_scale(id, Vec3::splat(-1.0)));
    assert!(s.object(id).unwrap().transform.scale.min_element() > 0.0);
    // Joint setter clamps to limits.
    assert!(s.set_joint(id, Bone::LowerArmL, JointRotation::new(40.0, 0.0, 0.0)));
    assert_eq!(
        s.character(id).unwrap().pose.rotation(Bone::LowerArmL).x,
        0.0
    );
    assert!(!s.set_position(ObjectId(999), Vec3::ZERO));
}

// ---------------------------------------------------------------- mannequins

#[test]
fn mannequin_types_and_sliders() {
    for kind in MannequinKind::ALL {
        let m = mannequin::generate(&MannequinParams::of(kind));
        assert!(
            m.triangle_count() > 3000,
            "{kind:?}: {}",
            m.triangle_count()
        );
        assert!(m.triangle_count() < 20_000);
    }
    let child = MannequinParams::of(MannequinKind::Child).proportions();
    let adult = MannequinParams::of(MannequinKind::AdultMale).proportions();
    assert!(
        child.head_height / child.height > adult.head_height / adult.height,
        "children have bigger heads"
    );
    let female = MannequinParams::of(MannequinKind::AdultFemale).proportions();
    assert!(female.shoulder_half / female.hip_half < adult.shoulder_half / adult.hip_half);

    let base = MannequinParams::default();
    let width = |p: &MannequinParams| {
        let m = mannequin::generate(p);
        let s = Scene::default();
        let _ = s;
        let mut b = Aabb::EMPTY;
        for part in &m.parts {
            if part.bone == Bone::Spine {
                b = b.union(&part.mesh.bounds());
            }
        }
        b.size().x
    };
    let heavy = MannequinParams { build: 1.0, ..base };
    let slim = MannequinParams { build: 0.0, ..base };
    assert!(width(&heavy) > width(&slim) * 1.3);
    let tall = MannequinParams {
        height: Some(2.0),
        ..base
    };
    assert!((emulsion_scene::skeleton::Rig::new(&tall).height - 2.0).abs() < 1e-5);
    let long_legs = MannequinParams {
        leg_length: 1.2,
        ..base
    };
    let rl = emulsion_scene::skeleton::Rig::new(&long_legs);
    let rb = emulsion_scene::skeleton::Rig::new(&base);
    assert!(rl.heads[Bone::Hips.index()].y > rb.heads[Bone::Hips.index()].y + 0.05);
    let big_head = MannequinParams {
        head_size: 1.4,
        ..base
    };
    assert!(emulsion_scene::skeleton::Rig::new(&big_head).head_height > rb.head_height * 1.3);
}

// ---------------------------------------------------------------- framing

fn single_character_scene(pose: PosePreset) -> (Scene, ObjectId) {
    let mut s = Scene::new();
    let id = s.add_character(
        "A",
        Character::default().with_pose(pose),
        Vec3::new(0.5, 0.0, 2.0),
        30.0,
    );
    (s, id)
}

#[test]
fn shot_sizes_fill_the_expected_fraction() {
    let (s, id) = single_character_scene(PosePreset::Stand);
    let posed = pose_character(s.object(id).unwrap()).unwrap();
    let (w, h) = (1920u32, 1080u32);
    let crown = posed.crown_world();
    let cases = [
        (
            ShotSize::CloseUp,
            posed.joint_world(Bone::Head) - Vec3::Y * posed.rig.head_height * 0.35,
        ),
        (
            ShotSize::MediumCloseUp,
            (posed.joint_world(Bone::Chest) + posed.joint_world(Bone::Neck)) * 0.5,
        ),
        (ShotSize::Medium, posed.joint_world(Bone::Spine)),
        (
            ShotSize::MediumWide,
            (posed.joint_world(Bone::LowerLegL) + posed.joint_world(Bone::LowerLegR)) * 0.5,
        ),
    ];
    for (size, cut) in cases {
        for angle in [CameraAngle::EyeLevel, CameraAngle::Low, CameraAngle::High] {
            let spec = ShotSpec {
                angle,
                ..ShotSpec::new(id, size)
            };
            let cam = frame_shot(&s, &AssetLibrary::new(), &spec, w as f32 / h as f32).unwrap();
            let top = project_point(&cam, w, h, crown).unwrap();
            let bottom = project_point(&cam, w, h, cut).unwrap();
            let frac = (bottom.y - top.y) / h as f32;
            let tol = if angle == CameraAngle::EyeLevel {
                0.04
            } else {
                0.15
            };
            assert!(
                (frac - 1.0 / 1.12).abs() < tol,
                "{size:?} {angle:?}: subject fills {frac}"
            );
            assert!(top.y > 0.0, "{size:?} {angle:?}: head cut off");
            // The subject is centred horizontally.
            assert!(
                (top.x - w as f32 / 2.0).abs() < w as f32 * 0.08,
                "{size:?}: x {}",
                top.x
            );
        }
    }
    // Wide: the whole figure fits; extreme wide: it is small.
    let ws = frame_shot(
        &s,
        &AssetLibrary::new(),
        &ShotSpec::new(id, ShotSize::Wide),
        16.0 / 9.0,
    )
    .unwrap();
    let top = project_point(&ws, w, h, crown).unwrap();
    let feet = project_point(&ws, w, h, posed.tip_world(Bone::FootL)).unwrap();
    assert!(top.y > 0.0 && feet.y <= h as f32 + 1.0 && (feet.y - top.y) / (h as f32) > 0.75);
    let ews = frame_shot(
        &s,
        &AssetLibrary::new(),
        &ShotSpec::new(id, ShotSize::ExtremeWide),
        16.0 / 9.0,
    )
    .unwrap();
    let t = project_point(&ews, w, h, crown).unwrap();
    let f = project_point(&ews, w, h, posed.tip_world(Bone::FootL)).unwrap();
    assert!((f.y - t.y) / (h as f32) < 0.25);
    // ECU: the eyes are centred and the head overfills the frame.
    let ecu = frame_shot(
        &s,
        &AssetLibrary::new(),
        &ShotSpec::new(id, ShotSize::ExtremeCloseUp),
        16.0 / 9.0,
    )
    .unwrap();
    let e = project_point(&ecu, w, h, posed.eyes_world()).unwrap();
    assert!((e.y - h as f32 / 2.0).abs() < 2.0);
    assert!(project_point(&ecu, w, h, crown).unwrap().y < 0.0);
}

#[test]
fn angles_place_the_camera() {
    let (s, id) = single_character_scene(PosePreset::Stand);
    let posed = pose_character(s.object(id).unwrap()).unwrap();
    let eyes = posed.eyes_world();
    let get = |angle: CameraAngle, side: ShotSide| {
        frame_shot(
            &s,
            &AssetLibrary::new(),
            &ShotSpec {
                angle,
                side,
                ..ShotSpec::new(id, ShotSize::Medium)
            },
            1.5,
        )
        .unwrap()
    };
    let low = get(CameraAngle::Low, ShotSide::Front);
    let high = get(CameraAngle::High, ShotSide::Front);
    let bird = get(CameraAngle::BirdsEye, ShotSide::Front);
    let worm = get(CameraAngle::WormsEye, ShotSide::Front);
    let dutch = get(CameraAngle::Dutch, ShotSide::Front);
    assert!(low.position.y < eyes.y && low.pitch > 5.0);
    assert!(high.position.y > eyes.y && high.pitch < -5.0);
    assert!(bird.pitch < -70.0);
    assert!(worm.pitch > 30.0 && worm.position.y >= 0.08);
    assert!(dutch.roll.abs() > 10.0);
    // Front: the camera is in front of the face; back: behind it.
    let front = get(CameraAngle::EyeLevel, ShotSide::Front);
    let back = get(CameraAngle::EyeLevel, ShotSide::Back);
    let facing = posed.facing_world();
    assert!((front.position - eyes).dot(facing) > 0.0);
    assert!((back.position - eyes).dot(facing) < 0.0);
}

#[test]
fn over_the_shoulder_and_two_shot() {
    let mut s = Scene::new();
    let mia = s.add_character("Mia", Character::default(), Vec3::new(0.6, 0.0, 0.0), -90.0);
    let tom = s.add_character("Tom", Character::default(), Vec3::new(-0.6, 0.0, 0.0), 90.0);
    let lib = AssetLibrary::new();
    let ots = frame_shot(
        &s,
        &lib,
        &ShotSpec {
            angle: CameraAngle::OverTheShoulder,
            secondary: Some(tom),
            ..ShotSpec::new(mia, ShotSize::MediumCloseUp)
        },
        16.0 / 9.0,
    )
    .unwrap();
    // The camera is behind Tom (further from Mia than Tom is), looking at Mia.
    assert!(ots.position.x < -0.6, "{:?}", ots.position);
    let pm = pose_character(s.object(mia).unwrap()).unwrap();
    let p = project_point(&ots, 1920, 1080, pm.eyes_world()).unwrap();
    assert!(p.x > 0.0 && p.x < 1920.0 && p.y > 0.0 && p.y < 1080.0);
    let two = frame_shot(
        &s,
        &lib,
        &ShotSpec {
            angle: CameraAngle::TwoShot,
            secondary: Some(tom),
            ..ShotSpec::new(mia, ShotSize::Medium)
        },
        16.0 / 9.0,
    )
    .unwrap();
    let pt = pose_character(s.object(tom).unwrap()).unwrap();
    for c in [&pm, &pt] {
        let q = project_point(&two, 1920, 1080, c.crown_world()).unwrap();
        assert!(
            q.x > 0.0 && q.x < 1920.0 && q.y > 0.0,
            "both heads in frame: {q:?}"
        );
    }
}

#[test]
fn shot_explorer_proposes_varied_named_setups() {
    let s = demo_scene();
    let id = s.character_ids()[0];
    let props = explore_shots(&s, &AssetLibrary::new(), id, 12, 16.0 / 9.0).unwrap();
    assert_eq!(props.len(), 12);
    let names: std::collections::HashSet<_> = props.iter().map(|p| p.name.clone()).collect();
    assert_eq!(names.len(), 12, "{names:?}");
    let sizes: std::collections::HashSet<_> = props.iter().map(|p| p.spec.size).collect();
    let angles: std::collections::HashSet<_> = props.iter().map(|p| p.spec.angle).collect();
    let sides: std::collections::HashSet<_> = props.iter().map(|p| p.spec.side).collect();
    assert!(
        sizes.len() >= 6 && angles.len() >= 5 && sides.len() >= 4,
        "{sizes:?} {angles:?} {sides:?}"
    );
    assert!(props.iter().all(|p| p.camera.position.is_finite()));
    // Deterministic.
    let again = explore_shots(&s, &AssetLibrary::new(), id, 12, 16.0 / 9.0).unwrap();
    assert_eq!(again, props);
}

// ---------------------------------------------------------------- renderer

fn boxes_scene() -> Scene {
    let mut s = Scene::new();
    s.environment.show_grid = false;
    s.environment.show_horizon = false;
    let near = s.add_prop(
        "Near",
        Prop::Builtin(BuiltinProp::new(PropKind::Box).with_size(Vec3::splat(1.0))),
        Vec3::new(-0.3, 0.0, 0.0),
        0.0,
    );
    let far = s.add_prop(
        "Far",
        Prop::Builtin(BuiltinProp::new(PropKind::Box).with_size(Vec3::splat(1.0))),
        Vec3::new(0.3, 0.0, 2.0),
        0.0,
    );
    s.object_mut(near).unwrap().color = Rgb([220, 40, 40]);
    s.object_mut(far).unwrap().color = Rgb([40, 40, 220]);
    s.camera = Camera::default();
    s.camera.position = Vec3::new(0.0, 0.5, -5.0);
    s.camera.look_at(Vec3::new(0.0, 0.5, 0.0));
    s
}

fn is_reddish(p: [u8; 4]) -> bool {
    p[0] as i32 > p[2] as i32 + 60
}
fn is_bluish(p: [u8; 4]) -> bool {
    p[2] as i32 > p[0] as i32 + 60
}

#[test]
fn render_depth_ordering_and_size() {
    let s = boxes_scene();
    let lib = AssetLibrary::new();
    let img = render_to_rgba(&s, &lib, 160, 90, RenderStyle::Toon).unwrap();
    dump("boxes", &img);
    assert_eq!(img.pixels.len(), 160 * 90 * 4);
    // Where both boxes overlap (near box's right half), red wins.
    let view = s.camera.view(160, 90);
    let near_c = view.project(Vec3::new(-0.1, 0.5, -0.5)).unwrap();
    let far_only = view.project(Vec3::new(0.7, 0.5, 1.5)).unwrap();
    assert!(
        is_reddish(img.pixel(near_c.x as u32, near_c.y as u32)),
        "{:?}",
        img.pixel(near_c.x as u32, near_c.y as u32)
    );
    assert!(is_bluish(img.pixel(far_only.x as u32, far_only.y as u32)));
    // Swap the camera to the other side: blue now occludes red.
    let mut s2 = s.clone();
    s2.camera.position = Vec3::new(0.0, 0.5, 7.0);
    s2.camera.look_at(Vec3::new(0.0, 0.5, 0.0));
    let img2 = render_to_rgba(&s2, &lib, 160, 90, RenderStyle::Toon).unwrap();
    let v2 = s2.camera.view(160, 90);
    let c = v2.project(Vec3::new(0.0, 0.5, 2.5)).unwrap();
    assert!(is_bluish(img2.pixel(c.x as u32, c.y as u32)));
}

#[test]
fn back_faces_are_culled_and_near_plane_clips() {
    let mut s = Scene::new();
    s.environment.show_ground = false;
    s.environment.show_grid = false;
    s.environment.show_horizon = false;
    // A wall seen from inside a box: the camera sits inside a big cube, so
    // every face points away and nothing is drawn.
    s.add_prop(
        "Room",
        Prop::Builtin(BuiltinProp::new(PropKind::Box).with_size(Vec3::splat(10.0))),
        Vec3::new(0.0, -5.0, 0.0),
        0.0,
    );
    s.camera.position = Vec3::new(0.0, 0.0, 0.0);
    s.camera.look_at(Vec3::new(0.0, 0.0, 1.0));
    let lib = AssetLibrary::new();
    let prepared = prepare(&s, &lib).unwrap();
    let img = render(
        &prepared,
        &s.camera,
        64,
        36,
        &RenderOptions {
            faces: false,
            ..RenderOptions::style(RenderStyle::Silhouette)
        },
    );
    assert!(
        img.pixels.chunks(4).all(|p| p[0] > 200),
        "inside faces must be culled"
    );
    // A slab running under and behind the camera straddles the near plane:
    // it is clipped, not dropped or exploded.
    let mut s = Scene::new();
    s.environment.show_ground = false;
    s.add_prop(
        "Slab",
        Prop::Builtin(BuiltinProp::new(PropKind::Box).with_size(Vec3::new(4.0, 0.2, 20.0))),
        Vec3::new(0.0, -0.7, 0.0),
        0.0,
    );
    s.camera.position = Vec3::new(0.0, 0.0, 0.0);
    s.camera.near = 0.05;
    s.camera.look_at(Vec3::new(0.0, -0.3, 5.0));
    let prepared = prepare(&s, &lib).unwrap();
    let img = render(
        &prepared,
        &s.camera,
        64,
        36,
        &RenderOptions::style(RenderStyle::Silhouette),
    );
    let bottom_dark = (0..64).filter(|x| img.pixel(*x, 35)[0] < 30).count();
    assert!(
        bottom_dark > 40,
        "slab fills the bottom edge: {bottom_dark}"
    );
    assert!(img.pixel(32, 0)[0] > 200, "sky above");
}

#[test]
fn contours_toon_bands_and_styles() {
    let mut s = Scene::new();
    s.environment.show_grid = false;
    s.environment.show_horizon = false;
    s.add_prop(
        "Ball",
        Prop::Builtin(BuiltinProp::new(PropKind::Sphere).with_size(Vec3::splat(1.0))),
        Vec3::ZERO,
        0.0,
    );
    s.camera.position = Vec3::new(0.0, 0.5, -3.0);
    s.camera.look_at(Vec3::new(0.0, 0.5, 0.0));
    let lib = AssetLibrary::new();
    let prepared = prepare(&s, &lib).unwrap();
    let (w, h) = (200u32, 120u32);
    let toon = render(
        &prepared,
        &s.camera,
        w,
        h,
        &RenderOptions {
            line_width: 2.0,
            ..RenderOptions::default()
        },
    );
    dump("ball_toon", &toon);
    let view = s.camera.view(w, h);
    // The silhouette row through the centre: a dark contour at the left edge.
    let c = view.project(Vec3::new(0.0, 0.5, 0.0)).unwrap();
    let y = c.y as u32;
    let row: Vec<[u8; 4]> = (0..w).map(|x| toon.pixel(x, y)).collect();
    let first_dark = row.iter().position(|p| p[0] < 90 && p[1] < 90 && p[2] < 90);
    assert!(first_dark.is_some(), "contour line present");
    // Toon: interior shades fall into at most three bands (plus antialiased
    // line pixels, excluded by staying away from edges).
    let mut shades = std::collections::BTreeSet::new();
    let r = (view.project(Vec3::new(0.5, 0.5, 0.0)).unwrap().x - c.x).abs() * 0.8;
    for yy in (c.y - r) as u32..(c.y + r) as u32 {
        for xx in (c.x - r) as u32..(c.x + r) as u32 {
            let dx = xx as f32 - c.x;
            let dy = yy as f32 - c.y;
            if dx * dx + dy * dy < r * r {
                let p = toon.pixel(xx, yy);
                if p[0] > 90 {
                    shades.insert(p[0]);
                }
            }
        }
    }
    assert!((2..=3).contains(&shades.len()), "toon bands: {shades:?}");
    let two = render(
        &prepared,
        &s.camera,
        w,
        h,
        &RenderOptions {
            toon_bands: 2,
            ..RenderOptions::default()
        },
    );
    let mut s2 = std::collections::BTreeSet::new();
    for p in two.pixels.chunks(4) {
        s2.insert([p[0], p[1], p[2]]);
    }
    // Clay is grey; silhouette has no lines and black shapes; outline is
    // white with lines; transparent background leaves alpha 0.
    let clay = render(
        &prepared,
        &s.camera,
        w,
        h,
        &RenderOptions::style(RenderStyle::Clay),
    );
    let p = clay.pixel(c.x as u32, c.y as u32);
    assert!(p[0] == p[1] && p[1] == p[2]);
    let sil = render(
        &prepared,
        &s.camera,
        w,
        h,
        &RenderOptions::style(RenderStyle::Silhouette),
    );
    assert!(sil.pixel(c.x as u32, c.y as u32)[0] < 20);
    let out = render(
        &prepared,
        &s.camera,
        w,
        h,
        &RenderOptions {
            transparent_background: true,
            show_ground: Some(false),
            ..RenderOptions::style(RenderStyle::Outline)
        },
    );
    assert_eq!(out.pixel(c.x as u32, c.y as u32), [255, 255, 255, 255]);
    assert_eq!(out.pixel(2, 2)[3], 0, "transparent background");
    let edge = (0..w).map(|x| out.pixel(x, y)).find(|p| p[3] > 0).unwrap();
    assert!(edge[0] < 128, "outline style draws the contour: {edge:?}");
    // Thicker lines darken more pixels.
    let count_dark = |img: &RgbaImage| img.pixels.chunks(4).filter(|p| p[0] < 100).count();
    let thin = render(
        &prepared,
        &s.camera,
        w,
        h,
        &RenderOptions {
            line_width: 1.0,
            ..RenderOptions::style(RenderStyle::Outline)
        },
    );
    let thick = render(
        &prepared,
        &s.camera,
        w,
        h,
        &RenderOptions {
            line_width: 4.0,
            ..RenderOptions::style(RenderStyle::Outline)
        },
    );
    assert!(count_dark(&thick) > count_dark(&thin) * 2);
}

#[test]
fn ground_grid_horizon_and_ortho_views() {
    let mut s = Scene::new();
    s.camera.position = Vec3::new(0.0, 1.6, -6.0);
    s.camera.yaw = 0.0;
    s.camera.pitch = 0.0;
    let lib = AssetLibrary::new();
    let prepared = prepare(&s, &lib).unwrap();
    let img = render(&prepared, &s.camera, 160, 90, &RenderOptions::default());
    dump("ground", &img);
    // A level camera puts the horizon on the middle row.
    let lum = |y: u32| (0..160).map(|x| img.pixel(x, y)[0] as u32).sum::<u32>() / 160;
    let darkest = (40..50).min_by_key(|y| lum(*y)).unwrap();
    assert!((44..=45).contains(&darkest), "horizon at row {darkest}");
    assert!(
        lum(darkest) + 30 < lum(30),
        "horizon line darker: {} vs {}",
        lum(darkest),
        lum(30)
    );
    // Grid lines vary the ground.
    let ground_row: std::collections::BTreeSet<u8> =
        (0..160).map(|x| img.pixel(x, 80)[0]).collect();
    assert!(ground_row.len() > 2);

    // Top view of a character: orthographic, sees the head from above.
    let mut s = demo_scene();
    let b = prepare(&s, &lib).unwrap().bounds;
    s.camera = Camera::top_view(&b, 1.0);
    let prepared = prepare(&s, &lib).unwrap();
    let top = render(
        &prepared,
        &s.camera,
        100,
        100,
        &RenderOptions::style(RenderStyle::Silhouette),
    );
    dump("top", &top);
    let dark = top.pixels.chunks(4).filter(|p| p[0] < 30).count();
    assert!(dark > 100, "figures visible from the top: {dark}");
    let side = Camera::side_view(&b, 1.0);
    let img = render(
        &prepared,
        &side,
        100,
        100,
        &RenderOptions::style(RenderStyle::Silhouette),
    );
    assert!(img.pixels.chunks(4).filter(|p| p[0] < 30).count() > 100);
}

#[test]
fn rendering_is_deterministic_across_thread_counts() {
    let s = demo_scene();
    let lib = AssetLibrary::new();
    let opts = RenderOptions {
        supersample: 2,
        highlight: Some(s.character_ids()[1]),
        ..RenderOptions::default()
    };
    let prepared = prepare(&s, &lib).unwrap();
    let a = render(&prepared, &s.camera, 240, 135, &opts);
    let b = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap()
        .install(|| render(&prepare(&s, &lib).unwrap(), &s.camera, 240, 135, &opts));
    assert_eq!(a.hash(), b.hash());
    assert_eq!(a, b);
    dump("demo", &a);
    // Highlight colour appears.
    assert!(a.pixels.chunks(4).any(|p| p[2] > 180 && p[0] < 80));
}

#[test]
fn faces_are_drawn_on_heads() {
    let mut s = Scene::new();
    s.environment.show_grid = false;
    let id = s.add_character("A", Character::default(), Vec3::ZERO, 0.0);
    let lib = AssetLibrary::new();
    s.camera = frame_shot(&s, &lib, &ShotSpec::new(id, ShotSize::CloseUp), 1.0).unwrap();
    let prepared = prepare(&s, &lib).unwrap();
    let with = render(&prepared, &s.camera, 128, 128, &RenderOptions::default());
    let without = render(
        &prepared,
        &s.camera,
        128,
        128,
        &RenderOptions {
            faces: false,
            ..RenderOptions::default()
        },
    );
    dump("face", &with);
    let dark = |img: &RgbaImage| img.pixels.chunks(4).filter(|p| p[0] < 90).count();
    assert!(dark(&with) > dark(&without) + 20);
    // Different expressions draw differently.
    s.character_mut(id).unwrap().face = FacePreset::Surprised;
    let surprised = render(
        &prepare(&s, &lib).unwrap(),
        &s.camera,
        128,
        128,
        &RenderOptions::default(),
    );
    assert_ne!(surprised.hash(), with.hash());
}

// ---------------------------------------------------------------- interop

#[test]
fn picking_and_projection() {
    let s = demo_scene();
    let lib = AssetLibrary::new();
    let prepared = prepare(&s, &lib).unwrap();
    let (w, h) = (640, 360);
    let mut cam = s.camera;
    let tom = s.find("Tom").unwrap().id;
    let posed = prepared.character(tom).unwrap();
    let chest = posed.joint_world(Bone::Chest);
    cam.position = chest + Vec3::new(0.0, 0.3, 4.0);
    cam.look_at(chest);
    let p = project_point(&cam, w, h, chest).unwrap();
    let hit = pick(&prepared, &cam, w, h, p.x, p.y).expect("hit");
    assert_eq!(hit.object, tom);
    assert!(
        matches!(hit.bone(), Some(Bone::Chest | Bone::Spine)),
        "{:?}",
        hit.part
    );
    assert!((hit.point - chest).length() < 0.3);
    assert!(
        pick(&prepared, &cam, w, h, 2.0, 2.0).is_none()
            || pick(&prepared, &cam, w, h, 2.0, 2.0).unwrap().object != tom
    );
    let j = pick_joint(&prepared, &cam, w, h, p.x, p.y, 6.0).unwrap();
    assert_eq!(j.object, tom);
    let frame = SurfaceFrame::from_hit(&hit);
    assert!((frame.right.cross(frame.up) - frame.normal).length() < 1e-4);
    let m = attachment_matrix(&s, &prepared, tom, Some(Bone::HandR)).unwrap();
    assert!((m.w_axis.truncate() - posed.joint_world(Bone::HandR)).length() < 1e-4);
}

// ---------------------------------------------------------------- import

#[test]
fn gltf_import_embedded() {
    let m = import_file(&fixture("cube.gltf")).unwrap();
    assert_eq!(m.format, ModelFormat::Gltf);
    assert_eq!(m.triangle_count(), 12);
    let b = m.bounds();
    assert!(
        (b.min.y - 0.0).abs() < 1e-5 && (b.max.y - 1.0).abs() < 1e-5,
        "node translation applied: {b:?}"
    );
    assert_eq!(m.primitives[0].color, Rgb([255, 0, 0]));
    // Render it as a prop.
    let mut lib = AssetLibrary::new();
    lib.insert("cube", m).unwrap();
    let mut s = boxes_scene();
    s.objects.retain(|o| !matches!(o.kind, ObjectKind::Prop(_)));
    let id = s.add_prop(
        "Imported",
        Prop::Model(ModelRef {
            asset: "cube".into(),
            joint_rotations: BTreeMap::new(),
        }),
        Vec3::ZERO,
        0.0,
    );
    let prepared = prepare(&s, &lib).unwrap();
    assert!(prepared.warnings.is_empty());
    let img = render(&prepared, &s.camera, 80, 45, &RenderOptions::default());
    let c = project_point(&s.camera, 80, 45, Vec3::new(0.0, 0.5, -0.5)).unwrap();
    assert!(
        is_reddish(img.pixel(c.x as u32, c.y as u32)),
        "{:?}",
        img.pixel(c.x as u32, c.y as u32)
    );
    // A missing asset renders a placeholder with a warning.
    lib.remove("cube");
    let prepared = prepare(&s, &lib).unwrap();
    assert_eq!(prepared.warnings.len(), 1);
    assert!(!prepared.object_bounds(id).is_empty());
}

#[test]
fn glb_skin_import_and_posing() {
    let m = import_file(&fixture("arm.glb")).unwrap();
    assert_eq!(m.format, ModelFormat::Glb);
    assert_eq!(
        m.joint_names(),
        vec!["Root".to_string(), "Elbow".to_string()]
    );
    let rest = m.bounds();
    assert!((rest.max.y - 2.0).abs() < 1e-4, "{rest:?}");
    let mut rot = BTreeMap::new();
    rot.insert("Elbow".to_string(), JointRotation::new(0.0, 0.0, 90.0));
    let posed = m.posed(&rot);
    let b = posed
        .iter()
        .fold(Aabb::EMPTY, |a, p| a.union(&p.mesh.bounds()));
    // The upper half swings toward -X around the elbow at y = 1.
    assert!(b.min.x < -0.9 && b.max.y < 1.2, "{b:?}");
    // Picking labels triangles with the dominant joint.
    let mut lib = AssetLibrary::new();
    lib.insert("arm", m).unwrap();
    let mut s = Scene::new();
    let id = s.add_prop(
        "Arm",
        Prop::Model(ModelRef {
            asset: "arm".into(),
            joint_rotations: BTreeMap::new(),
        }),
        Vec3::ZERO,
        0.0,
    );
    s.camera.position = Vec3::new(0.0, 1.0, -4.0);
    s.camera.look_at(Vec3::new(0.0, 1.0, 0.0));
    let prepared = prepare(&s, &lib).unwrap();
    let p = project_point(&s.camera, 200, 200, Vec3::new(0.0, 1.8, -0.1)).unwrap();
    let hit = pick(&prepared, &s.camera, 200, 200, p.x, p.y).unwrap();
    assert_eq!(hit.object, id);
    assert_eq!(hit.part, PartLabel::Node("Elbow".into()));
}

#[test]
fn obj_import_groups_and_normals() {
    let m = import_file(&fixture("house.obj")).unwrap();
    assert_eq!(m.format, ModelFormat::Obj);
    assert_eq!(m.nodes.len(), 2);
    assert_eq!(m.nodes[0].name, "Body");
    assert_eq!(m.nodes[1].name, "Roof");
    assert_eq!(m.triangle_count(), 12 + 1);
    let b = m.bounds();
    assert!((b.max.y - 3.0).abs() < 1e-5);
    assert!(m.primitives.iter().all(|p| {
        p.mesh
            .normals
            .iter()
            .all(|n| (n.length() - 1.0).abs() < 1e-3)
    }));
}

#[test]
fn bad_model_files_error_cleanly() {
    let cases: Vec<(ModelFormat, Vec<u8>)> = vec![
        (ModelFormat::Glb, b"glTF".to_vec()),
        (ModelFormat::Glb, b"glTF\x02\0\0\0\xff\xff\xff\xff\x10\0\0\0JSON{}".to_vec()),
        (ModelFormat::Glb, b"nope".to_vec()),
        (ModelFormat::Gltf, b"{".to_vec()),
        (ModelFormat::Gltf, br#"{"asset":{"version":"1.0"}}"#.to_vec()),
        (ModelFormat::Gltf, br#"{"asset":{"version":"2.0"}}"#.to_vec()),
        (ModelFormat::Gltf, br#"{"asset":{"version":"2.0"},"extensionsRequired":["KHR_draco_mesh_compression"]}"#.to_vec()),
        (
            ModelFormat::Gltf,
            br#"{"asset":{"version":"2.0"},"buffers":[{"byteLength":4,"uri":"data:application/octet-stream;base64,AAAAAA=="}],
               "bufferViews":[{"buffer":0,"byteLength":4}],
               "accessors":[{"bufferView":0,"componentType":5126,"count":100,"type":"VEC3"}],
               "meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}],"nodes":[{"mesh":0}]}"#
                .to_vec(),
        ),
        (
            ModelFormat::Gltf,
            br#"{"asset":{"version":"2.0"},"nodes":[{"children":[1]},{"children":[0]}]}"#.to_vec(),
        ),
        (
            ModelFormat::Gltf,
            br#"{"asset":{"version":"2.0"},"buffers":[{"byteLength":4,"uri":"../../etc/passwd"}]}"#.to_vec(),
        ),
        (ModelFormat::Obj, b"v 0 0 0\nf 1 2 3\n".to_vec()),
        (ModelFormat::Obj, b"v 0 0 nan\n".to_vec()),
        (ModelFormat::Obj, b"# empty\n".to_vec()),
        (ModelFormat::Obj, b"v 0 0 0\nv 1 0 0\nf 1 2\n".to_vec()),
    ];
    for (i, (fmt, bytes)) in cases.into_iter().enumerate() {
        let r = import_bytes("bad", fmt, &bytes, None);
        assert!(r.is_err(), "case {i} should fail");
    }
    // Truncated real files never panic.
    let glb = std::fs::read(fixture("arm.glb")).unwrap();
    for n in (0..glb.len()).step_by(37) {
        let _ = import_bytes("t", ModelFormat::Glb, &glb[..n], None);
    }
    // Random byte flips never panic either.
    let mut rng = 12345u64;
    for _ in 0..200 {
        let mut b = glb.clone();
        for _ in 0..4 {
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let i = (rng >> 33) as usize % b.len();
            b[i] = (rng >> 20) as u8;
        }
        let _ = import_bytes("f", ModelFormat::Glb, &b, None);
    }
}

// ---------------------------------------------------------------- text to shot

#[test]
fn parser_cases() {
    let d = parse_shot("low-angle close-up of two people at a table");
    assert_eq!(d.size, Some(ShotSize::CloseUp));
    assert_eq!(d.angle, Some(CameraAngle::Low));
    assert_eq!(d.subjects.len(), 2);
    assert_eq!(d.props, vec![PropKind::Table]);
    assert!(d.unrecognized.is_empty(), "{:?}", d.unrecognized);

    let d = parse_shot("Wide shot of a woman running down a street");
    assert_eq!(d.size, Some(ShotSize::Wide));
    assert_eq!(d.subjects.len(), 1);
    assert_eq!(d.subjects[0].kind, MannequinKind::AdultFemale);
    assert_eq!(d.subjects[0].pose, Some(PosePreset::Run));
    assert_eq!(d.setting, Some(text::Setting::Street));

    let d = parse_shot("Over-the-shoulder of Mia talking to Tom");
    assert_eq!(d.angle, Some(CameraAngle::OverTheShoulder));
    let names: Vec<&str> = d.subjects.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["Mia", "Tom"]);
    assert!(d.facing);

    let d = parse_shot("bird's eye view of a car");
    assert_eq!(d.angle, Some(CameraAngle::BirdsEye));
    assert_eq!(d.props, vec![PropKind::Car]);
    assert!(d.subjects.is_empty());

    let d = parse_shot("extreme close up of a hand");
    assert_eq!(d.size, Some(ShotSize::ExtremeCloseUp));
    assert_eq!(d.focus, Some(Bone::HandR));
    assert_eq!(d.subjects.len(), 1);

    let d = parse_shot("Dutch angle medium shot of an angry man pointing, 85mm");
    assert_eq!(d.angle, Some(CameraAngle::Dutch));
    assert_eq!(d.size, Some(ShotSize::Medium));
    assert_eq!(d.focal_length_mm, Some(85.0));
    assert_eq!(d.subjects[0].face, FacePreset::Angry);
    assert_eq!(d.subjects[0].pose, Some(PosePreset::Point));

    let d = parse_shot("a child sitting on the floor, telephoto");
    assert_eq!(d.subjects[0].kind, MannequinKind::Child);
    assert_eq!(d.subjects[0].pose, Some(PosePreset::SitOnFloor));
    assert_eq!(d.focal_length_mm, Some(135.0));

    let d = parse_shot("worm's-eye shot of three soldiers zorbing");
    assert_eq!(d.angle, Some(CameraAngle::WormsEye));
    assert_eq!(d.subjects.len(), 3);
    assert_eq!(d.unrecognized, vec!["zorbing".to_string()]);

    let d = parse_shot("a family of four in a park");
    let kinds: Vec<MannequinKind> = d.subjects.iter().map(|s| s.kind).collect();
    assert_eq!(
        kinds,
        vec![
            MannequinKind::AdultMale,
            MannequinKind::AdultFemale,
            MannequinKind::Child,
            MannequinKind::Child
        ]
    );
    let d = parse_shot("two men and a woman");
    assert_eq!(d.subjects.len(), 3);
}

#[test]
fn vocabulary_tables_are_consistent() {
    let terms = text::all_terms();
    let mut seen = std::collections::HashMap::new();
    for (phrase, term) in &terms {
        assert_eq!(phrase.trim(), *phrase);
        assert_eq!(phrase.to_lowercase(), *phrase, "{phrase}");
        assert!(!phrase.contains("  ") && !phrase.contains('-'), "{phrase}");
        if let Some(prev) = seen.insert(*phrase, *term) {
            panic!("duplicate phrase `{phrase}`: {prev:?} vs {term:?}");
        }
    }
    // Every shot size, angle, pose and prop has at least one phrase.
    for s in ShotSize::ALL {
        assert!(
            terms.iter().any(|(_, t)| *t == text::Term::Size(s)),
            "{s:?}"
        );
    }
    for a in CameraAngle::ALL {
        assert!(
            terms.iter().any(|(_, t)| *t == text::Term::Angle(a)),
            "{a:?}"
        );
    }
    for p in PosePreset::ALL {
        let ok = terms.iter().any(|(_, t)| *t == text::Term::Pose(p))
            || matches!(p, PosePreset::SitOnFloor | PosePreset::RelaxedStand);
        assert!(ok, "{p:?}");
    }
    for k in PropKind::ALL {
        let ok = terms.iter().any(|(_, t)| *t == text::Term::Prop(k)) || k == PropKind::Floor;
        assert!(ok, "{k:?}");
    }
}

#[test]
fn text_to_shot_builds_framed_scenes() {
    let cases = [
        "low-angle close-up of two people at a table",
        "wide shot of a woman running down a street",
        "over-the-shoulder of Mia talking to Tom",
        "bird's eye view of a car",
        "extreme close up of a hand",
        "high angle wide shot of a family of three in a park",
        "two shot of a couple on a sofa",
        "a man lying on a bed, from above",
        "establishing shot of a street",
    ];
    for text in cases {
        let shot = text_to_shot(text, 16.0 / 9.0).unwrap_or_else(|e| panic!("{text}: {e}"));
        shot.scene.validate().unwrap();
        assert!(!shot.interpretation.is_empty());
        // The subject is in frame.
        let lib = AssetLibrary::new();
        let prepared = prepare(&shot.scene, &lib).unwrap();
        let target = match (prepared.character(shot.spec.subject), shot.spec.focus) {
            (Some(c), Some(bone)) => c.joint_world(bone),
            (Some(c), None) => c.eyes_world(),
            (None, _) => prepared.object_bounds(shot.spec.subject).center(),
        };
        let p = project_point(&shot.scene.camera, 1920, 1080, target)
            .unwrap_or_else(|| panic!("{text}: subject behind camera"));
        assert!(
            p.x > -50.0 && p.x < 1970.0 && p.y > -50.0 && p.y < 1130.0,
            "{text}: subject at {p:?}"
        );
        let img = render(
            &prepared,
            &shot.scene.camera,
            96,
            54,
            &RenderOptions::default(),
        );
        dump(&text.replace(' ', "_").replace('\'', ""), &img);
    }
    let table = text_to_shot("low-angle close-up of two people at a table", 1.5).unwrap();
    let chars = table.scene.character_ids();
    assert_eq!(chars.len(), 2);
    assert!(
        table
            .scene
            .objects
            .iter()
            .filter(|o| o.name.starts_with("Chair"))
            .count()
            == 2
    );
    assert_eq!(table.scene.character(chars[0]).unwrap().pose.name, "sit");
    assert!(table.scene.camera.pitch > 5.0, "low angle looks up");
    let ots = text_to_shot("over-the-shoulder of Mia talking to Tom", 1.5).unwrap();
    assert_eq!(ots.scene.find("Mia").unwrap().id, ots.spec.subject);
    assert_eq!(ots.spec.secondary, Some(ots.scene.find("Tom").unwrap().id));
    let car = text_to_shot("bird's eye view of a car", 1.5).unwrap();
    assert!(car.scene.camera.pitch < -70.0);
    assert!(car.scene.character_ids().is_empty());
    let family = text_to_shot(
        "high angle wide shot of a family of three in a park",
        16.0 / 9.0,
    )
    .unwrap();
    assert!(family.spec.group);
    let prepared = prepare(&family.scene, &AssetLibrary::new()).unwrap();
    for c in &prepared.characters {
        let p = project_point(&family.scene.camera, 1920, 1080, c.crown_world()).unwrap();
        assert!(
            p.x > 0.0 && p.x < 1920.0 && p.y > 0.0 && p.y < 1080.0,
            "group framing keeps everyone: {p:?}"
        );
    }
    let hand = text_to_shot("extreme close up of a hand", 1.5).unwrap();
    assert_eq!(hand.spec.focus, Some(Bone::HandR));
}

// ---------------------------------------------------------------- benchmark

/// Run with `cargo test --release -p emulsion-scene -- --ignored --nocapture bench`.
#[test]
#[ignore]
fn bench_viewport_render() {
    let mut s = Scene::new();
    let poses = [PosePreset::Walk, PosePreset::Point, PosePreset::Sit];
    for (i, p) in poses.iter().enumerate() {
        s.add_character(
            &format!("C{i}"),
            Character::default().with_pose(*p),
            Vec3::new(i as f32 - 1.0, 0.0, 0.0),
            0.0,
        );
    }
    let kinds = [
        PropKind::Table,
        PropKind::Chair,
        PropKind::Car,
        PropKind::Tree,
        PropKind::LampPost,
        PropKind::Door,
        PropKind::Sofa,
        PropKind::Stairs,
        PropKind::Box,
        PropKind::Wall,
    ];
    for (i, k) in kinds.iter().enumerate() {
        let a = i as f32 * 0.6;
        s.add_prop(
            k.name(),
            Prop::builtin(*k),
            Vec3::new(a.cos() * 5.0, 0.0, a.sin() * 5.0 + 3.0),
            i as f32 * 20.0,
        );
    }
    s.camera.position = Vec3::new(0.0, 1.7, -6.0);
    s.camera.look_at(Vec3::new(0.0, 1.0, 1.0));
    let lib = AssetLibrary::new();
    let n = 20;
    let t0 = std::time::Instant::now();
    let mut prepared = prepare(&s, &lib).unwrap();
    for _ in 1..n {
        prepared = prepare(&s, &lib).unwrap();
    }
    let prep = t0.elapsed() / n;
    let opts = RenderOptions::default();
    let _ = render(&prepared, &s.camera, 960, 540, &opts);
    let t1 = std::time::Instant::now();
    for _ in 0..n {
        std::hint::black_box(render(&prepared, &s.camera, 960, 540, &opts));
    }
    let draw = t1.elapsed() / n;
    let t2 = std::time::Instant::now();
    for _ in 0..n {
        std::hint::black_box(render_to_rgba(&s, &lib, 960, 540, RenderStyle::Toon).unwrap());
    }
    let full = t2.elapsed() / n;
    let ss = RenderOptions {
        supersample: 2,
        ..opts
    };
    let t3 = std::time::Instant::now();
    for _ in 0..5 {
        std::hint::black_box(render(&prepared, &s.camera, 960, 540, &ss));
    }
    let snap = t3.elapsed() / 5;
    println!(
        "scene: {} triangles, {} threads\nprepare: {prep:?}\nrender 960x540: {draw:?}\nprepare+render: {full:?}\nrender 960x540 2x supersampled: {snap:?}",
        prepared.triangle_count(),
        rayon::current_num_threads()
    );
}

/// Renders a gallery of text-to-shot results for eyeballing:
/// `EMULSION_SCENE_DUMP=/tmp/x cargo test -p emulsion-scene -- --ignored gallery`.
#[test]
#[ignore]
fn gallery() {
    let prompts = [
        "low-angle close-up of two people at a table",
        "wide shot of a woman running down a street",
        "over-the-shoulder of Mia talking to Tom",
        "bird's eye view of a car",
        "extreme close up of a hand",
        "high angle wide shot of a family of three in a park",
        "medium shot of a scared child kneeling",
        "dutch angle cowboy shot of two men fighting",
    ];
    for (i, p) in prompts.iter().enumerate() {
        let shot = text_to_shot(p, 16.0 / 9.0).unwrap();
        let img = render_to_rgba(
            &shot.scene,
            &AssetLibrary::new(),
            480,
            270,
            RenderStyle::Toon,
        )
        .unwrap();
        println!("{i}: {p} -> {}", shot.interpretation);
        dump(&format!("gallery{i}"), &img);
    }
    let s = demo_scene();
    for style in RenderStyle::ALL {
        let img = render(
            &prepare(&s, &AssetLibrary::new()).unwrap(),
            &s.camera,
            480,
            270,
            &RenderOptions {
                supersample: 2,
                ..RenderOptions::style(style)
            },
        );
        dump(&format!("style_{style:?}"), &img);
    }
}
