//! Real Layers panel regressions for lazy row construction and changing row geometry.
use super::*;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{Modifiers, ScrollDelta, SharedString, point, px, size};

fn many_layers(count: usize) -> Document {
    let mut document = Document::new(8, 8);
    for index in 1..=count {
        // Distinct allocations make the thumbnail cache a count of constructed
        // raster rows, even when the pixels happen to be identical.
        Command::AddNode {
            node: Box::new(Node::raster(
                0,
                format!("Layer {index:03}"),
                Arc::new(Raster::solid(8, 8, [0.2, 0.3, 0.4, 1.0])),
                Placement::default(),
            )),
            slot: Slot::TOP,
        }
        .apply(&mut document)
        .unwrap();
    }
    document
}

fn scroll_layers(cx: &mut VisualTestContext, delta: f32) {
    cx.update(|window, cx| {
        window.scroll(
            "sidebar-layers-list",
            ScrollDelta::Pixels(point(px(0.), px(delta))),
            cx,
        );
    });
    cx.run_until_parked();
}

#[gpui_kit::test]
fn hundreds_of_layers_only_construct_nearby_rows_and_select_across_unmounted_rows(
    cx: &mut TestAppContext,
) {
    // Stay below thumbnail-cache eviction capacity so eager construction cannot
    // pass the assertion by clearing its cache halfway through the document.
    let document = many_layers(240);
    let (ws, cx) = open(cx, document.clone());
    cx.simulate_resize(size(px(1100.), px(900.)));
    cx.run_until_parked();
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|window, cx| {
        let built = editor.read(cx).thumbs.len();
        assert!(
            built > 0 && built < 40,
            "constructed {built} of 240 raster rows"
        );
        assert!(window.find(("row", 240u64)).visible());
        assert!(window.try_find(("row", 120u64)).is_none());
        assert!(window.try_find(("row", 1u64)).is_none());
        window.click(("row", 240u64), cx);
    });
    cx.run_until_parked();
    scroll_layers(cx, -100_000.);
    let bottom = cx.update(|window, _| {
        assert!(window.find(("row", 1u64)).visible());
        assert!(window.try_find(("row", 240u64)).is_none());
        window.find(("row", 1u64)).bounds().center()
    });
    cx.simulate_click(
        bottom,
        Modifiers {
            shift: true,
            ..Modifiers::none()
        },
    );
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        assert_eq!(e.selected, Some(1));
        assert_eq!(e.selected_layer_ids(), (1..=240).collect::<Vec<_>>());
        assert_eq!(e.editor.doc, document);
        assert!(
            e.editor.history.is_empty(),
            "scroll and selection do not edit the document"
        );
    });
}

#[gpui_kit::test]
fn expanding_effects_remeasures_virtual_rows_and_preserves_effect_identity(
    cx: &mut TestAppContext,
) {
    let mut document = many_layers(80);
    let top = 80u64;
    Command::SetLayerEffects {
        id: top,
        styles: emulsion_core::styles::LayerStyle::catalogue()[..2].to_vec(),
        options: vec![
            emulsion_core::style_options::StyleOptions {
                id: 7,
                ..Default::default()
            },
            emulsion_core::style_options::StyleOptions {
                id: 9,
                ..Default::default()
            },
        ],
    }
    .apply(&mut document)
    .unwrap();
    let (ws, cx) = open(cx, document.clone());
    cx.simulate_resize(size(px(1100.), px(1100.)));
    cx.run_until_parked();
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    let effect = SharedString::from(format!("effect-visible-{top}-7"));
    let expanded_y = cx.update(|window, cx| {
        assert!(window.find(effect.clone()).visible());
        let next_y = window.find(("row", 79u64)).bounds().origin.y;
        window.click(("effects-expand", top), cx);
        next_y
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.try_find(effect.clone()).is_none());
        assert!(window.find(("row", 79u64)).bounds().origin.y < expanded_y);
        assert_eq!(editor.read(cx).editor.doc, document);
        window.click(("effects-expand", top), cx);
    });
    cx.run_until_parked();
    cx.update(|window, _| {
        assert!(window.find(effect.clone()).visible());
        assert_eq!(window.find(("row", 79u64)).bounds().origin.y, expanded_y);
    });
    scroll_layers(cx, -100_000.);
    cx.update(|window, _| assert!(window.try_find(effect.clone()).is_none()));
    scroll_layers(cx, 100_000.);
    cx.update(|window, cx| window.click(effect, cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        let node = e.editor.doc.node(top).unwrap();
        assert!(!node.style_options[0].enabled);
        assert!(node.style_options[1].enabled);
        assert_eq!(e.editor.history.len(), 1);
        assert_eq!(e.editor.doc.node(79), document.node(79));
    });
}

