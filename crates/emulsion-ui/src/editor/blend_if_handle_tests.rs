use super::*;
use crate::tests::open;
use core::prelude::v1::test;
use gpui_kit::test::TestWindowExt;

fn doc(names: &[&str], pixels: Option<Raster>) -> Document {
    let mut doc = Document::new(16, 16);
    for (index, name) in names.iter().enumerate() {
        let color = if index == 0 {
            [0., 0., 0., 1.]
        } else {
            [1.; 4]
        };
        Command::AddNode {
            node: Box::new(Node::raster(
                0,
                *name,
                Arc::new(
                    pixels
                        .clone()
                        .unwrap_or_else(|| Raster::solid(16, 16, color)),
                ),
                Placement::default(),
            )),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap();
    }
    doc
}

fn reveal(cx: &mut VisualTestContext, target: impl Into<ElementId>) {
    let target = target.into();
    cx.update(|window, cx| {
        window.render_frame(cx);
        let pane = window.find("style-dialog-settings").bounds();
        let bounds = window.within("layer-style-dialog").find(target).bounds();
        let dy = if bounds.bottom() > pane.bottom() {
            pane.bottom() - bounds.bottom() - px(4.)
        } else if bounds.top() < pane.top() {
            pane.top() - bounds.top() + px(4.)
        } else {
            return;
        };
        window.scroll(
            "style-dialog-settings",
            gpui_kit::ScrollDelta::Pixels(point(px(0.), dy)),
            cx,
        );
    });
    cx.run_until_parked();
}

fn click(cx: &mut VisualTestContext, target: impl Into<ElementId>) {
    let target = target.into();
    cx.run_until_parked();
    let pos = cx.update(|window, cx| {
        window.render_frame(cx);
        if matches!(&target, ElementId::Name(name) if name.as_ref() == "style-dialog-ok" || name.as_ref() == "style-dialog-cancel") {
            window.find(target).bounds().center()
        } else {
            window.within("layer-style-dialog").find(target).bounds().center()
        }
    });
    cx.simulate_click(pos, Default::default());
    cx.run_until_parked();
}

fn open_blend_if(view: &Entity<EditorView>, id: NodeId, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        view.update(cx, |e, cx| {
            e.open_blending_options(id, window, cx);
            e.styles_ui.blend_if_open = true;
            cx.notify();
        })
    });
    cx.run_until_parked();
}

fn drag_half(
    cx: &mut VisualTestContext,
    backdrop: bool,
    index: usize,
    delta: f32,
    alt: bool,
    release: bool,
) -> Point<Pixels> {
    let target = format!("blend-if-handle-{backdrop}-{index}");
    reveal(cx, target.clone());
    let (from, to) = cx.update(|window, cx| {
        window.render_frame(cx);
        let from = window
            .within("layer-style-dialog")
            .find(target)
            .bounds()
            .center();
        let width = window
            .within("layer-style-dialog")
            .find(("blend-if-gradient", usize::from(backdrop)))
            .bounds()
            .size
            .width;
        (from, from + point(width * delta, px(0.)))
    });
    let mods = Modifiers {
        alt,
        ..Default::default()
    };
    cx.simulate_mouse_down(from, MouseButton::Left, mods);
    cx.simulate_mouse_move(to, Some(MouseButton::Left), mods);
    if release {
        cx.simulate_mouse_up(to, MouseButton::Left, mods);
    }
    cx.run_until_parked();
    from
}

fn near(value: f32, expected: f32) {
    assert!(
        (value - expected).abs() <= 1. / 255. + 1e-5,
        "{value} != {expected}"
    );
}

