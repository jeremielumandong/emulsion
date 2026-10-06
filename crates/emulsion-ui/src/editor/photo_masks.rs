//! Persistent mask parameters. Pointer previews form one transaction; exact
//! values commit on Enter, while blur, Escape and target changes discard drafts.
use super::*;
use emulsion_core::{MAX_MASK_FEATHER, MaskProperties};
use gpui_kit::component::{
    Disableable, Sizable,
    button::{Button, ButtonVariants},
    input::Escape,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum MaskProperty {
    Density,
    Feather,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum MaskControlSurface {
    Properties,
    Taskbar,
}

impl MaskProperty {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Density => "Density",
            Self::Feather => "Feather",
        }
    }

    fn spec(self) -> (f32, f32, f32) {
        match self {
            Self::Density => (0., 100., 1.),
            Self::Feather => (0., MAX_MASK_FEATHER, 0.1),
        }
    }

    fn value(self, properties: MaskProperties) -> f32 {
        match self {
            Self::Density => properties.density * 100.,
            Self::Feather => properties.feather,
        }
    }

    fn display(self, properties: MaskProperties) -> String {
        match self {
            Self::Density => format!("{:.0}%", self.value(properties)),
            Self::Feather => format!("{:.1} px", self.value(properties)),
        }
    }
}

pub(super) struct MaskNumericEdit {
    key: SliderKey,
    input: Entity<InputState>,
    ticket: (u64, u64),
    _subscription: Subscription,
}

#[derive(Default)]
pub(crate) struct PhotoMaskState {
    pub(super) edit: Option<MaskNumericEdit>,
}

impl EditorView {
    fn mask_properties_target_ready(&self, id: NodeId, target: MaskEditTarget) -> bool {
        self.mask_component_ready(id, target, false)
            && self.mask_properties_component(id) == Some(target)
    }

    pub(super) fn mask_property_slider_ready(&self, key: SliderKey) -> bool {
        match key {
            SliderKey::MaskProperty(id, target, ..) => {
                self.layer_menu_ready() && self.mask_properties_target_ready(id, target)
            }
            _ => true,
        }
    }

    /// Changing selection, target or panel commits an already-visible pointer
    /// preview and discards uncommitted text. The captured NodeId never migrates.
    pub(super) fn finish_mask_properties(&mut self) {
        self.tools.photo_masks.edit = None;
        if matches!(
            self.drag,
            Some(Drag::Slider {
                key: SliderKey::MaskProperty(..),
                ..
            })
        ) {
            self.drag = None;
            if self.editor.in_transaction() {
                self.editor.end();
            }
        }
    }

    pub(super) fn cancel_mask_properties(&mut self, cx: &mut Context<Self>) -> bool {
        let mut had = self.tools.photo_masks.edit.take().is_some();
        if matches!(
            self.drag,
            Some(Drag::Slider {
                key: SliderKey::MaskProperty(..),
                ..
            })
        ) {
            self.drag = None;
            self.invalidate_pending_edits();
            self.editor.cancel();
            self.after_change(cx);
            had = true;
        }
        if had {
            cx.notify();
        }
        had
    }

    pub(super) fn apply_mask_property(
        &mut self,
        id: NodeId,
        target: MaskEditTarget,
        property: MaskProperty,
        value: f32,
        cx: &mut Context<Self>,
    ) {
        if !self.mask_properties_target_ready(id, target) || !value.is_finite() {
            return;
        }
        let Some(mut properties) = self.editor.doc.node(id).and_then(|n| target.properties(n))
        else {
            return;
        };
        let (min, max, _) = property.spec();
        let value = value.clamp(min, max);
        match property {
            MaskProperty::Density => properties.density = value / 100.,
            MaskProperty::Feather => properties.feather = value,
        }
        self.execute(target.properties_command(id, properties), cx);
    }

    fn reset_mask_properties(
        &mut self,
        id: NodeId,
        target: MaskEditTarget,
        cx: &mut Context<Self>,
    ) {
        self.finish_mask_properties();
        if !self.layer_menu_ready() || !self.mask_properties_target_ready(id, target) {
            return;
        }
        self.execute(target.properties_command(id, MaskProperties::default()), cx);
    }