#[gpui_kit::test]
fn filtering_from_bottom_reveals_matching_row_and_rename_survives_virtualization(
    cx: &mut TestAppContext,
) {
    let (ws, cx) = open(cx, many_layers(120));
    cx.simulate_resize(size(px(1100.), px(900.)));
    cx.run_until_parked();
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    scroll_layers(cx, -100_000.);
    let search = cx.update(|_, cx| {
        editor
            .read(cx)
            .layer_panel
            .search
            .as_ref()
            .unwrap()
            .0
            .clone()
    });
    cx.update(|window, cx| search.update(cx, |state, cx| state.focus(window, cx)));
    cx.simulate_input("Layer 120");
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find(("row", 120u64)).visible());
        assert!(window.try_find(("row", 1u64)).is_none());
        window.double_click(("layer-name", 120u64), cx);
    });
    cx.run_until_parked();
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-a"
    } else {
        "ctrl-a"
    });
    cx.simulate_input("Renamed top layer");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(
            editor.read(cx).editor.doc.node(120).unwrap().name,
            "Renamed top layer"
        );
        assert_eq!(editor.read(cx).editor.history.len(), 1);
        search.update(cx, |state, cx| state.focus(window, cx));
    });
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-a backspace"
    } else {
        "ctrl-a backspace"
    });
    cx.run_until_parked();
    scroll_layers(cx, 100_000.);
    cx.update(|window, cx| {
        assert!(window.find(("row", 120u64)).visible());
        assert_eq!(editor.read(cx).selected, Some(120));
    });
}

#[gpui_kit::test]
fn keyboard_edge_selection_reveals_rows_and_offscreen_rename_keeps_input_dispatch(
    cx: &mut TestAppContext,
) {
    let (ws, cx) = open(cx, many_layers(120));
    cx.simulate_resize(size(px(1100.), px(900.)));
    cx.run_until_parked();
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|window, cx| window.click(("row", 120u64), cx));
    cx.simulate_keystrokes("alt-,");
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(editor.read(cx).selected, Some(1));
        assert!(window.find(("row", 1u64)).visible());
    });
    cx.simulate_keystrokes("alt-.");
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(editor.read(cx).selected, Some(120));
        assert!(window.find(("row", 120u64)).visible());
    });
    scroll_layers(cx, -100_000.);
    cx.update(|window, cx| {
        assert_eq!(editor.read(cx).selected, Some(120));
        assert!(window.find(("row", 1u64)).visible());
    });
    // Repeating the command must reveal its target even if selection did not change.
    cx.simulate_keystrokes("alt-.");
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find(("row", 120u64)).visible());
        window.double_click(("layer-name", 120u64), cx);
    });
    cx.run_until_parked();
    scroll_layers(cx, -100_000.);
    cx.update(|window, _| assert!(window.find(("row", 1u64)).visible()));
    // Scrolling does not move keyboard focus. List must preserve dispatch for
    // the focused input even while its row is outside the viewport.
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-a"
    } else {
        "ctrl-a"
    });
    cx.simulate_input("Renamed while scrolled away");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(
            editor.read(cx).editor.doc.node(120).unwrap().name,
            "Renamed while scrolled away"
        );
        assert_eq!(editor.read(cx).editor.history.len(), 1);
    });
}

#[gpui_kit::test]
fn hundreds_of_effect_rows_are_virtualized_and_keep_layer_identity(cx: &mut TestAppContext) {
    let mut document = many_layers(20);
    let style = emulsion_core::styles::LayerStyle::catalogue()[0].clone();
    for layer in 1..=20 {
        Command::SetLayerEffects {
            id: layer,
            styles: vec![style.clone(); 12],
            options: (1..=12)
                .map(|id| emulsion_core::style_options::StyleOptions {
                    id,
                    ..Default::default()
                })
                .collect(),
        }
        .apply(&mut document)
        .unwrap();
        // Stay within the supported per-layer limit while exercising 240 rows,
        // without paying for 240 image filters in this UI regression.
        Command::SetEffectsEnabled {
            id: layer,
            enabled: false,
        }
        .apply(&mut document)
        .unwrap();
    }
    let (ws, cx) = open(cx, document);
    cx.simulate_resize(size(px(1100.), px(1100.)));
    cx.run_until_parked();
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|window, _| {
        assert!(window.find("effect-visible-20-1").visible());
        assert!(window.try_find("effect-visible-10-1").is_none());
        assert!(window.try_find("effect-visible-1-12").is_none());
    });
    scroll_layers(cx, -100_000.);
    cx.update(|window, cx| {
        assert!(window.try_find("effect-visible-20-1").is_none());
        assert!(window.find("effect-visible-1-12").visible());
        window.click("effect-visible-1-12", cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        let node = e.editor.doc.node(1).unwrap();
        assert!(node.style_options[..11].iter().all(|option| option.enabled));
        assert!(!node.style_options[11].enabled);
        assert!(
            e.editor
                .doc
                .nodes
                .iter()
                .filter(|node| node.id != 1)
                .all(|node| node.style_options.iter().all(|option| option.enabled))
        );
        assert_eq!(e.editor.history.len(), 1);
    });
}

