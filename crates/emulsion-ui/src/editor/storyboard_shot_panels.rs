//! The Shot Generator's side panels: Add (characters, props, lights,
//! imported models) and the object list on the left; the inspector
//! (transform, body sliders, poses, hand shapes, faces, look-at, joints,
//! colour), the camera (lens, shot size, angle, roll, height), the set's
//! light and ground, and the render style on the right.
use super::storyboard_shot_generator::{ShotGenerator, ShotView};
use super::storyboard_shot_viewport::SliderApply;
use super::*;
use crate::file_prompt::FilePrompts;
use crate::widgets::tip;
use emulsion_core::storyboard_shot::{LENSES, label_of as title};
use emulsion_scene as s3;
use emulsion_scene::mannequin::ranges;
use glam::Vec3;
use gpui_kit::component::{
    Sizable,
    button::{Button, ButtonVariants},
};

/// Object colours offered in the inspector.
const COLORS: [[u8; 3]; 8] = [
    [214, 219, 228],
    [200, 196, 188],
    [229, 72, 77],
    [247, 107, 21],
    [255, 197, 61],
    [48, 164, 108],
    [0, 144, 255],
    [60, 60, 66],
];

/// The body slider `i` of the inspector's list.
fn body_slider(b: &mut s3::MannequinParams, i: usize) -> &mut f32 {
    match i {
        0 => &mut b.build,
        1 => &mut b.head_size,
        2 => &mut b.leg_length,
        3 => &mut b.arm_length,
        4 => &mut b.shoulder_width,
        _ => &mut b.hip_width,
    }
}

fn section(name: &str, p: &Palette) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .pb(px(6.))
        .border_b_1()
        .border_color(p.line)
        .child(label(name, p))
}

fn wrap() -> Div {
    div().flex().flex_wrap().gap(px(4.))
}

impl ShotGenerator {
    /// A labelled slider; dragging it is one Undo step labelled `undo`.
    #[allow(clippy::too_many_arguments)]
    fn slider_row(
        &self,
        id: impl Into<ElementId>,
        name: &str,
        value: f32,
        range: (f32, f32),
        shown: String,
        undo: &'static str,
        apply: impl Fn(&mut ShotGenerator, f32) + 'static,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let track: TrackBounds = Rc::default();
        let apply: SliderApply = Rc::new(apply);
        let me = cx.entity().downgrade();
        let t = track.clone();
        let fraction = (value - range.0) / (range.1 - range.0).max(1e-6);
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(
                div()
                    .w(px(68.))
                    .flex_none()
                    .text_size(px(11.))
                    .child(name.to_string()),
            )
            .child(div().flex_1().min_w_0().child(slider(
                id,
                fraction,
                track,
                p,
                move |e, _, cx| {
                    let (t, apply) = (t.clone(), apply.clone());
                    me.update(cx, |this, cx| {
                        this.start_slider(t, range, e.position.x, undo, apply, cx)
                    })
                    .ok();
                },
            )))
            .child(
                div()
                    .w(px(46.))
                    .flex_none()
                    .child(mono(shown, 10., p.muted)),
            )
    }

    /// A one-click change of the working set, committed as one Undo step.
    fn change(
        &mut self,
        undo: &'static str,
        cx: &mut Context<Self>,
        edit: impl FnOnce(&mut s3::Scene, Option<s3::ObjectId>),
    ) {
        let selected = self.selected;
        edit(&mut self.shot.set, selected);
        self.touched(false, cx);
        self.commit(undo, cx);
    }

    // ── Left: Add and the object list ──