    fn start_mask_numeric(&mut self, key: SliderKey, window: &mut Window, cx: &mut Context<Self>) {
        let SliderKey::MaskProperty(id, target, property, _) = key else {
            return;
        };
        if !self.mask_property_slider_ready(key) {
            return;
        }
        self.reset_photo_numeric();
        self.close_text_field(cx);
        let value = property.value(
            target
                .properties(self.editor.doc.node(id).unwrap())
                .unwrap(),
        );
        let input = cx.new(|cx| InputState::new(window, cx).default_value(format!("{value:.1}")));
        let subscription = cx.subscribe_in(
            &input,
            window,
            |this, input, event: &InputEvent, window, cx| {
                if !this
                    .tools
                    .photo_masks
                    .edit
                    .as_ref()
                    .is_some_and(|edit| edit.input == *input)
                {
                    return;
                }
                match event {
                    InputEvent::PressEnter { .. } => this.commit_mask_numeric(window, cx),
                    InputEvent::Blur => {
                        this.tools.photo_masks.edit = None;
                        cx.notify();
                    }
                    _ => {}
                }
            },
        );
        self.tools.photo_masks.edit = Some(MaskNumericEdit {
            key,
            input: input.clone(),
            ticket: self.edit_ticket(),
            _subscription: subscription,
        });
        input.update(cx, |state, cx| {
            state.focus(window, cx);
            state.select_all(window, cx);
        });
        cx.notify();
    }

    fn commit_mask_numeric(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = self.tools.photo_masks.edit.as_ref() else {
            return;
        };
        let SliderKey::MaskProperty(id, target, property, _) = edit.key else {
            return;
        };
        if !self.mask_property_slider_ready(edit.key) || !self.edit_is_current(edit.ticket) {
            self.tools.photo_masks.edit = None;
            cx.notify();
            return;
        }
        let spec = property.spec();
        let Some(value) = parse_mask_numeric(&edit.input.read(cx).value(), spec) else {
            self.set_status(
                format!("Enter {} from {} to {}.", property.label(), spec.0, spec.1),
                true,
                cx,
            );
            return;
        };
        self.tools.photo_masks.edit = None;
        self.apply_mask_property(id, target, property, value, cx);
        window.focus(&self.canvas_focus, cx);
        cx.notify();
    }