#[test]
fn blend_if_handles_split_rejoin_and_clamp_without_moving_neighbors() {
    let original = [0., 0., 1., 1.];
    assert_eq!(
        BlendIfHandle::for_half(original, 0, false),
        BlendIfHandle::JoinedBlack
    );
    assert_eq!(
        BlendIfHandle::for_half(original, 3, false),
        BlendIfHandle::JoinedWhite
    );
    assert_eq!(
        BlendIfHandle::for_half(original, 1, true),
        BlendIfHandle::Half(1)
    );
    let joined = BlendIfHandle::JoinedBlack.moved(original, 0.25);
    assert_eq!(joined, [0.25, 0.25, 1., 1.]);
    let split = BlendIfHandle::Half(1).moved(joined, 0.5);
    assert_eq!(split, [0.25, 0.5, 1., 1.]);
    assert_eq!(
        BlendIfHandle::for_half(split, 0, false),
        BlendIfHandle::Half(0)
    );
    assert_eq!(BlendIfHandle::Half(0).moved(split, 2.), [0.5, 0.5, 1., 1.]);
    assert_eq!(BlendIfHandle::Half(1).moved(split, -1.), joined);
    assert_eq!(
        BlendIfHandle::JoinedWhite.moved(split, -1.),
        [0.25, 0.5, 0.5, 0.5]
    );
    assert_eq!(
        BlendIfHandle::Half(2).moved(split, -1.),
        [0.25, 0.5, 0.5, 1.]
    );
    assert_eq!(BlendIfHandle::Half(3).moved(split, -1.), split);
    assert_eq!(BlendIfHandle::Half(0).moved(split, f32::NAN), split);
    assert_eq!(BlendIfHandle::Half(99).moved(split, 0.), split);
}

#[gpui_kit::test]
fn blend_if_pointer_halves_edit_both_ranges_channels_and_commit_one_undo(cx: &mut TestAppContext) {
    let original = doc(&["Base", "Photo"], None);
    let id = original.nodes[1].id;
    let (workspace, cx) = open(cx, original.clone());
    cx.simulate_resize(size(px(1000.), px(900.)));
    let view = cx.update(|_, cx| workspace.read(cx).editor.clone().unwrap());
    open_blend_if(&view, id, cx);
    for backdrop in [false, true] {
        drag_half(cx, backdrop, 1, 0.25, false, true);
        cx.update(|_, cx| {
            let options = view.read(cx).editor.doc.node(id).unwrap().blending;
            let range = if backdrop {
                options.blend_if.backdrop
            } else {
                options.blend_if.source
            };
            near(range.black, 0.25);
            assert_eq!(range.black, range.black_fade);
            assert_eq!(view.read(cx).editor.transaction_depth(), 1);
        });
        drag_half(cx, backdrop, 1, 0.25, true, true);
        drag_half(cx, backdrop, 0, -0.125, false, true);
        drag_half(cx, backdrop, 2, -0.125, false, true);
        drag_half(cx, backdrop, 2, -0.125, true, true);
        cx.update(|_, cx| {
            let options = view.read(cx).editor.doc.node(id).unwrap().blending;
            let range = if backdrop {
                options.blend_if.backdrop
            } else {
                options.blend_if.source
            };
            near(range.black, 0.125);
            near(range.black_fade, 0.5);
            near(range.white_fade, 0.75);
            near(range.white, 0.875);
            assert!(range.valid());
            assert_eq!(view.read(cx).editor.history.len(), 0);
            assert!(view.read(cx).drag.is_none());
        });
    }
    for index in 0usize..4 {
        reveal(cx, ("blend-if-channel", index));
        click(cx, ("blend-if-channel", index));
        cx.update(|_, cx| {
            let e = view.read(cx);
            assert_eq!(
                e.editor.doc.node(id).unwrap().blending.blend_if.channel,
                [
                    BlendIfChannel::Gray,
                    BlendIfChannel::Red,
                    BlendIfChannel::Green,
                    BlendIfChannel::Blue
                ][index]
            );
        });
    }
    click(cx, "style-dialog-ok");
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            assert!(e.styles_ui.dialog_for.is_none());
            assert!(!e.editor.in_transaction());
            assert_eq!(e.editor.history.len(), 1);
            let changed = e.editor.doc.clone();
            let before =
                emulsion_raster::composite::flatten(&original.composite_tree(), 0).to_srgba8();
            let after =
                emulsion_raster::composite::flatten(&changed.composite_tree(), 0).to_srgba8();
            assert_ne!(before, after, "range controls must change rendered pixels");
            let dir = tempfile::tempdir().unwrap();
            let file = dir.path().join("blend-if-handles.ora");
            emulsion_io::ora::write(&changed, &file).unwrap();
            let reopened = emulsion_io::ora::read(&file).unwrap();
            assert_eq!(
                reopened.node(id).unwrap().blending,
                changed.node(id).unwrap().blending
            );
            e.undo(cx);
            assert_eq!(e.editor.doc, original);
            e.redo(cx);
            assert_eq!(e.editor.doc, changed);
        })
    });
}