#[gpui_kit::test]
fn dragging_after_scrolling_reorders_the_target_layers_and_undo_restores_them(
    cx: &mut TestAppContext,
) {
    let original = many_layers(120);
    let (ws, cx) = open(cx, original.clone());
    cx.simulate_resize(size(px(1100.), px(1100.)));
    cx.run_until_parked();
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    scroll_layers(cx, -100_000.);
    cx.update(|window, cx| {
        let source = window.find(("layer-name", 1u64)).bounds().center();
        let target = window.find(("layer-name", 3u64)).bounds().center();
        window.drag(source, target, cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = editor.read(cx);
        let expected: Vec<_> = [2, 3, 1].into_iter().chain(4..=120).collect();
        assert_eq!(e.editor.doc.children(None), expected);
        assert_eq!(e.selected, Some(1));
        assert_eq!(e.editor.history.len(), 1);
        editor.update(cx, |e, cx| e.undo(cx));
        assert_eq!(editor.read(cx).editor.doc, original);
    });
}

#[gpui_kit::test]
fn expanding_an_offscreen_group_preserves_scroll_anchor_and_density_changes_keep_ends_reachable(
    cx: &mut TestAppContext,
) {
    let mut document = many_layers(120);
    let group = Command::AddNode {
        node: Box::new(Node::group(0, "Offscreen group")),
        slot: Slot::TOP,
    }
    .apply(&mut document)
    .unwrap()
    .unwrap();
    for index in 0..3 {
        Command::AddNode {
            node: Box::new(Node::raster(
                0,
                format!("Child {index}"),
                Arc::new(Raster::solid(8, 8, [0.2, 0.3, 0.4, 1.0])),
                Placement::default(),
            )),
            slot: Slot::top_of(Some(group)),
        }
        .apply(&mut document)
        .unwrap();
    }
    Command::SetCollapsed {
        id: group,
        collapsed: true,
    }
    .apply(&mut document)
    .unwrap();
    let (ws, cx) = open(cx, document);
    cx.simulate_resize(size(px(1100.), px(1000.)));
    cx.run_until_parked();
    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    scroll_layers(cx, -1700.);
    let (anchor, anchor_y) = cx.update(|window, cx| {
        assert!(window.try_find(("row", group)).is_none());
        let anchor = (1..=120u64)
            .filter_map(|id| {
                window
                    .try_find(("row", id))
                    .filter(|row| row.visible())
                    .map(|row| (id, row.bounds().origin.y))
            })
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .expect("a layer is visible midway through the list");
        editor.update(cx, |e, cx| {
            e.execute(
                Command::SetCollapsed {
                    id: group,
                    collapsed: false,
                },
                cx,
            );
        });
        anchor
    });
    cx.run_until_parked();
    cx.update(|window, _| {
        assert_eq!(window.find(("row", anchor)).bounds().origin.y, anchor_y);
    });
    cx.simulate_resize(size(px(1000.), px(760.)));
    cx.update(|window, cx| {
        cx.global_mut::<AppSettings>().0.compact_chrome = true;
        window.set_rem_size(px(18.));
        window.refresh();
    });
    cx.run_until_parked();
    scroll_layers(cx, -100_000.);
    cx.update(|window, _| assert!(window.find(("row", 1u64)).visible()));
    scroll_layers(cx, 100_000.);
    cx.update(|window, cx| {
        assert!(window.find(("row", group)).visible());
        assert!(
            editor.read(cx).editor.history.is_empty(),
            "group expansion is view-only"
        );
        assert!(matches!(
            editor.read(cx).editor.doc.node(group).unwrap().kind,
            emulsion_core::NodeKind::Group { collapsed: false }
        ));
    });
}