    fn mask_numeric_value(
        &mut self,
        key: SliderKey,
        property: MaskProperty,
        properties: MaskProperties,
        disabled: bool,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Some(edit) = self
            .tools
            .photo_masks
            .edit
            .as_ref()
            .filter(|edit| edit.key == key)
        {
            return div()
                .id(SharedString::from(format!("mask-value-edit-{key:?}")))
                .test_support()
                .w(px(76.))
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.tools.photo_masks.edit = None;
                    cx.notify();
                }))
                .on_action(cx.listener(|this, _: &Escape, window, cx| {
                    this.tools.photo_masks.edit = None;
                    window.focus(&this.canvas_focus, cx);
                    cx.notify();
                }))
                .child(Input::new(&edit.input).small().w_full())
                .into_any_element();
        }
        let component_label = if matches!(
            key,
            SliderKey::MaskProperty(_, MaskEditTarget::SmartFilterMask, ..)
        ) {
            t!("editor.filter_mask.title").into_owned()
        } else {
            "Mask".to_string()
        };
        Button::new(SharedString::from(format!("mask-value-{key:?}")))
            .label(property.display(properties))
            .accessibility_label(format!("{} {} value", component_label, property.label()))
            .tooltip("Enter commits; Escape or clicking away cancels")
            .small()
            .ghost()
            .disabled(disabled)
            .text_color(p.ink)
            .on_click(
                cx.listener(move |this, _, window, cx| this.start_mask_numeric(key, window, cx)),
            )
            .into_any_element()
    }

    pub(super) fn mask_property_controls(
        &mut self,
        id: NodeId,
        surface: MaskControlSurface,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(target) = self.mask_properties_component(id) else {
            return div().into_any_element();
        };
        let Some(properties) = self
            .editor
            .doc
            .node(id)
            .and_then(|node| target.properties(node))
        else {
            return div().into_any_element();
        };
        let disabled = !self.mask_properties_target_ready(id, target);
        let compact = surface == MaskControlSurface::Taskbar;
        let mut controls = div()
            .id(SharedString::from(format!("mask-properties-{surface:?}")))
            .test_support()
            .flex()
            .gap_2()
            .when(!compact, |el| el.flex_col())
            .when(compact, |el| el.items_center().flex_wrap());
        if !compact && target == MaskEditTarget::SmartFilterMask {
            controls = controls.child(mono(t!("editor.filter_mask.title"), 11., p.muted));
        }
        for property in [MaskProperty::Density, MaskProperty::Feather] {
            let key = SliderKey::MaskProperty(id, target, property, surface);
            let spec = property.spec();
            let norm = property.value(properties) / spec.1;
            let track = self.tracks.entry(key).or_default().clone();
            let value = self.mask_numeric_value(key, property, properties, disabled, p, cx);
            let row = div()
                .flex()
                .flex_col()
                .gap_1()
                .when(compact, |el| el.w(px(142.)))
                .when(disabled, |el| el.opacity(0.5))
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .items_center()
                        .child(mono(property.label(), 11., p.muted))
                        .child(value),
                )
                .child(
                    slider(
                        SharedString::from(format!("{key:?}")),
                        norm,
                        track,
                        p,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            if this.mask_property_slider_ready(key) {
                                this.slider_down(key, spec, event, cx);
                            }
                        }),
                    )
                    .test_support()
                    .tab_index(0)
                    .key_context("Slider")
                    .role(Role::Slider)
                    .aria_label(format!(
                        "{} {}",
                        if target == MaskEditTarget::SmartFilterMask {
                            t!("editor.filter_mask.title").into_owned()
                        } else {
                            "Mask".to_string()
                        },
                        property.label()
                    ))
                    .aria_value(property.display(properties))
                    .aria_min_numeric_value(spec.0 as f64)
                    .aria_max_numeric_value(spec.1 as f64)
                    .aria_description(match property {
                        MaskProperty::Density if target == MaskEditTarget::SmartFilterMask => {
                            "0 to 100 percent; 0 reveals the entire filter effect"
                        }
                        MaskProperty::Density => "0 to 100 percent; 0 reveals the whole layer",
                        MaskProperty::Feather => {
                            "0 to 1000 stored mask pixels, before the mask transform"
                        }
                    })
                    .focus_visible(|s| s.bg(p.accent.opacity(0.2)))
                    .on_key_down(cx.listener(
                        move |this, event: &KeyDownEvent, _, cx| {
                            if event.keystroke.key == "escape" && this.cancel_mask_properties(cx) {
                                cx.stop_propagation();
                            } else if this.mask_property_slider_ready(key) {
                                this.slider_key(key, norm, spec, event, cx);
                            }
                        },
                    )),
                );
            controls = controls.child(row);
        }
        controls = controls.child(
            Button::new(SharedString::from(format!("mask-reset-{surface:?}")))
                .label("Reset mask properties")
                .small()
                .outline()
                .disabled(disabled || properties == MaskProperties::default())
                .on_click(
                    cx.listener(move |this, _, _, cx| this.reset_mask_properties(id, target, cx)),
                ),
        );
        if !compact {
            controls = controls.child(mono(
                "Feather uses stored mask pixels, before the mask transform.",
                10.,
                p.muted,
            ));
        }
        controls.into_any_element()
    }
}