    pub(super) fn left_panel(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let mut people = wrap();
        for (i, kind) in s3::MannequinKind::ALL.into_iter().enumerate() {
            people = people.child(
                chip(("shot-add-character", i), kind.label(), false, p)
                    .test_support()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.add(
                            kind.label(),
                            s3::ObjectKind::Character(s3::Character::of(kind)),
                            cx,
                        )
                    })),
            );
        }
        let mut props = wrap();
        for (i, kind) in s3::PropKind::ALL.into_iter().enumerate() {
            props = props.child(
                chip(("shot-add-prop", i), title(kind.name()), false, p)
                    .test_support()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.add(
                            &title(kind.name()),
                            s3::ObjectKind::Prop(s3::Prop::builtin(kind)),
                            cx,
                        )
                    })),
            );
        }
        let mut lights = wrap();
        for (i, (kind, name)) in [
            (s3::LightKind::Key, "Key light"),
            (s3::LightKind::Fill, "Fill light"),
            (s3::LightKind::Rim, "Rim light"),
        ]
        .into_iter()
        .enumerate()
        {
            lights = lights.child(
                chip(("shot-add-light", i), name, false, p)
                    .test_support()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let light = s3::Light {
                            kind,
                            intensity: if kind == s3::LightKind::Key { 0.9 } else { 0.4 },
                        };
                        this.add(name, s3::ObjectKind::Light(light), cx)
                    })),
            );
        }
        let mut list = div()
            .id("shot-objects")
            .test_support()
            .flex()
            .flex_col()
            .gap(px(1.))
            .flex_1()
            .min_h(px(80.))
            .overflow_y_scroll();
        for (i, o) in self.shot.set.objects.iter().enumerate() {
            let id = o.id;
            let kind = match &o.kind {
                s3::ObjectKind::Character(_) => "person",
                s3::ObjectKind::Prop(s3::Prop::Model(_)) => "model",
                s3::ObjectKind::Prop(_) => "prop",
                s3::ObjectKind::Light(_) => "light",
            };
            let on = self.selected == Some(id);
            list = list.child(
                div()
                    .id(("shot-object", i))
                    .test_support()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .px(px(6.))
                    .py(px(3.))
                    .cursor_pointer()
                    .when(on, |d| d.bg(p.accent).text_color(p.accent_fg))
                    .when(!on, |d| d.hover(|s| s.bg(p.soft_bg)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected = Some(id);
                        this.joint = None;
                        this.model_joint = None;
                        this.request_frame(false, cx);
                        cx.notify();
                    }))
                    .child(div().flex_1().truncate().child(if o.visible {
                        o.name.clone()
                    } else {
                        format!("{} (hidden)", o.name)
                    }))
                    .child(mono(kind, 9.5, if on { p.accent_fg } else { p.muted })),
            );
        }
        div()
            .id("shot-left")
            .flex()
            .flex_col()
            .gap(px(8.))
            .w(px(220.))
            .flex_none()
            .overflow_y_scroll()
            .child(
                section("Add", p)
                    .child(mono("Characters", 10., p.muted))
                    .child(people)
                    .child(mono("Props", 10., p.muted))
                    .child(props)
                    .child(mono("Lights", 10., p.muted))
                    .child(lights)
                    .child(
                        Button::new("shot-import-model")
                            .label("Import model…")
                            .tooltip("Add a glTF (.glb, .gltf) or OBJ model; it is stored in the project")
                            .small()
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| this.import_model(cx))),
                    ),
            )
            .child(label("Objects", p))
            .child(list)
            .into_any_element()
    }

    /// Add an object in front of the shot camera, facing it.
    pub(crate) fn add(&mut self, name: &str, kind: s3::ObjectKind, cx: &mut Context<Self>) {
        let camera = self.shot.set.camera;
        let f = camera.forward();
        let mut at = if f.y < -0.02 {
            camera.position + f * (camera.position.y / -f.y).min(8.)
        } else {
            camera.position + Vec3::new(f.x, 0., f.z).normalize_or_zero() * 4.
        };
        at.y = 0.;
        let crowd = self.shot.set.objects.len() as f32;
        at += camera.right() * ((crowd % 5.) - 2.) * 0.15;
        let is_light = matches!(kind, s3::ObjectKind::Light(_));
        let set = &mut self.shot.set;
        let id = set.add(name, kind);
        if is_light {
            set.set_transform(
                id,
                s3::Transform {
                    position: at + Vec3::new(-2., 4., 2.),
                    rotation: s3::Rotation::euler(150., -45., 0.),
                    scale: Vec3::ONE,
                },
            );
        } else {
            let to_camera = camera.position - at;
            let yaw = to_camera.x.atan2(to_camera.z).to_degrees();
            set.set_transform(id, s3::Transform::at_yaw(at, yaw));
        }
        self.selected = Some(id);
        self.joint = None;
        self.touched(false, cx);
        self.commit("Add to set", cx);
    }

    fn import_model(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_open_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import a glTF (.glb, .gltf) or OBJ model".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let read = cx
                .background_spawn(async move {
                    let size = std::fs::metadata(&path).map_err(|e| e.to_string())?.len();
                    if size as usize > s3::limits::MAX_ASSET_BYTES {
                        return Err("Models are at most 64 MB.".to_string());
                    }
                    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
                    let name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("model")
                        .to_string();
                    Ok((name, bytes))
                })
                .await;
            this.update(cx, |this, cx| match read {
                Ok((name, bytes)) => this.import_model_bytes(&name, bytes, cx),
                Err(error) => {
                    this.status = Some((error, true));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Keep a model file in the project and place it in the set (C7).
    pub(crate) fn import_model_bytes(
        &mut self,
        name: &str,
        bytes: Vec<u8>,
        cx: &mut Context<Self>,
    ) {
        self.commit("Shot Generator set", cx);
        let mut placed = None;
        let added = self.edit_project("Import model", cx, |shot, library| {
            let asset = library.add_model(name, bytes)?;
            let model_name = library.models[&asset].name.clone();
            placed = Some(shot.set.add_prop(
                &model_name,
                s3::Prop::Model(s3::ModelRef {
                    asset,
                    joint_rotations: Default::default(),
                }),
                Vec3::ZERO,
                0.,
            ));
            Ok(())
        });
        if added {
            self.selected = placed;
            self.status = Some((format!("Imported {name}."), false));
        }
        cx.notify();
    }

    // ── Right: inspector, camera, set, render ──

    pub(super) fn right_panel(&mut self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let inspector = self.inspector(p, cx);
        div()
            .id("shot-right")
            .test_support()
            .flex()
            .flex_col()
            .gap(px(8.))
            .w(px(300.))
            .flex_none()
            .overflow_y_scroll()
            .child(inspector)
            .child(self.camera_section(p, cx))
            .child(self.set_section(p, cx))
            .child(self.render_section(p, cx))
            .into_any_element()
    }

    fn inspector(&mut self, p: &Palette, cx: &mut Context<Self>) -> Div {
        let Some(o) = self.object().cloned() else {
            return section("Selection", p).child(mono(
                "Click an object in the viewport or the list.",
                10.,
                p.muted,
            ));
        };
        let id = o.id;
        let t = o.transform;
        let (yaw, pitch, roll) = t.rotation.to_yaw_pitch_roll();
        let mut root = section("Selection", p)
            .child(div().text_size(px(13.)).child(o.name.clone()))
            .child(
                wrap()
                    .child(
                        Button::new("shot-duplicate")
                            .label("Duplicate")
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let Some(copy) = this.shot.set.object(id).cloned() else {
                                    return;
                                };
                                let new =
                                    this.shot.set.add(&format!("{} copy", copy.name), copy.kind);
                                let mut tr = copy.transform;
                                tr.position += Vec3::new(0.6, 0., 0.);
                                this.shot.set.set_transform(new, tr);
                                this.shot.set.object_mut(new).unwrap().color = copy.color;
                                this.selected = Some(new);
                                this.touched(false, cx);
                                this.commit("Duplicate object", cx);
                            })),
                    )
                    .child(
                        Button::new("shot-hide")
                            .label(if o.visible { "Hide" } else { "Show" })
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.change("Show or hide object", cx, |set, _| {
                                    if let Some(o) = set.object_mut(id) {
                                        o.visible = !o.visible;
                                    }
                                })
                            })),
                    )
                    .child(
                        Button::new("shot-casts-shadow")
                            .label(if o.casts_shadows {
                                "Casts shadow"
                            } else {
                                "No shadow"
                            })
                            .tooltip(
                                "Whether it casts a shadow (on a key light: whether that light casts shadows)",
                            )
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.change("Casts shadow", cx, |set, _| {
                                    if let Some(o) = set.object_mut(id) {
                                        o.casts_shadows = !o.casts_shadows;
                                    }
                                })
                            })),
                    )
                    .child(
                        Button::new("shot-delete")
                            .label("Delete")
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.selected = None;
                                this.change("Delete object", cx, |set, _| {
                                    set.remove(id);
                                })
                            })),
                    ),
            );
        let span = if matches!(o.kind, s3::ObjectKind::Light(_)) {
            20.
        } else {
            10.
        };
        root = root
            .child(self.slider_row(
                "shot-x",
                "Left/right",
                t.position.x,
                (-span, span),
                format!("{:.2} m", t.position.x),
                "Move object",
                move |g, v| {
                    let mut at = g.object().map_or(Vec3::ZERO, |o| o.transform.position);
                    at.x = v;
                    g.shot.set.set_position(id, at);
                },
                p,
                cx,
            ))
            .child(self.slider_row(
                "shot-z",
                "Near/far",
                t.position.z,
                (-span, span),
                format!("{:.2} m", t.position.z),
                "Move object",
                move |g, v| {
                    let mut at = g.object().map_or(Vec3::ZERO, |o| o.transform.position);
                    at.z = v;
                    g.shot.set.set_position(id, at);
                },
                p,
                cx,
            ))
            .child(self.slider_row(
                "shot-y",
                "Height",
                t.position.y,
                (0., span / 2.),
                format!("{:.2} m", t.position.y),
                "Move object",
                move |g, v| {
                    let mut at = g.object().map_or(Vec3::ZERO, |o| o.transform.position);
                    at.y = v;
                    g.shot.set.set_position(id, at);
                },
                p,
                cx,
            ))
            .child(self.slider_row(
                "shot-yaw",
                "Turn",
                yaw,
                (-180., 180.),
                format!("{yaw:.0}°"),
                "Rotate object",
                move |g, v| {
                    g.shot.set.set_rotation_euler(id, v, pitch, roll);
                },
                p,
                cx,
            ))
            .child(self.slider_row(
                "shot-scale",
                "Scale",
                t.scale.x,
                (0.1, 4.),
                format!("{:.2}×", t.scale.x),
                "Scale object",
                move |g, v| {
                    g.shot.set.set_scale(id, Vec3::splat(v));
                },
                p,
                cx,
            ));
        if !matches!(o.kind, s3::ObjectKind::Light(_)) {
            let mut swatches = wrap();
            for (i, c) in COLORS.into_iter().enumerate() {
                let on = o.color.0 == c;
                swatches = swatches.child(
                    div()
                        .id(("shot-color", i))
                        .test_support()
                        .size(px(18.))
                        .cursor_pointer()
                        .border_1()
                        .border_color(if on { p.accent } else { p.line })
                        .bg(rgb(u32::from(c[0]) << 16
                            | u32::from(c[1]) << 8
                            | u32::from(c[2])))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.change("Object colour", cx, |set, _| {
                                if let Some(o) = set.object_mut(id) {
                                    o.color = s3::Rgb(c);
                                }
                            })
                        })),
                );
            }
            root = root.child(swatches);
        }
        match &o.kind {
            s3::ObjectKind::Character(c) => root.child(self.character_panel(id, c, p, cx)),
            s3::ObjectKind::Prop(s3::Prop::Builtin(b)) => {
                let size = b.size();
                let mut root = root;
                for (axis, name, v) in [
                    (0usize, "Width", size.x),
                    (1, "Prop height", size.y),
                    (2, "Depth", size.z),
                ] {
                    root = root.child(self.slider_row(
                        ("shot-size", axis),
                        name,
                        v,
                        (0.05, 20.),
                        format!("{v:.2} m"),
                        "Prop size",
                        move |g, value| {
                            if let Some(s3::ObjectKind::Prop(s3::Prop::Builtin(b))) =
                                g.shot.set.object_mut(id).map(|o| &mut o.kind)
                            {
                                let mut s = b.size();
                                s[axis] = value;
                                b.size = Some(s);
                            }
                        },
                        p,
                        cx,
                    ));
                }
                root
            }
            s3::ObjectKind::Prop(s3::Prop::Model(m)) => root.child(self.model_panel(id, m, p, cx)),
            s3::ObjectKind::Light(l) => {
                let mut kinds = wrap();
                for (i, (kind, name)) in [
                    (s3::LightKind::Key, "Key"),
                    (s3::LightKind::Fill, "Fill"),
                    (s3::LightKind::Rim, "Rim"),
                ]
                .into_iter()
                .enumerate()
                {
                    kinds = kinds.child(
                        chip(("shot-light-kind", i), name, l.kind == kind, p).on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.change("Light kind", cx, |set, _| {
                                    if let Some(s3::ObjectKind::Light(l)) =
                                        set.object_mut(id).map(|o| &mut o.kind)
                                    {
                                        l.kind = kind;
                                    }
                                })
                            }),
                        ),
                    );
                }
                root.child(kinds).child(self.slider_row(
                    "shot-intensity",
                    "Intensity",
                    l.intensity,
                    (0., 4.),
                    format!("{:.2}", l.intensity),
                    "Light intensity",
                    move |g, v| {
                        if let Some(s3::ObjectKind::Light(l)) =
                            g.shot.set.object_mut(id).map(|o| &mut o.kind)
                        {
                            l.intensity = v;
                        }
                    },
                    p,
                    cx,
                ))
            }
        }
    }

    fn character_panel(
        &mut self,
        id: s3::ObjectId,
        c: &s3::Character,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let body = c.body;
        let mut root = div().flex().flex_col().gap(px(4.));
        let mut kinds = wrap();
        for (i, kind) in s3::MannequinKind::ALL.into_iter().enumerate() {
            kinds = kinds.child(
                chip(("shot-body-kind", i), kind.label(), body.kind == kind, p).on_click(
                    cx.listener(move |this, _, _, cx| {
                        this.change("Body type", cx, |set, _| {
                            if let Some(c) = set.character_mut(id) {
                                c.body = s3::MannequinParams {
                                    kind,
                                    height: None,
                                    ..c.body
                                };
                            }
                        })
                    }),
                ),
            );
        }
        root = root.child(mono("Body", 10., p.muted)).child(kinds);
        let sliders: [(&str, f32, (f32, f32)); 6] = [
            ("Build", body.build, ranges::BUILD),
            ("Head", body.head_size, ranges::HEAD_SIZE),
            ("Legs", body.leg_length, ranges::LIMB_LENGTH),
            ("Arms", body.arm_length, ranges::LIMB_LENGTH),
            ("Shoulders", body.shoulder_width, ranges::WIDTH),
            ("Hips", body.hip_width, ranges::WIDTH),
        ];
        let height = body.height_m();
        root = root.child(self.slider_row(
            "shot-body-height",
            "Body height",
            height,
            ranges::HEIGHT,
            format!("{height:.2} m"),
            "Body sliders",
            move |g, v| {
                if let Some(c) = g.shot.set.character_mut(id) {
                    c.body.height = Some(v);
                }
            },
            p,
            cx,
        ));
        for (i, (name, value, range)) in sliders.into_iter().enumerate() {
            root = root.child(self.slider_row(
                ("shot-body", i),
                name,
                value,
                range,
                format!("{value:.2}"),
                "Body sliders",
                move |g, v| {
                    if let Some(c) = g.shot.set.character_mut(id) {
                        *body_slider(&mut c.body, i) = v;
                    }
                },
                p,
                cx,
            ));
        }
        // Poses: presets and the project's custom poses.
        let mut poses = wrap();
        for (i, preset) in s3::PosePreset::ALL.into_iter().enumerate() {
            poses = poses.child(
                chip(
                    ("shot-pose", i),
                    title(preset.name()),
                    c.pose.name == preset.name(),
                    p,
                )
                .test_support()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.change("Pose", cx, |set, _| {
                        if let Some(c) = set.character_mut(id) {
                            c.pose = preset.pose();
                            c.ik.clear();
                        }
                    })
                })),
            );
        }
        for (i, pose) in self.library.poses.iter().enumerate() {
            let pose = pose.clone();
            poses = poses.child(
                chip(
                    ("shot-custom-pose", i),
                    pose.name.clone(),
                    c.pose.name == pose.name,
                    p,
                )
                .test_support()
                .on_click(cx.listener(move |this, _, _, cx| {
                    let pose = pose.clone();
                    this.change("Pose", cx, |set, _| {
                        if let Some(c) = set.character_mut(id) {
                            c.pose = pose;
                            c.ik.clear();
                        }
                    })
                })),
            );
        }
        root = root.child(mono("Pose", 10., p.muted)).child(poses).child(
            wrap()
                .child(
                    Button::new("shot-mirror")
                        .label("Mirror")
                        .xsmall()
                        .ghost()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.change("Mirror pose", cx, |set, _| {
                                if let Some(c) = set.character_mut(id) {
                                    c.pose = c.pose.mirrored();
                                }
                            })
                        })),
                )
                .child(
                    Button::new("shot-bake")
                        .label("Bake IK")
                        .tooltip("Turn IK targets and look-at into joint angles")
                        .xsmall()
                        .ghost()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.change("Bake pose", cx, |set, _| {
                                set.bake_pose(id);
                            })
                        })),
                )
                .child(
                    Button::new("shot-clear-ik")
                        .label("Clear IK")
                        .xsmall()
                        .ghost()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.change("Clear IK", cx, |set, _| {
                                if let Some(c) = set.character_mut(id) {
                                    c.ik.clear();
                                }
                            })
                        })),
                ),
        );
        if let Some(input) = self.pose_name.clone() {
            root = root.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(div().flex_1().child(Input::new(&input).xsmall()))
                    .child(
                        Button::new("shot-save-pose")
                            .label("Save pose")
                            .tooltip("Keep this pose in the project's pose library")
                            .xsmall()
                            .outline()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let name = input.read(cx).value().to_string();
                                let Some(pose) =
                                    this.shot.set.character(id).map(|c| c.pose.clone())
                                else {
                                    return;
                                };
                                this.commit("Shot Generator set", cx);
                                if this.edit_project("Save pose", cx, |_, library| {
                                    library.save_pose(&name, &pose)
                                }) {
                                    this.status =
                                        Some((format!("Saved pose “{}”.", name.trim()), false));
                                }
                            })),
                    ),
            );
        }
        // Hands and face.
        for (side, left) in [("Left hand", true), ("Right hand", false)] {
            let current = if left {
                c.pose.left_hand
            } else {
                c.pose.right_hand
            };
            let mut hands = wrap();
            for (i, shape) in s3::HandShape::ALL.into_iter().enumerate() {
                hands = hands.child(
                    chip(
                        ("shot-hand", usize::from(left) * 10 + i),
                        title(shape.name()),
                        current == shape,
                        p,
                    )
                    .test_support()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.change("Hand shape", cx, |set, _| {
                            if let Some(c) = set.character_mut(id) {
                                if left {
                                    c.pose.left_hand = shape;
                                } else {
                                    c.pose.right_hand = shape;
                                }
                            }
                        })
                    })),
                );
            }
            root = root.child(mono(side, 10., p.muted)).child(hands);
        }
        let mut faces = wrap();
        for (i, face) in s3::FacePreset::ALL.into_iter().enumerate() {
            faces = faces.child(
                chip(("shot-face", i), title(face.name()), c.face == face, p)
                    .test_support()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.change("Face", cx, |set, _| {
                            if let Some(c) = set.character_mut(id) {
                                c.face = face;
                            }
                        })
                    })),
            );
        }
        root = root.child(mono("Face", 10., p.muted)).child(faces).child(
            wrap()
                .child(mono("Look at", 10., p.muted))
                .child(
                    chip("shot-look-pick", "Pick point", self.picking_look, p)
                        .test_support()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.picking_look = !this.picking_look;
                            cx.notify();
                        })),
                )
                .child(
                    chip("shot-look-camera", "Camera", false, p).on_click(cx.listener(
                        move |this, _, _, cx| {
                            let at = this.shot.set.camera.position;
                            this.change("Look at", cx, |set, _| {
                                if let Some(c) = set.character_mut(id) {
                                    c.look_at = Some(at);
                                }
                            })
                        },
                    )),
                )
                .child(
                    chip("shot-look-clear", "Ahead", c.look_at.is_none(), p).on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.change("Look ahead", cx, |set, _| {
                                if let Some(c) = set.character_mut(id) {
                                    c.look_at = None;
                                }
                            })
                        },
                    )),
                ),
        );
        // The joint picked in the Pose tool.
        if let Some(bone) = self.joint {
            let r = c.pose.rotation(bone);
            root = root.child(mono(format!("Joint · {}", bone.name()), 10., p.muted));
            for (axis, name, value) in [
                (0usize, "Bend (x)", r.x),
                (1, "Twist (y)", r.y),
                (2, "Swing (z)", r.z),
            ] {
                root = root.child(self.slider_row(
                    ("shot-joint", axis),
                    name,
                    value,
                    (-180., 180.),
                    format!("{value:.0}°"),
                    "Pose joint",
                    move |g, v| {
                        let Some(c) = g.shot.set.character(id) else {
                            return;
                        };
                        let mut r = c.pose.rotation(bone);
                        match axis {
                            0 => r.x = v,
                            1 => r.y = v,
                            _ => r.z = v,
                        }
                        g.shot.set.set_joint(id, bone, r);
                    },
                    p,
                    cx,
                ));
            }
        }
        root
    }

    /// An imported model: pose its joints by name (C10).
    fn model_panel(
        &mut self,
        id: s3::ObjectId,
        m: &s3::ModelRef,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let assets = self.assets();
        let joints = assets
            .get(&m.asset)
            .map(|a| a.joint_names())
            .unwrap_or_default();
        let mut root = div().flex().flex_col().gap(px(4.));
        if assets.get(&m.asset).is_none() {
            return root.child(mono(
                "This model is missing; it shows as a box.",
                10.,
                p.muted,
            ));
        }
        if joints.is_empty() {
            return root.child(mono("This model has no rig to pose.", 10., p.muted));
        }
        let mut list = wrap();
        for (i, joint) in joints.iter().take(120).enumerate() {
            let name = joint.clone();
            list = list.child(
                chip(
                    ("shot-model-joint", i),
                    joint.clone(),
                    self.model_joint.as_deref() == Some(joint),
                    p,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.model_joint = Some(name.clone());
                    cx.notify();
                })),
            );
        }
        root = root.child(mono("Joints", 10., p.muted)).child(list);
        if let Some(joint) = self.model_joint.clone() {
            let r = m
                .joint_rotations
                .get(&joint)
                .copied()
                .unwrap_or(s3::JointRotation::ZERO);
            for (axis, name, value) in [(0usize, "X", r.x), (1, "Y", r.y), (2, "Z", r.z)] {
                let joint = joint.clone();
                root = root.child(self.slider_row(
                    ("shot-model-axis", axis),
                    name,
                    value,
                    (-180., 180.),
                    format!("{value:.0}°"),
                    "Pose joint",
                    move |g, v| {
                        if let Some(s3::ObjectKind::Prop(s3::Prop::Model(m))) =
                            g.shot.set.object_mut(id).map(|o| &mut o.kind)
                        {
                            let r = m
                                .joint_rotations
                                .entry(joint.clone())
                                .or_insert(s3::JointRotation::ZERO);
                            match axis {
                                0 => r.x = v,
                                1 => r.y = v,
                                _ => r.z = v,
                            }
                        }
                    },
                    p,
                    cx,
                ));
            }
        }
        root
    }

    fn camera_section(&mut self, p: &Palette, cx: &mut Context<Self>) -> Div {
        let camera = self.shot.set.camera;
        let mut lenses = wrap();
        for (i, f) in LENSES.into_iter().enumerate() {
            lenses = lenses.child(
                chip(
                    ("shot-lens", i),
                    format!("{f:.0}"),
                    (camera.focal_length_mm - f).abs() < 0.5,
                    p,
                )
                .test_support()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.shot.set.camera.focal_length_mm = f;
                    this.view = ShotView::Camera;
                    this.request_frame(false, cx);
                    this.commit("Lens", cx);
                })),
            );
        }
        let (shot_size, shot_angle, shot_side) = self.camera_highlights();
        let mut sizes = wrap();
        for (i, size) in s3::ShotSize::ALL.into_iter().enumerate() {
            sizes = sizes.child(
                tip(
                    chip(
                        ("shot-size-frame", i),
                        size.abbreviation(),
                        shot_size == Some(size),
                        p,
                    ),
                    size.label(),
                )
                .test_support()
                .on_click(cx.listener(move |this, _, _, cx| this.frame_subject(size, cx))),
            );
        }
        let mut angles = wrap();
        for (i, angle) in s3::CameraAngle::ALL.into_iter().enumerate() {
            angles = angles.child(
                chip(
                    ("shot-angle", i),
                    title(angle.label()),
                    shot_angle == Some(angle),
                    p,
                )
                .test_support()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.frame_angle = Some(angle);
                    this.reframe(cx);
                })),
            );
        }
        let mut sides = wrap();
        for (i, side) in s3::ShotSide::ALL.into_iter().enumerate() {
            sides = sides.child(
                chip(
                    ("shot-side", i),
                    title(side.label()),
                    shot_side == Some(side),
                    p,
                )
                .test_support()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.frame_side = Some(side);
                    this.reframe(cx);
                })),
            );
        }
        let mut root = section("Camera", p)
            .child(mono(
                format!(
                    "{:.0} mm · {:.0}° across",
                    camera.focal_length_mm,
                    camera.horizontal_fov_deg()
                ),
                10.,
                p.muted,
            ))
            .child(lenses)
            .child(mono("Frame the selection", 10., p.muted))
            .child(sizes)
            .child(mono("Angle", 10., p.muted))
            .child(angles)
            .child(mono("Side", 10., p.muted))
            .child(sides)
            .child(self.slider_row(
                "shot-roll",
                "Roll",
                camera.roll,
                (-45., 45.),
                format!("{:.0}°", camera.roll),
                "Camera roll",
                |g, v| {
                    g.shot.set.camera.roll = v;
                },
                p,
                cx,
            ))
            .child(self.slider_row(
                "shot-tilt",
                "Tilt",
                camera.pitch,
                (-89., 89.),
                format!("{:.0}°", camera.pitch),
                "Camera tilt",
                |g, v| {
                    g.shot.set.camera.pitch = v;
                },
                p,
                cx,
            ))
            .child(self.slider_row(
                "shot-cam-height",
                "Height",
                camera.position.y,
                (0.05, 30.),
                format!("{:.2} m", camera.position.y),
                "Camera height",
                |g, v| {
                    g.shot.set.camera.position.y = v;
                },
                p,
                cx,
            ));
        if self.view == ShotView::Free {
            root = root.child(
                Button::new("shot-view-to-camera")
                    .label("Use this view as the shot")
                    .xsmall()
                    .outline()
                    .on_click(cx.listener(|this, _, _, cx| {
                        let camera = this.shot.set.camera;
                        let free = this.free.camera(camera.film_back);
                        this.shot.set.camera = s3::Camera {
                            position: free.position,
                            yaw: free.yaw,
                            pitch: free.pitch,
                            ..camera
                        };
                        this.view = ShotView::Camera;
                        this.commit("Move camera", cx);
                        this.request_frame(false, cx);
                    })),
            );
        }
        root
    }

    /// The shot size, angle and side chips to highlight: the camera's
    /// framing while it has not moved since, else the angle and side picked
    /// for the next framing.
    pub(crate) fn camera_highlights(
        &self,
    ) -> (
        Option<s3::ShotSize>,
        Option<s3::CameraAngle>,
        Option<s3::ShotSide>,
    ) {
        match self.shot.set.current_shot() {
            Some(spec) => (Some(spec.size), Some(spec.angle), Some(spec.side)),
            None => (None, self.frame_angle, self.frame_side),
        }
    }

    /// After an angle or side pick: reframe a framed camera at its size.
    fn reframe(&mut self, cx: &mut Context<Self>) {
        match self.shot.set.current_shot().map(|s| s.size) {
            Some(size) => self.frame_subject(size, cx),
            None => cx.notify(),
        }
    }

    /// Frame the subject at `size` with the chosen angle and side (the
    /// current shot's when none was picked).
    pub(crate) fn frame_subject(&mut self, size: s3::ShotSize, cx: &mut Context<Self>) {
        let Some(subject) = self.subject() else {
            self.status = Some(("Add a character or prop to frame.".into(), true));
            cx.notify();
            return;
        };
        let current = self.shot.set.current_shot().copied();
        let mut spec = s3::ShotSpec::new(subject, size);
        spec.angle = self
            .frame_angle
            .or(current.map(|s| s.angle))
            .unwrap_or_default();
        spec.side = self
            .frame_side
            .or(current.map(|s| s.side))
            .unwrap_or_default();
        if spec.angle.needs_secondary() {
            spec.secondary = self
                .shot
                .set
                .character_ids()
                .into_iter()
                .find(|id| *id != subject);
        }
        let assets = self.assets();
        match s3::frame_shot(&self.shot.set, &assets, &spec, self.aspect) {
            Ok(camera) => {
                self.shot.set.apply_shot(spec, camera);
                self.frame_angle = None;
                self.frame_side = None;
                self.view = ShotView::Camera;
                self.status = Some((spec.name(), false));
                self.commit("Frame shot", cx);
                self.request_frame(false, cx);
            }
            Err(error) => self.status = Some((error.to_string(), true)),
        }
        cx.notify();
    }

    fn set_section(&mut self, p: &Palette, cx: &mut Context<Self>) -> Div {
        let env = self.shot.set.environment;
        let toggle =
            |id: &'static str, name: &'static str, on: bool, flip: fn(&mut s3::Environment)| {
                chip(id, name, on, p).on_click(cx.listener(move |this, _, _, cx| {
                    this.change("Set ground and sky", cx, |set, _| {
                        flip(&mut set.environment)
                    })
                }))
            };
        section("Light and ground", p)
            .child(
                wrap()
                    .child(toggle("shot-ground", "Ground", env.show_ground, |e| {
                        e.show_ground = !e.show_ground
                    }))
                    .child(toggle("shot-grid", "Grid", env.show_grid, |e| {
                        e.show_grid = !e.show_grid
                    }))
                    .child(toggle("shot-horizon", "Horizon", env.show_horizon, |e| {
                        e.show_horizon = !e.show_horizon
                    })),
            )
            .child(self.slider_row(
                "shot-ambient",
                "Ambient",
                env.ambient,
                (0., 1.),
                format!("{:.2}", env.ambient),
                "Ambient light",
                |g, v| {
                    g.shot.set.environment.ambient = v;
                },
                p,
                cx,
            ))
    }

    fn render_section(&mut self, p: &Palette, cx: &mut Context<Self>) -> Div {
        let reference = self.shot.reference;
        let mut styles = wrap();
        for (i, style) in s3::RenderStyle::ALL.into_iter().enumerate() {
            let name = match style {
                s3::RenderStyle::Toon => "Toon",
                s3::RenderStyle::Clay => "Clay",
                s3::RenderStyle::Outline => "Outline",
                s3::RenderStyle::Silhouette => "Silhouette",
            };
            styles = styles.child(
                chip(("shot-style", i), name, reference.style == style, p)
                    .test_support()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.shot.reference.style = style;
                        this.request_frame(false, cx);
                        this.commit("Reference style", cx);
                    })),
            );
        }
        section("Reference layer", p)
            .child(mono("Style (Outline is best for tracing)", 10., p.muted))
            .child(styles)
            .child(self.slider_row(
                "shot-opacity",
                "Opacity",
                reference.opacity,
                (0.05, 1.),
                format!("{:.0}%", reference.opacity * 100.),
                "Reference opacity",
                |g, v| g.shot.reference.opacity = v,
                p,
                cx,
            ))
            .child(
                chip(
                    "shot-auto-update",
                    "Update with the set",
                    reference.auto_update,
                    p,
                )
                .test_support()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.shot.reference.auto_update = !this.shot.reference.auto_update;
                    this.commit("Reference updates", cx);
                })),
            )
            .child(
                tip(
                    chip("shot-shadows", "Shadows", reference.shadows, p),
                    "Key lights cast shadows in the Toon and Clay styles",
                )
                .test_support()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.shot.reference.shadows = !this.shot.reference.shadows;
                    this.request_frame(false, cx);
                    this.commit("Shadows", cx);
                })),
            )
            .child(mono("Panel layers", 10., p.muted))
            .child(
                tip(
                    chip(
                        "shot-lay-on-surface",
                        "Lay selected layer on a surface",
                        self.picking_surface.is_some(),
                        p,
                    ),
                    "Then click a surface: the selected pixel layer is warped to its angle and stays on it",
                )
                .test_support()
                .on_click(cx.listener(|this, _, _, cx| {
                    if this.picking_surface.take().is_none() {
                        let layer = this
                            .editor
                            .upgrade()
                            .and_then(|e| e.read(cx).selected);
                        match layer {
                            Some(layer) => {
                                this.picking_surface = Some(layer);
                                this.view = ShotView::Camera;
                            }
                            None => {
                                this.status =
                                    Some(("Select a pixel layer of the panel first.".into(), true))
                            }
                        }
                    }
                    cx.notify();
                })),
            )
    }
}