#[gpui_kit::test]
fn blend_if_handle_cancel_reset_and_click_noop_preserve_history(cx: &mut TestAppContext) {
    let original = doc(&["Photo"], None);
    let id = original.nodes[0].id;
    let (workspace, cx) = open(cx, original.clone());
    cx.simulate_resize(size(px(900.), px(720.)));
    let view = cx.update(|_, cx| workspace.read(cx).editor.clone().unwrap());
    for escape in [false, true] {
        open_blend_if(&view, id, cx);
        drag_half(cx, false, 1, 0.3, true, false);
        if escape {
            cx.simulate_keystrokes("escape");
            cx.run_until_parked();
        } else {
            click(cx, "style-dialog-cancel");
        }
        cx.update(|_, cx| {
            let e = view.read(cx);
            assert_eq!(e.editor.doc, original);
            assert!(e.drag.is_none());
            assert!(!e.editor.in_transaction());
            assert_eq!(e.editor.history.len(), 0);
        });
    }
    open_blend_if(&view, id, cx);
    drag_half(cx, false, 1, 0., false, true);
    cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, original));
    // A real excursion followed by the exact original pointer position must
    // also restore the imported non-integer bound without quantizing it.
    let from = drag_half(cx, false, 1, 0.1, false, false);
    cx.simulate_mouse_move(from, Some(MouseButton::Left), Default::default());
    cx.simulate_mouse_up(from, MouseButton::Left, Default::default());
    cx.run_until_parked();
    click(cx, "style-dialog-ok");
    cx.update(|_, cx| {
        assert_eq!(view.read(cx).editor.doc, original);
        assert_eq!(view.read(cx).editor.history.len(), 0);
    });
    open_blend_if(&view, id, cx);
    drag_half(cx, true, 2, -0.4, true, true);
    reveal(cx, "blend-if-reset");
    click(cx, "blend-if-reset");
    click(cx, "blend-if-reset");
    click(cx, "style-dialog-ok");
    cx.update(|_, cx| {
        assert_eq!(view.read(cx).editor.doc, original);
        assert_eq!(view.read(cx).editor.history.len(), 0);
    });
}