fn parse_mask_numeric(text: &str, (min, max, step): (f32, f32, f32)) -> Option<f32> {
    let value = text.trim().parse::<f32>().ok()?;
    (value.is_finite() && (min..=max).contains(&value)).then(|| snap(value, step).clamp(min, max))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    use emulsion_raster::Mask;
    use gpui_kit::{TestAppContext, VisualTestContext, test::TestWindowExt};

    fn setup(cx: &mut TestAppContext) -> (Entity<EditorView>, Vec<NodeId>, &mut VisualTestContext) {
        let mut doc = Document::new(32, 24);
        for name in ["First", "Second"] {
            let mut node = Node::raster(
                0,
                name,
                Arc::new(Raster::solid(32, 24, [0.2, 0.3, 0.4, 1.])),
                Placement::default(),
            );
            node.mask = Some(Arc::new(Mask::from_fn(32, 24, 255, |x, _| {
                if x < 16 { 0 } else { 255 }
            })));
            Command::AddNode {
                node: Box::new(node),
                slot: Slot::TOP,
            }
            .apply(&mut doc)
            .unwrap();
        }
        let ids = doc.nodes.iter().map(|node| node.id).collect::<Vec<_>>();
        let (workspace, cx) = crate::tests::open(cx, doc);
        let editor = cx.update(|_, cx| workspace.read(cx).editor.clone().unwrap());
        cx.simulate_resize(size(px(1440.), px(1000.)));
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.select_layer_mask(ids[0], cx);
                editor.show_sidebar_tab(SidebarTab::Properties, cx);
                window.focus(&editor.canvas_focus, cx);
            })
        });
        cx.run_until_parked();
        (editor, ids, cx)
    }

    fn key(id: NodeId, property: MaskProperty) -> SliderKey {
        SliderKey::MaskProperty(
            id,
            MaskEditTarget::RasterMask,
            property,
            MaskControlSurface::Taskbar,
        )
    }

    fn draft(
        editor: &Entity<EditorView>,
        id: NodeId,
        property: MaskProperty,
        text: &str,
        cx: &mut VisualTestContext,
    ) {
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.start_mask_numeric(key(id, property), window, cx)
            })
        });
        cx.run_until_parked();
        cx.simulate_keystrokes("ctrl-a");
        cx.simulate_input(text);
        cx.run_until_parked();
    }

    fn enter(cx: &mut VisualTestContext) {
        // Native Return dispatch, without the test IME's synthetic newline.
        cx.simulate_event(KeyDownEvent {
            keystroke: Keystroke::parse("enter").unwrap(),
            is_held: false,
            prefer_character_input: false,
        });
        cx.run_until_parked();
    }

    fn start_drag(
        editor: &mut EditorView,
        id: NodeId,
        property: MaskProperty,
        cx: &mut Context<EditorView>,
    ) {
        let key = key(id, property);
        editor.tracks.entry(key).or_default().set(Some(Bounds::new(
            point(px(0.), px(0.)),
            size(px(100.), px(24.)),
        )));
        editor.slider_down(
            key,
            property.spec(),
            &MouseDownEvent {
                position: point(px(50.), px(12.)),
                button: MouseButton::Left,
                modifiers: Modifiers::none(),
                click_count: 1,
                first_mouse: false,
            },
            cx,
        );
    }

    #[test]
    fn mask_numeric_rejects_nonfinite_and_out_of_range_values() {
        for text in ["NaN", "inf", "-1", "100.1", "", "density"] {
            assert_eq!(
                parse_mask_numeric(text, MaskProperty::Density.spec()),
                None,
                "{text}"
            );
        }
        assert_eq!(
            parse_mask_numeric("0", MaskProperty::Density.spec()),
            Some(0.)
        );
        assert_eq!(
            parse_mask_numeric("100", MaskProperty::Density.spec()),
            Some(100.)
        );
        assert_eq!(
            parse_mask_numeric("1000", MaskProperty::Feather.spec()),
            Some(1000.)
        );
        assert_eq!(
            parse_mask_numeric("1000.1", MaskProperty::Feather.spec()),
            None
        );
        assert!(
            (parse_mask_numeric("6.24", MaskProperty::Feather.spec()).unwrap() - 6.2).abs() < 0.001
        );
    }

    #[gpui_kit::test]
    fn mask_numeric_commit_cancel_invalid_and_reset_are_nondestructive(cx: &mut TestAppContext) {
        let (editor, ids, cx) = setup(cx);
        let id = ids[0];
        let before = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
        draft(&editor, id, MaskProperty::Density, "37", cx);
        enter(cx);
        draft(&editor, id, MaskProperty::Feather, "6.5", cx);
        enter(cx);
        cx.update(|_, cx| {
            let editor = editor.read(cx);
            let node = editor.editor.doc.node(id).unwrap();
            assert_eq!(
                node.mask_properties,
                MaskProperties {
                    density: 0.37,
                    feather: 6.5
                }
            );
            assert!(Arc::ptr_eq(
                node.mask.as_ref().unwrap(),
                before.node(id).unwrap().mask.as_ref().unwrap()
            ));
            assert_eq!(node.kind, before.node(id).unwrap().kind);
            assert_eq!(editor.editor.history.len(), 2);
        });
        draft(&editor, id, MaskProperty::Feather, "NaN", cx);
        enter(cx);
        cx.update(|_, cx| assert!(editor.read(cx).tools.photo_masks.edit.is_some()));
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert!(editor.read(cx).tools.photo_masks.edit.is_none());
            assert_eq!(editor.read(cx).editor.history.len(), 2);
        });
        cx.update(|window, cx| window.click("mask-reset-Taskbar", cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                assert_eq!(editor.editor.doc, before);
                assert_eq!(editor.editor.history.len(), 3);
                editor.undo(cx);
                assert_eq!(
                    editor.editor.doc.node(id).unwrap().mask_properties.feather,
                    6.5
                );
                editor.undo(cx);
                editor.undo(cx);
                assert_eq!(editor.editor.doc, before);
            })
        });
    }

    #[gpui_kit::test]
    fn mask_slider_gesture_escape_keyboard_and_noop_history(cx: &mut TestAppContext) {
        let (editor, ids, cx) = setup(cx);
        let id = ids[0];
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                let before = editor.editor.doc.clone();
                start_drag(editor, id, MaskProperty::Density, cx);
                editor.apply_slider(key(id, MaskProperty::Density), 25., cx);
                editor.apply_slider(key(id, MaskProperty::Density), 40., cx);
                assert!(editor.editor.in_transaction());
                assert!(editor.editor.history.is_empty());
                editor.drag_end(cx);
                assert_eq!(editor.editor.history.len(), 1);
                assert!(!editor.editor.in_transaction());
                editor.undo(cx);
                assert_eq!(editor.editor.doc, before);
                start_drag(editor, id, MaskProperty::Feather, cx);
                editor.apply_slider(key(id, MaskProperty::Feather), 8., cx);
                assert!(editor.tool_cancel(cx));
                assert_eq!(editor.editor.doc, before);
                assert!(!editor.editor.in_transaction());
                assert!(editor.drag.is_none());
                let count = editor.editor.history.len();
                editor.apply_mask_property(
                    id,
                    MaskEditTarget::RasterMask,
                    MaskProperty::Feather,
                    0.,
                    cx,
                );
                assert_eq!(editor.editor.history.len(), count);
                editor.slider_key(
                    key(id, MaskProperty::Density),
                    1.,
                    MaskProperty::Density.spec(),
                    &KeyDownEvent {
                        keystroke: Keystroke::parse("left").unwrap(),
                        is_held: false,
                        prefer_character_input: false,
                    },
                    cx,
                );
                assert_eq!(
                    editor.editor.doc.node(id).unwrap().mask_properties.density,
                    0.99
                );
                assert_eq!(editor.editor.history.len(), count + 1);
                editor.undo(cx);
                assert_eq!(editor.editor.doc, before);
            })
        });
    }

    #[gpui_kit::test]
    fn mask_property_target_and_panel_switches_never_retarget_drafts(cx: &mut TestAppContext) {
        let (editor, ids, cx) = setup(cx);
        let id = ids[0];
        for change in 0..3 {
            draft(&editor, id, MaskProperty::Density, "12", cx);
            cx.update(|_, cx| {
                editor.update(cx, |editor, cx| {
                    match change {
                        0 => editor.select_layer_mask(ids[1], cx),
                        1 => editor.select_layer_content(id, cx),
                        _ => editor.select_sidebar(SidebarTab::History, cx),
                    }
                    assert!(editor.tools.photo_masks.edit.is_none());
                    assert!(editor.editor.history.is_empty());
                    for node in &editor.editor.doc.nodes {
                        assert_eq!(node.mask_properties, MaskProperties::default());
                    }
                    editor.select_layer_mask(id, cx);
                    editor.show_sidebar_tab(SidebarTab::Properties, cx);
                })
            });
            cx.run_until_parked();
        }
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                start_drag(editor, id, MaskProperty::Density, cx);
                editor.apply_slider(key(id, MaskProperty::Density), 42., cx);
                editor.select_layer_mask(ids[1], cx);
                assert!(!editor.editor.in_transaction());
                assert!(editor.drag.is_none());
                assert_eq!(editor.editor.history.len(), 1);
                assert_eq!(
                    editor.editor.doc.node(id).unwrap().mask_properties.density,
                    0.42
                );
                // A stale callback still targets the old id and is safely ignored.
                editor.apply_slider(key(id, MaskProperty::Density), 5., cx);
                assert_eq!(
                    editor.editor.doc.node(id).unwrap().mask_properties.density,
                    0.42
                );
                assert_eq!(
                    editor.editor.doc.node(ids[1]).unwrap().mask_properties,
                    MaskProperties::default()
                );
                editor.undo(cx);
                assert_eq!(
                    editor.editor.doc.node(id).unwrap().mask_properties,
                    MaskProperties::default()
                );
                start_drag(editor, ids[1], MaskProperty::Feather, cx);
                editor.apply_slider(key(ids[1], MaskProperty::Feather), 2., cx);
                editor.select_sidebar(SidebarTab::History, cx);
                assert!(!editor.editor.in_transaction());
                assert_eq!(
                    editor
                        .editor
                        .doc
                        .node(ids[1])
                        .unwrap()
                        .mask_properties
                        .feather,
                    2.
                );
            })
        });
    }

    #[gpui_kit::test]
    fn mask_properties_survive_invert_and_paint_but_new_mask_resets(cx: &mut TestAppContext) {
        let (editor, ids, cx) = setup(cx);
        let id = ids[0];
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                let pixels = editor.editor.doc.node(id).unwrap().kind.clone();
                editor.apply_mask_property(
                    id,
                    MaskEditTarget::RasterMask,
                    MaskProperty::Density,
                    50.,
                    cx,
                );
                editor.apply_mask_property(
                    id,
                    MaskEditTarget::RasterMask,
                    MaskProperty::Feather,
                    1.5,
                    cx,
                );
                let properties = editor.editor.doc.node(id).unwrap().mask_properties;
                editor.invert_mask(cx);
                editor.commit_stroke(
                    id,
                    Raster::solid(32, 24, [0., 0., 0., 1.]),
                    emulsion_raster::IRect::new(3, 4, 1, 1),
                    "Paint mask",
                    tools::PaintTarget::Component(MaskEditTarget::RasterMask),
                    cx,
                );
                let node = editor.editor.doc.node(id).unwrap();
                assert_eq!(node.mask_properties, properties);
                assert_eq!(node.kind, pixels);
                assert_eq!(node.mask.as_ref().unwrap().get(3, 4), 0);
                editor.remove_mask(cx);
                assert_eq!(
                    editor.editor.doc.node(id).unwrap().mask_properties,
                    MaskProperties::default()
                );
                editor.add_mask(cx);
                assert_eq!(
                    editor.editor.doc.node(id).unwrap().mask_properties,
                    MaskProperties::default()
                );
                assert_eq!(editor.editor.doc.node(id).unwrap().kind, pixels);
            })
        });
    }

    #[gpui_kit::test]
    fn mask_property_controls_keep_independent_tracks_and_respect_locks(cx: &mut TestAppContext) {
        let (editor, ids, cx) = setup(cx);
        let id = ids[0];
        cx.update(|window, cx| {
            assert!(window.find("mask-properties-Taskbar").visible());
            assert!(window.find("mask-properties-Properties").visible());
            editor.update(cx, |editor, cx| {
                let taskbar = editor
                    .tracks
                    .get(&key(id, MaskProperty::Density))
                    .unwrap()
                    .get()
                    .unwrap();
                let properties = editor
                    .tracks
                    .get(&SliderKey::MaskProperty(
                        id,
                        MaskEditTarget::RasterMask,
                        MaskProperty::Density,
                        MaskControlSurface::Properties,
                    ))
                    .unwrap()
                    .get()
                    .unwrap();
                assert_ne!(taskbar, properties);
                editor.editor.doc.node_mut(id).unwrap().locks.pixels = true;
                editor.apply_mask_property(
                    id,
                    MaskEditTarget::RasterMask,
                    MaskProperty::Density,
                    75.,
                    cx,
                );
                assert_eq!(
                    editor.editor.doc.node(id).unwrap().mask_properties.density,
                    0.75
                );
                editor.editor.doc.node_mut(id).unwrap().locked = true;
                start_drag(editor, id, MaskProperty::Density, cx);
                assert!(!editor.editor.in_transaction());
                assert!(editor.drag.is_none());
                editor.apply_mask_property(
                    id,
                    MaskEditTarget::RasterMask,
                    MaskProperty::Density,
                    25.,
                    cx,
                );
                assert_eq!(
                    editor.editor.doc.node(id).unwrap().mask_properties.density,
                    0.75
                );
            });
        });
    }
    #[gpui_kit::test]
    fn mask_thumbnail_and_selection_follow_effective_disabled_coverage(cx: &mut TestAppContext) {
        let (editor, ids, cx) = setup(cx);
        let id = ids[0];
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                let initial = editor
                    .editor
                    .doc
                    .mask_for_inspection(editor.editor.doc.node(id).unwrap())
                    .unwrap()
                    .unwrap();
                let thumbnail = editor.mask_thumbnail(id, MaskEditTarget::RasterMask, &initial);
                let repeated = editor.mask_thumbnail(id, MaskEditTarget::RasterMask, &initial);
                assert!(Arc::ptr_eq(&thumbnail, &repeated));
                editor.apply_mask_property(
                    id,
                    MaskEditTarget::RasterMask,
                    MaskProperty::Density,
                    50.,
                    cx,
                );
                editor.execute(Command::SetMaskEnabled { id, enabled: false }, cx);
                let processed = editor
                    .editor
                    .doc
                    .mask_for_inspection(editor.editor.doc.node(id).unwrap())
                    .unwrap()
                    .unwrap();
                let changed = editor.mask_thumbnail(id, MaskEditTarget::RasterMask, &processed);
                assert!(!Arc::ptr_eq(&thumbnail, &changed));
                assert_eq!(processed.get(3, 4), 128);
                assert_eq!(
                    editor
                        .editor
                        .doc
                        .node(id)
                        .unwrap()
                        .mask
                        .as_ref()
                        .unwrap()
                        .get(3, 4),
                    0
                );
                editor.mask_to_selection(cx);
                assert_eq!(editor.editor.doc.selection.as_ref().unwrap().get(3, 4), 128);
                assert_eq!(
                    editor.editor.doc.selection.as_ref().unwrap().get(24, 4),
                    255
                );
            })
        });
    }
    #[gpui_kit::test]
    fn fill_object_click_uses_effective_mask_density_and_transform(cx: &mut TestAppContext) {
        let (editor, ids, cx) = setup(cx);
        let id = ids[0];
        cx.update(|_, cx| editor.update(cx, |editor, cx| {
            editor.select_layer_content(id, cx);
            editor.editor.doc.node_mut(id).unwrap().kind = NodeKind::Fill { rgba: [255; 4] };
            let color = [120, 30, 50, 255];
            let at = editor.doc_to_window((3.5, 4.5)).unwrap();
            let nodes = editor.editor.doc.nodes.len();
            // Full-density black coverage prevents recoloring this object.
            editor.color_drop(color, at, cx);
            assert_eq!(editor.editor.doc.nodes.len(), nodes);
            assert!(matches!(editor.editor.doc.node(id).unwrap().kind, NodeKind::Fill { rgba } if rgba == [255; 4]));
            // Reduced density reveals that exact same stored-black pixel.
            editor.apply_mask_property(id, MaskEditTarget::RasterMask, MaskProperty::Density, 50., cx);
            editor.color_drop(color, at, cx);
            assert_eq!(editor.editor.doc.nodes.len(), nodes);
            assert!(matches!(editor.editor.doc.node(id).unwrap().kind, NodeKind::Fill { rgba } if rgba == color));
            // Moving the mask away also exposes outside-fill coverage.
            editor.apply_mask_property(id, MaskEditTarget::RasterMask, MaskProperty::Density, 100., cx);
            { let mut affine = editor.editor.doc.node_mut(id).unwrap().mask_transform.affine().expect("affine fixture"); let mut columns = affine.to_cols_array(); columns[4] = 8.; affine = glam::DAffine2::from_cols_array(&columns); editor.editor.doc.node_mut(id).unwrap().mask_transform =emulsion_core::Mapping2::Affine(affine); }
            let next = [20, 50, 120, 255];
            editor.color_drop(next, at, cx);
            assert_eq!(editor.editor.doc.nodes.len(), nodes);
            assert!(matches!(editor.editor.doc.node(id).unwrap().kind, NodeKind::Fill { rgba } if rgba == next));
        }));
    }
    fn vector_key(id: NodeId, property: MaskProperty) -> SliderKey {
        SliderKey::MaskProperty(
            id,
            MaskEditTarget::VectorMask,
            property,
            MaskControlSurface::Taskbar,
        )
    }
    fn vector_draft(
        editor: &Entity<EditorView>,
        id: NodeId,
        property: MaskProperty,
        text: &str,
        cx: &mut VisualTestContext,
    ) {
        cx.update(|window, cx| {
            editor.update(cx, |e, cx| {
                e.start_mask_numeric(vector_key(id, property), window, cx)
            })
        });
        cx.run_until_parked();
        cx.simulate_keystrokes("ctrl-a");
        cx.simulate_input(text);
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    fn vector_mask_numeric_enter_escape_and_stale_component_switch_are_independent(
        cx: &mut TestAppContext,
    ) {
        let (editor, ids, cx) = setup(cx);
        let id = ids[0];
        cx.update(|_, cx| {
            editor.update(cx, |e, cx| {
                e.editor.doc.node_mut(id).unwrap().vector_mask =
                    Some(emulsion_core::VectorMask::default());
                e.select_layer_vector_mask(id, cx);
            })
        });
        cx.run_until_parked();
        let before = cx.update(|_, cx| editor.read(cx).editor.doc.clone());
        vector_draft(&editor, id, MaskProperty::Density, "31", cx);
        enter(cx);
        vector_draft(&editor, id, MaskProperty::Feather, "7.2", cx);
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = editor.read(cx);
            let n = e.editor.doc.node(id).unwrap();
            assert_eq!(
                n.vector_mask.as_ref().unwrap().properties,
                MaskProperties {
                    density: 0.31,
                    feather: 0.
                }
            );
            assert_eq!(n.mask_properties, before.node(id).unwrap().mask_properties);
            assert_eq!(n.kind, before.node(id).unwrap().kind);
            assert_eq!(e.editor.history.len(), 1);
        });
        vector_draft(&editor, id, MaskProperty::Density, "77", cx);
        cx.update(|_, cx| editor.update(cx, |e, cx| e.select_layer_mask(id, cx)));
        enter(cx);
        cx.update(|_, cx| {
            editor.update(cx, |e, cx| {
                assert!(e.tools.photo_masks.edit.is_none());
                e.apply_slider(vector_key(id, MaskProperty::Density), 99., cx);
                assert_eq!(
                    e.editor
                        .doc
                        .node(id)
                        .unwrap()
                        .vector_mask
                        .as_ref()
                        .unwrap()
                        .properties
                        .density,
                    0.31
                );
                assert_eq!(e.editor.doc.node(id).unwrap().mask_properties.density, 1.);
                assert_eq!(e.editor.history.len(), 1);
                e.undo(cx);
                assert_eq!(e.editor.doc, before);
            })
        });
    }

    #[gpui_kit::test]
    fn vector_mask_property_slider_one_gesture_cancel_and_component_tracks(
        cx: &mut TestAppContext,
    ) {
        let (editor, ids, cx) = setup(cx);
        let id = ids[0];
        cx.update(|_, cx| {
            editor.update(cx, |e, cx| {
                e.editor.doc.node_mut(id).unwrap().vector_mask =
                    Some(emulsion_core::VectorMask::default());
                e.select_layer_vector_mask(id, cx);
            })
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            editor.update(cx, |e, cx| {
                let before = e.editor.doc.clone();
                let key = vector_key(id, MaskProperty::Density);
                let raster_key = key_for_raster(id);
                assert_ne!(key, raster_key);
                let start = |e: &mut EditorView, cx: &mut Context<EditorView>| {
                    e.tracks.entry(key).or_default().set(Some(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(100.), px(24.)),
                    )));
                    e.slider_down(
                        key,
                        MaskProperty::Density.spec(),
                        &MouseDownEvent {
                            position: point(px(50.), px(12.)),
                            button: MouseButton::Left,
                            modifiers: Modifiers::none(),
                            click_count: 1,
                            first_mouse: false,
                        },
                        cx,
                    );
                };
                start(e, cx);
                e.apply_slider(key, 20., cx);
                e.apply_slider(key, 40., cx);
                assert!(e.editor.history.is_empty());
                e.finish_mask_properties();
                assert_eq!(e.editor.history.len(), 1);
                assert_eq!(
                    e.editor
                        .doc
                        .node(id)
                        .unwrap()
                        .vector_mask
                        .as_ref()
                        .unwrap()
                        .properties
                        .density,
                    0.4
                );
                assert_eq!(e.editor.doc.node(id).unwrap().mask_properties.density, 1.);
                start(e, cx);
                e.apply_slider(key, 90., cx);
                assert!(e.cancel_mask_properties(cx));
                assert_eq!(
                    e.editor
                        .doc
                        .node(id)
                        .unwrap()
                        .vector_mask
                        .as_ref()
                        .unwrap()
                        .properties
                        .density,
                    0.4
                );
                assert_eq!(e.editor.history.len(), 1);
                e.undo(cx);
                assert_eq!(e.editor.doc, before);
            })
        });
        fn key_for_raster(id: NodeId) -> SliderKey {
            key(id, MaskProperty::Density)
        }
    }
}