#[gpui_kit::test]
fn blend_if_numeric_sliders_keep_distinct_tracks_and_lock_safety(cx: &mut TestAppContext) {
    let original = doc(&["Photo"], None);
    let id = original.nodes[0].id;
    let (workspace, cx) = open(cx, original.clone());
    cx.simulate_resize(size(px(900.), px(720.)));
    let view = cx.update(|_, cx| workspace.read(cx).editor.clone().unwrap());
    open_blend_if(&view, id, cx);
    drag_half(cx, false, 1, 0.2, true, true);
    let key = SliderKey::BlendRange(id, false, 2);
    let target = format!("{key:?}");
    reveal(cx, target.clone());
    let at = cx.update(|window, _| {
        let bounds = window.within("layer-style-dialog").find(target).bounds();
        point(bounds.left() + bounds.size.width * 0.6, bounds.center().y)
    });
    cx.simulate_click(at, Default::default());
    cx.run_until_parked();
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            near(
                e.editor
                    .doc
                    .node(id)
                    .unwrap()
                    .blending
                    .blend_if
                    .source
                    .white_fade,
                0.6,
            );
            let unchanged = e.editor.doc.clone();
            e.set_blend_range(id, false, 99, 0.5, cx);
            e.set_blend_range(id, false, 1, f32::NAN, cx);
            assert_eq!(e.editor.doc, unchanged);
            e.editor.doc.node_mut(id).unwrap().locked = true;
            let locked = e.editor.doc.clone();
            e.set_blend_if_handle(id, false, BlendIfHandle::JoinedBlack, 0.5, cx);
            assert_eq!(e.editor.doc, locked);
            e.editor.doc.node_mut(id).unwrap().locked = false;
        })
    });
    click(cx, "style-dialog-cancel");
    cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, original));
}

/// Regression: global history must not end the style dialog's transaction and
/// leave its preview live with no owner. Prior code fails this after `undo`.
#[gpui_kit::test]
fn blend_if_modal_history_cannot_orphan_transaction(cx: &mut TestAppContext) {
    let original = doc(&["Photo"], None);
    let id = original.nodes[0].id;
    let (workspace, cx) = open(cx, original.clone());
    cx.simulate_resize(size(px(1000.), px(900.)));
    let view = cx.update(|_, cx| workspace.read(cx).editor.clone().unwrap());
    open_blend_if(&view, id, cx);
    drag_half(cx, false, 1, 0.3, true, false);
    cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            let preview = e.editor.doc.clone();
            let depth = e.editor.transaction_depth();
            e.undo(cx);
            assert_eq!(e.editor.doc, preview);
            assert_eq!(e.editor.transaction_depth(), depth);
            assert_eq!(e.styles_ui.dialog_for, Some(id));
            e.redo(cx);
            e.undo_to(1, cx);
            assert_eq!(e.editor.doc, preview);
            assert_eq!(e.editor.transaction_depth(), depth);
            assert_eq!(e.editor.history.len(), 0);
        })
    });
    click(cx, "style-dialog-cancel");
    cx.update(|_, cx| {
        assert_eq!(view.read(cx).editor.doc, original);
        assert!(!view.read(cx).editor.in_transaction());
        assert_eq!(view.read(cx).editor.history.len(), 0);
    });
}

#[gpui_kit::test]
fn blend_if_fractional_bound_click_is_exact_noop_at_narrow_width(cx: &mut TestAppContext) {
    let mut original = doc(&["Photo"], None);
    let id = original.nodes[0].id;
    original.nodes[0].blending.blend_if.source.black_fade = 0.3;
    let (workspace, cx) = open(cx, original.clone());
    cx.simulate_resize(size(px(680.), px(620.)));
    let view = cx.update(|_, cx| workspace.read(cx).editor.clone().unwrap());
    open_blend_if(&view, id, cx);
    reveal(cx, "blend-if-handle-false-1");
    cx.update(|window, _| {
        let pane = window.find("style-dialog-settings").bounds();
        let gradient = window
            .within("layer-style-dialog")
            .find(("blend-if-gradient", 0usize))
            .bounds();
        assert!(gradient.size.width > px(32.));
        for index in 0..4 {
            let bounds = window
                .within("layer-style-dialog")
                .find(format!("blend-if-handle-false-{index}"))
                .bounds();
            assert!(bounds.left() >= pane.left() && bounds.right() <= pane.right());
            assert!(bounds.left() >= px(0.) && bounds.right() <= window.viewport_size().width);
        }
    });
    drag_half(cx, false, 1, 0., false, true);
    click(cx, "style-dialog-ok");
    cx.update(|_, cx| {
        assert_eq!(view.read(cx).editor.doc, original);
        assert_eq!(view.read(cx).editor.history.len(), 0);
    });
}
