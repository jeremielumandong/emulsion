//! Headless UI tests: real views in an offscreen window, driven through the
//! same key bindings and actions a person uses.

use crate::app_state::{AppSettings, Capabilities, CliStatus};
use crate::workspace::Workspace;
use crate::{actions, theme};
use emulsion_core::command::Slot;
use emulsion_core::{Command, Document, Node};
use emulsion_io::settings::Settings;
use emulsion_raster::{Placement, Raster};
use gpui_kit::component::Root;
use gpui_kit::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

fn doc(names: &[&str], pixels: Option<Raster>) -> Document {
    let mut d = Document::new(256, 192);
    for name in names {
        let r = pixels
            .clone()
            .unwrap_or_else(|| Raster::solid(256, 192, [0.2, 0.3, 0.4, 1.0]));
        Command::AddNode {
            node: Box::new(Node::raster(0, *name, Arc::new(r), Placement::default())),
            slot: Slot::TOP,
        }
        .apply(&mut d)
        .unwrap();
    }
    d
}

/// A workspace with `d` open, no Jev key, and no coding CLI.
fn open(cx: &mut TestAppContext, d: Document) -> (Entity<Workspace>, &mut VisualTestContext) {
    open_with(cx, d, CliStatus::Missing)
}

fn open_with(
    cx: &mut TestAppContext,
    d: Document,
    cli: CliStatus,
) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        theme::install(cx);
        actions::bind(cx);
        cx.set_global(AppSettings(Settings {
            jev_api_key: None,
            ..Settings::default()
        }));
        cx.set_global(Capabilities { cli });
    });
    // SAFETY: tests run single-threaded per process for this variable.
    unsafe { std::env::remove_var("TYPESAFE_API_KEY") };
    let slot: Rc<RefCell<Option<Entity<Workspace>>>> = Rc::default();
    let s = slot.clone();
    let (_, vcx) = cx.add_window_view(move |window, cx| {
        let ws = cx.new(|cx| Workspace::new(window, cx));
        *s.borrow_mut() = Some(ws.clone());
        Root::new(ws, window, cx)
    });
    let ws = slot.borrow().clone().unwrap();
    vcx.update(|window, cx| {
        ws.update(cx, |w, cx| {
            w.install(d, None, None, None, "test".into(), window, cx)
        })
    });
    vcx.run_until_parked();
    (ws, vcx)
}

fn names(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> Vec<(String, bool)> {
    cx.update(|_, cx| {
        let e = ws.read(cx).editor.clone().unwrap();
        e.read(cx)
            .editor
            .doc
            .nodes
            .iter()
            .map(|n| (n.name.clone(), n.visible))
            .collect()
    })
}

#[gpui_kit::test]
fn ask_bar_plans_offline_applies_as_one_step_and_undoes(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, doc(&["Grass", "Sun", "Clouds"], None));
    cx.simulate_keystrokes("ctrl-k");
    cx.simulate_input("hide the top two nodes and rename the third to Sky");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();

    assert_eq!(
        names(&ws, cx),
        vec![
            ("Sky".into(), true),
            ("Sun".into(), false),
            ("Clouds".into(), false)
        ]
    );
    let (steps, dock) = cx.update(|_, cx| {
        let e = ws.read(cx).editor.clone().unwrap();
        let e = e.read(cx);
        (
            e.editor.history.len(),
            e.assistant.turn.as_ref().map(|t| (t.cards.len(), t.local)),
        )
    });
    assert_eq!(steps, 1, "one undo step for the whole request");
    assert_eq!(
        dock,
        Some((3, Some("keywords"))),
        "the dock lists three applied changes"
    );

    cx.simulate_keystrokes("ctrl-z");
    cx.run_until_parked();
    assert_eq!(
        names(&ws, cx),
        vec![
            ("Grass".into(), true),
            ("Sun".into(), true),
            ("Clouds".into(), true)
        ]
    );
}

#[gpui_kit::test]
fn escape_closes_the_ask_bar_and_shortcuts_keep_working(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, doc(&["Grass", "Sun"], None));
    cx.simulate_keystrokes("ctrl-k");
    cx.simulate_input("hide sun");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    let open = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap().read(cx).ask.is_some());
    assert!(!open, "escape closes the bar");
    assert_eq!(
        names(&ws, cx),
        vec![("Grass".into(), true), ("Sun".into(), true)],
        "nothing was submitted"
    );
    cx.simulate_keystrokes("ctrl-k");
    cx.simulate_input("hide sun");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(
        names(&ws, cx),
        vec![("Grass".into(), true), ("Sun".into(), false)]
    );
    cx.simulate_keystrokes("ctrl-z");
    cx.run_until_parked();
    assert_eq!(
        names(&ws, cx),
        vec![("Grass".into(), true), ("Sun".into(), true)],
        "undo works right after Enter"
    );
}

#[gpui_kit::test]
fn requests_that_need_the_assistant_say_so_when_it_is_missing(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, doc(&["Grass", "Sun"], None));
    cx.simulate_keystrokes("ctrl-k");
    cx.simulate_input("remove the person on the left");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let (status, steps) = cx.update(|_, cx| {
        let e = ws.read(cx).editor.clone().unwrap();
        let e = e.read(cx);
        (e.status.clone(), e.editor.history.len())
    });
    let (msg, err) = status.unwrap();
    assert!(err && msg.contains("Claude Code is not installed"), "{msg}");
    assert_eq!(steps, 0, "nothing changed");
}

#[gpui_kit::test]
fn suggestions_appear_and_accept_as_one_labelled_node(cx: &mut TestAppContext) {
    // A flat, dark, warm image: levels, shadows and white balance all apply.
    let (w, h) = (256u32, 192u32);
    let data: Vec<u8> = (0..h)
        .flat_map(|_| {
            (0..w).flat_map(|x| {
                [
                    (60 + x / 4) as u8,
                    (40 + x / 6) as u8,
                    (30 + x / 8) as u8,
                    255,
                ]
            })
        })
        .collect();
    let (ws, cx) = open(cx, doc(&["Photo"], Some(Raster::from_srgba8(w, h, &data))));
    cx.run_until_parked();
    let first = cx.update(|_, cx| {
        let e = ws.read(cx).editor.clone().unwrap();
        e.read(cx).suggestions.first().map(|s| s.node_name.clone())
    });
    let first = first.expect("suggestions for a flat, dark image");
    cx.simulate_keystrokes("alt-1");
    cx.run_until_parked();
    let (top, steps) = cx.update(|_, cx| {
        let e = ws.read(cx).editor.clone().unwrap();
        let e = e.read(cx);
        (
            e.editor.doc.nodes.last().unwrap().name.clone(),
            e.editor.history.len(),
        )
    });
    assert_eq!(top, first);
    assert_eq!(steps, 1);
}

/// Drives the installed Claude Code through the real UI: the Ask bar, the
/// relay inside GPUI, and the Apply button on each confirmation card.
#[gpui_kit::test]
#[ignore = "needs Claude Code and `cargo build -p emulsion-app`; uses a little Claude usage"]
fn assistant_turn_through_the_ui_with_the_real_cli(cx: &mut TestAppContext) {
    use crate::assistant::CardStatus;
    use gpui_kit::test::TestWindowExt;
    let exe = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/emulsion");
    assert!(
        exe.exists(),
        "build the app first: cargo build -p emulsion-app"
    );
    let Some(path) = emulsion_assistant::provider::find("claude", None) else {
        eprintln!("claude not installed; skipping");
        return;
    };
    // SAFETY: set before any thread reads it.
    unsafe { std::env::set_var("EMULSION_EXE", std::fs::canonicalize(&exe).unwrap()) };
    cx.executor().allow_parking();

    let mut d = Document::new(600, 400);
    for (name, c, (w, h, x, y)) in [
        ("Grass", [0.05, 0.4, 0.05, 1.0], (600, 150, 0.0, 250.0)),
        ("Sun", [0.9, 0.7, 0.1, 1.0], (120, 120, 440.0, 40.0)),
        ("Clouds", [0.8, 0.8, 0.85, 1.0], (300, 80, 0.0, 60.0)),
    ] {
        Command::AddNode {
            node: Box::new(Node::raster(
                0,
                name,
                Arc::new(Raster::solid(w, h, c)),
                Placement::at(x, y),
            )),
            slot: Slot::TOP,
        }
        .apply(&mut d)
        .unwrap();
    }
    let (ws, cx) = open_with(
        cx,
        d,
        CliStatus::Found {
            path,
            version: String::new(),
        },
    );
    cx.simulate_keystrokes("ctrl-k");
    cx.simulate_input("Look at the image, then hide whichever node is yellow.");
    cx.simulate_keystrokes("enter");

    let editor = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    let start = std::time::Instant::now();
    let mut applied = 0;
    loop {
        cx.run_until_parked();
        let (running, pending, done) = cx.update(|_, cx| {
            let a = &editor.read(cx).assistant;
            (
                a.running,
                a.turn.as_ref().map_or(0, |t| t.pending.len()),
                a.turn.as_ref().and_then(|t| t.done).is_some(),
            )
        });
        if pending > 0 {
            cx.update(|window, cx| window.click(("apply", 0usize), cx));
            applied += 1;
            continue;
        }
        if !running && done {
            break;
        }
        assert!(start.elapsed().as_secs() < 240, "turn timed out");
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let (vis, steps, tools, error, text) = cx.update(|_, cx| {
        let e = editor.read(cx);
        let t = e.assistant.turn.clone().unwrap();
        (
            e.editor
                .doc
                .nodes
                .iter()
                .map(|n| (n.name.clone(), n.visible))
                .collect::<Vec<_>>(),
            e.editor.history.len(),
            t.cards
                .iter()
                .map(|c| (c.tool.clone(), c.status.clone()))
                .collect::<Vec<_>>(),
            t.error,
            t.text,
        )
    });
    eprintln!("assistant said: {text}\ncards: {tools:?}\napplied {applied} confirmation(s)");
    assert_eq!(error, None);
    assert!(
        tools.iter().any(|(t, _)| t == "get_view"),
        "it looked at the image"
    );
    assert!(
        tools
            .iter()
            .any(|(t, s)| t == "set_visibility" && *s == CardStatus::Done)
    );
    assert!(applied >= 1, "the change went through a confirmation card");
    assert_eq!(
        vis,
        vec![
            ("Grass".into(), true),
            ("Sun".into(), false),
            ("Clouds".into(), true)
        ]
    );
    assert_eq!(steps, 1, "the whole turn is one undo step");
}

#[gpui_kit::test]
fn splash_dismisses_and_the_landing_image_opens_for_editing(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        theme::install(cx);
        actions::bind(cx);
        cx.set_global(AppSettings(Settings::default()));
        cx.set_global(Capabilities {
            cli: CliStatus::Missing,
        });
    });
    let slot: Rc<RefCell<Option<Entity<Workspace>>>> = Rc::default();
    let s = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let ws = cx.new(|cx| Workspace::new(window, cx));
        *s.borrow_mut() = Some(ws.clone());
        Root::new(ws, window, cx)
    });
    let ws = slot.borrow().clone().unwrap();
    cx.run_until_parked();
    assert!(
        cx.update(|_, cx| ws.read(cx).landing.is_some()),
        "landing image decoded"
    );
    assert!(
        cx.update(|_, cx| ws.read(cx).splash),
        "splash shows at launch"
    );
    cx.simulate_keystrokes("space");
    assert!(
        !cx.update(|_, cx| ws.read(cx).splash),
        "any key dismisses the splash"
    );

    cx.update(|window, cx| ws.update(cx, |w, cx| w.open_landing(window, cx)));
    cx.run_until_parked();
    let (name, size, nodes) = cx.update(|_, cx| {
        let e = ws
            .read(cx)
            .editor
            .clone()
            .expect("landing opened as a document");
        let e = e.read(cx);
        (
            e.name.clone(),
            (e.editor.doc.width, e.editor.doc.height),
            e.editor.doc.nodes.len(),
        )
    });
    assert_eq!((name.as_str(), size, nodes), ("landing", (1672, 941), 1));
}

// ── Phase 3 tools, driven through real pointer and key events ────────────

mod tools {
    use super::*;
    use crate::editor::{EditorView, Tool};
    use emulsion_core::NodeKind;
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{Pixels, Point};

    fn editor(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> Entity<EditorView> {
        cx.update(|_, cx| ws.read(cx).editor.clone().unwrap())
    }

    fn at(e: &Entity<EditorView>, cx: &mut VisualTestContext, d: (f64, f64)) -> Point<Pixels> {
        cx.update(|_, cx| e.read(cx).doc_to_window(d).expect("canvas laid out"))
    }

    fn drag(e: &Entity<EditorView>, cx: &mut VisualTestContext, a: (f64, f64), b: (f64, f64)) {
        let (a, b) = (at(e, cx, a), at(e, cx, b));
        cx.update(|window, cx| window.drag(a, b, cx));
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    fn move_tool_follows_the_pointer(cx: &mut TestAppContext) {
        let (_ws, e, cx) = setup(cx, Tool::Move);
        let id = cx.update(|_, cx| {
            let e = e.read(cx);
            e.editor.doc.nodes.last().unwrap().id
        });
        cx.update(|_, cx| e.update(cx, |e, _| e.selected = Some(id)));
        let a = at(&e, cx, (100.0, 80.0));
        let b = at(&e, cx, (130.0, 90.0));
        cx.simulate_mouse_down(a, gpui_kit::MouseButton::Left, gpui_kit::Modifiers::none());
        for i in 1..=10 {
            let t = i as f32 / 10.0;
            let p = gpui_kit::point(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
            cx.simulate_mouse_move(
                p,
                Some(gpui_kit::MouseButton::Left),
                gpui_kit::Modifiers::none(),
            );
            cx.run_until_parked();
        }
        cx.simulate_mouse_up(b, gpui_kit::MouseButton::Left, gpui_kit::Modifiers::none());
        cx.run_until_parked();
        let (x, y) = cx.update(
            |_, cx| match &e.read(cx).editor.doc.node(id).unwrap().kind {
                NodeKind::Raster { placement, .. } => (placement.x, placement.y),
                _ => unreachable!(),
            },
        );
        assert_eq!(
            (x, y),
            (30.0, 10.0),
            "the node moves exactly as far as the pointer"
        );
    }

    #[gpui_kit::test]
    fn retouch_on_a_branch_then_merge_through_the_history_page(cx: &mut TestAppContext) {
        use emulsion_core::Command;
        let (ws, cx) = open(cx, doc(&["Photo"], None));
        cx.run_until_parked();
        let e = editor(&ws, cx);
        let id = cx.update(|_, cx| e.read(cx).editor.doc.nodes[0].id);
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                e.create_branch("warm", None, cx);
                e.execute(Command::SetOpacity { id, opacity: 0.6 }, cx);
                e.open_history(cx);
            })
        });
        cx.run_until_parked();
        // The page renders with both branches and the compare pane.
        cx.update(|_, cx| {
            let e = e.read(cx);
            assert!(e.history.open);
            assert_eq!(e.editor.graph.head(), "warm");
            assert!(e.editor.differs_from_base());
        });
        cx.update(|_, cx| e.update(cx, |e, cx| e.switch_branch("main", cx)));
        cx.run_until_parked();
        assert_eq!(
            cx.update(|_, cx| e.read(cx).editor.doc.node(id).unwrap().opacity),
            1.0
        );
        cx.update(|_, cx| e.update(cx, |e, cx| e.merge_branch("warm", Default::default(), cx)));
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = e.read(cx);
            assert_eq!(e.editor.doc.node(id).unwrap().opacity, 0.6);
            assert!(e.history.merge.is_none());
            let last = e.editor.graph.commits().last().unwrap();
            assert_eq!(last.parents.len(), 2, "a merge commit");
        });
    }

    #[gpui_kit::test]
    fn drag_inside_a_selection_moves_it(cx: &mut TestAppContext) {
        let (_, e, cx) = setup(cx, Tool::Select);
        drag(&e, cx, (20.0, 20.0), (60.0, 50.0));
        drag(&e, cx, (40.0, 30.0), (70.0, 40.0));
        let b = cx.update(|_, cx| {
            let e = e.read(cx);
            emulsion_raster::select::bounds(e.editor.doc.selection.as_ref().unwrap())
        });
        assert!((b.x - 50).abs() <= 1 && (b.y - 30).abs() <= 1, "{b:?}");
        assert!((b.w - 40).abs() <= 1, "size unchanged: {b:?}");
    }

    #[gpui_kit::test]
    fn quick_select_and_magnetic_lasso(cx: &mut TestAppContext) {
        use crate::editor::SelectShape;
        // Left half dark, right half light.
        let (w, h) = (200u32, 120u32);
        let data: Vec<u8> = (0..w * h)
            .flat_map(|i| {
                if i % w < 100 {
                    [30, 30, 40, 255]
                } else {
                    [230, 225, 210, 255]
                }
            })
            .collect();
        let raster = emulsion_raster::Raster::from_srgba8(w, h, &data);
        let mut d = emulsion_core::Document::new(w, h);
        emulsion_core::Command::AddNode {
            node: Box::new(emulsion_core::Node::raster(
                0,
                "Photo",
                std::sync::Arc::new(raster),
                Default::default(),
            )),
            slot: emulsion_core::command::Slot::TOP,
        }
        .apply(&mut d)
        .unwrap();
        let (ws, cx) = open(cx, d);
        cx.run_until_parked();
        let e = editor(&ws, cx);
        cx.update(|_, cx| e.update(cx, |e, cx| e.set_select(SelectShape::Quick, cx)));
        drag(&e, cx, (20.0, 20.0), (40.0, 90.0));
        cx.run_until_parked();
        let b = cx.update(|_, cx| {
            emulsion_raster::select::bounds(e.read(cx).editor.doc.selection.as_ref().unwrap())
        });
        assert_eq!(
            (b.x, b.w),
            (0, 100),
            "quick select fills the dark half only: {b:?}"
        );

        // Magnetic: anchors near the edge, the outline hugs x = 100.
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                e.deselect(cx);
                e.set_select(SelectShape::Magnetic, cx);
            })
        });
        cx.run_until_parked();
        for p in [(99.0, 5.0), (99.0, 110.0), (5.0, 110.0), (5.0, 5.0)] {
            let at_p = at(&e, cx, p);
            cx.simulate_mouse_move(at_p, None, gpui_kit::Modifiers::none());
            cx.run_until_parked();
            cx.simulate_click(at_p, gpui_kit::Modifiers::none());
            cx.run_until_parked();
        }
        let first = at(&e, cx, (99.0, 5.0));
        cx.simulate_mouse_move(first, None, gpui_kit::Modifiers::none());
        cx.simulate_click(first, gpui_kit::Modifiers::none());
        cx.run_until_parked();
        let b = cx.update(|_, cx| {
            emulsion_raster::select::bounds(
                e.read(cx).editor.doc.selection.as_ref().expect("closed"),
            )
        });
        assert!(
            (b.right() - 100).abs() <= 2,
            "right edge snapped to the colour edge: {b:?}"
        );
    }

    #[gpui_kit::test]
    fn guides_from_the_ruler_and_snapping(cx: &mut TestAppContext) {
        let (_, e, cx) = setup(cx, Tool::Move);
        let id = cx.update(|_, cx| e.read(cx).editor.doc.nodes.last().unwrap().id);
        cx.update(|_, cx| e.update(cx, |e, _| e.selected = Some(id)));
        // Drag from the top ruler down to y = 50: a horizontal guide.
        let origin = cx.update(|_, cx| e.read(cx).canvas_origin().unwrap());
        let target = at(&e, cx, (100.0, 50.0));
        let ruler = gpui_kit::point(target.x, origin.y + gpui_kit::px(8.));
        cx.update(|window, cx| window.drag(ruler, target, cx));
        cx.run_until_parked();
        let guides = cx.update(|_, cx| e.read(cx).editor.doc.guides.clone());
        assert_eq!(guides.len(), 1);
        assert!(
            !guides[0].vertical && (guides[0].pos - 50.0).abs() <= 1.0,
            "{guides:?}"
        );
        // Move the node so its top edge stops two screen pixels short: it snaps.
        let zoom = cx.update(|_, cx| e.read(cx).view.zoom);
        let short = 50.0 - 2.0 / zoom;
        drag(&e, cx, (120.0, 60.0), (120.0, 60.0 + short));
        let y = cx.update(
            |_, cx| match &e.read(cx).editor.doc.node(id).unwrap().kind {
                NodeKind::Raster { placement, .. } => placement.y,
                _ => unreachable!(),
            },
        );
        assert_eq!(
            y,
            guides[0].pos.round(),
            "the top edge snapped to the guide"
        );
        // Dragging the guide back onto the ruler removes it.
        let on_guide = at(&e, cx, (300.0, guides[0].pos));
        let back = gpui_kit::point(on_guide.x, origin.y + gpui_kit::px(8.));
        cx.update(|window, cx| window.drag(on_guide, back, cx));
        cx.run_until_parked();
        assert!(cx.update(|_, cx| e.read(cx).editor.doc.guides.is_empty()));
    }

    #[gpui_kit::test]
    fn alt_crop_grows_from_the_centre_and_new_edges_fill(cx: &mut TestAppContext) {
        let (_, e, cx) = setup(cx, Tool::Crop);
        let (a, b) = (at(&e, cx, (100.0, 80.0)), at(&e, cx, (140.0, 100.0)));
        let alt = gpui_kit::Modifiers {
            alt: true,
            ..Default::default()
        };
        cx.simulate_mouse_down(a, gpui_kit::MouseButton::Left, alt);
        cx.simulate_mouse_move(b, Some(gpui_kit::MouseButton::Left), alt);
        cx.simulate_mouse_up(b, gpui_kit::MouseButton::Left, alt);
        cx.run_until_parked();
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        let size = cx.update(|_, cx| {
            let d = &e.read(cx).editor.doc;
            (d.width, d.height)
        });
        assert!(
            (size.0 as i32 - 80).abs() <= 1 && (size.1 as i32 - 40).abs() <= 1,
            "{size:?}"
        );

        // Extend the canvas by 20 px on every side and fill what was added.
        let (w, h) = size;
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                e.crop_canvas(
                    emulsion_raster::IRect::new(-20, -20, w as i32 + 40, h as i32 + 40),
                    0.0,
                    true,
                    cx,
                )
            })
        });
        cx.run_until_parked();
        let (names, alpha) = cx.update(|_, cx| {
            let d = &e.read(cx).editor.doc;
            let flat = emulsion_raster::composite::flatten(&d.composite_tree(), 0);
            let names: Vec<String> = d.nodes.iter().map(|n| n.name.clone()).collect();
            (names, flat.get(2, 2)[3])
        });
        assert_eq!(names.last().map(String::as_str), Some("Extended edges"));
        assert!(alpha > 60000, "the corner is filled, alpha {alpha}");
    }

    #[gpui_kit::test]
    fn free_transform_scale_rotate_and_distort(cx: &mut TestAppContext) {
        let (_, e, cx) = setup(cx, Tool::Move);
        let id = cx.update(|_, cx| e.read(cx).editor.doc.nodes.last().unwrap().id);
        cx.update(|_, cx| e.update(cx, |e, _| e.selected = Some(id)));
        cx.run_until_parked();
        let placement = |cx: &mut VisualTestContext| {
            cx.update(
                |_, cx| match &e.read(cx).editor.doc.node(id).unwrap().kind {
                    NodeKind::Raster { placement, raster } => {
                        (*placement, raster.width(), raster.height())
                    }
                    _ => unreachable!(),
                },
            )
        };
        // Bottom-right corner to the centre: half size, top-left fixed.
        drag(&e, cx, (256.0, 192.0), (128.0, 96.0));
        let (p, _, _) = placement(cx);
        assert!(
            (p.scale_x - 0.5).abs() < 0.02 && (p.scale_y - 0.5).abs() < 0.02,
            "{p:?}"
        );
        assert!(
            p.x.abs() < 0.5 && p.y.abs() < 0.5,
            "the opposite corner stays: {p:?}"
        );
        let steps = cx.update(|_, cx| e.read(cx).editor.history.len());

        // Just outside the top-right corner, swing a quarter turn about the centre.
        let zoom = cx.update(|_, cx| e.read(cx).view.zoom);
        let off = 12.0 / zoom;
        let (c, r) = ((64.0, 48.0), (128.0 + off, -off));
        let a0 = (r.1 - c.1).atan2(r.0 - c.0);
        let rad = (r.0 - c.0).hypot(r.1 - c.1);
        let to = (
            c.0 + rad * (a0 + std::f64::consts::FRAC_PI_2).cos(),
            c.1 + rad * (a0 + std::f64::consts::FRAC_PI_2).sin(),
        );
        drag(&e, cx, r, to);
        let (p, _, _) = placement(cx);
        assert!((p.rotation - 90.0).abs() < 2.0, "{p:?}");
        assert_eq!(
            cx.update(|_, cx| e.read(cx).editor.history.len()),
            steps + 1,
            "one step per drag"
        );

        // Ctrl-drag a corner: the pixels are re-projected into a new buffer.
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                e.execute(
                    emulsion_core::Command::SetPlacement {
                        id,
                        placement: Default::default(),
                    },
                    cx,
                )
            })
        });
        cx.run_until_parked();
        let (a, b) = (at(&e, cx, (256.0, 192.0)), at(&e, cx, (300.0, 230.0)));
        let ctrl = gpui_kit::Modifiers {
            control: true,
            ..Default::default()
        };
        cx.simulate_mouse_down(a, gpui_kit::MouseButton::Left, ctrl);
        cx.simulate_mouse_move(b, Some(gpui_kit::MouseButton::Left), ctrl);
        cx.simulate_mouse_up(b, gpui_kit::MouseButton::Left, ctrl);
        cx.run_until_parked();
        let (p, w, h) = placement(cx);
        assert_eq!((w, h), (300, 230), "the buffer grew to the distorted shape");
        assert!(p.x == 0.0 && p.y == 0.0 && p.rotation == 0.0);
    }

    #[gpui_kit::test]
    fn brush_presets_panel_applies_a_preset(cx: &mut TestAppContext) {
        let (_, e, cx) = setup(cx, Tool::Brush);
        cx.update(|_, cx| e.update(cx, |e, cx| e.toggle_presets(cx)));
        cx.run_until_parked();
        let chalk = emulsion_raster::library::find("Chalk").unwrap();
        cx.update(|_, cx| e.update(cx, |e, cx| e.apply_preset(&chalk, cx)));
        cx.run_until_parked();
        let b = cx.update(|_, cx| e.read(cx).brush());
        assert_eq!(
            (b.size, b.grain),
            (24.0, emulsion_raster::paint::GrainKind::Chalk)
        );
        // Picking an eraser switches the paint kind too.
        assert!(cx.update(|_, cx| e.update(cx, |e, cx| e.apply_preset_named("soft eraser", cx))));
        assert_eq!(
            cx.update(|_, cx| e.read(cx).paint_kind()),
            crate::editor::PaintKind::Eraser
        );
    }

    #[gpui_kit::test]
    fn smudge_drags_colour_and_mirror_paints_both_sides(cx: &mut TestAppContext) {
        use crate::editor::PaintKind;
        // Left half red, right half blue.
        let (w, h) = (256u32, 192u32);
        let data: Vec<u8> = (0..w * h)
            .flat_map(|i| {
                if i % w < 128 {
                    [255, 0, 0, 255]
                } else {
                    [0, 0, 255, 255]
                }
            })
            .collect();
        let raster = emulsion_raster::Raster::from_srgba8(w, h, &data);
        let (ws, cx) = open(cx, doc(&["Photo"], Some(raster)));
        cx.run_until_parked();
        let e = editor(&ws, cx);
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                e.set_paint(PaintKind::Smudge, cx);
                assert!(e.apply_preset_named("Smear", cx));
            })
        });
        drag(&e, cx, (120.0, 96.0), (170.0, 96.0));
        let px = cx.update(|_, cx| match &e.read(cx).editor.doc.nodes[0].kind {
            NodeKind::Raster { raster, .. } => raster.get(140, 96),
            _ => unreachable!(),
        });
        assert!(px[0] > 8000, "red smeared into the blue: {px:?}");

        // Mirrored ink lands on both halves.
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                e.set_paint(PaintKind::Brush, cx);
                e.apply_preset_named("Fine liner", cx);
                e.set_mirror(true, false, cx);
                e.set_fg([0, 255, 0, 255], cx);
            })
        });
        drag(&e, cx, (30.0, 20.0), (30.0, 170.0));
        let (a, b) = cx.update(|_, cx| match &e.read(cx).editor.doc.nodes[0].kind {
            NodeKind::Raster { raster, .. } => (raster.get(30, 100), raster.get(w - 30, 100)),
            _ => unreachable!(),
        });
        assert!(a[1] > 60000 && b[1] > 60000, "{a:?} {b:?}");
    }

    #[gpui_kit::test]
    fn pen_draws_edits_and_paints_along_a_path(cx: &mut TestAppContext) {
        use emulsion_core::NodeKind;
        let (_, e, cx) = setup(cx, Tool::Pen);
        let click = |cx: &mut VisualTestContext, d: (f64, f64)| {
            let p = at(&e, cx, d);
            cx.simulate_click(p, gpui_kit::Modifiers::none());
            cx.run_until_parked();
        };
        // Corner, curve (drag), corner, then click the first anchor to close.
        click(cx, (40.0, 40.0));
        drag(&e, cx, (200.0, 40.0), (230.0, 90.0));
        click(cx, (200.0, 150.0));
        click(cx, (40.0, 150.0));
        assert!(
            cx.update(|_, cx| e.read(cx).editor.doc.nodes.len()) == 1,
            "still building"
        );
        click(cx, (40.0, 40.0));
        let (id, anchors, closed, smooth) = cx.update(|_, cx| {
            let e = e.read(cx);
            let n = e.editor.doc.nodes.last().unwrap();
            match &n.kind {
                NodeKind::Path { path, .. } => (
                    n.id,
                    path.anchor_count(),
                    path.subpaths[0].closed,
                    path.subpaths[0].anchors[1].smooth,
                ),
                _ => panic!("expected a path node, got {:?}", n.kind.tag()),
            }
        });
        assert_eq!((anchors, closed, smooth), (4, true, true));
        // The stroke shows up in the composite along the bottom edge.
        let px = cx.update(|_, cx| {
            emulsion_raster::composite::flatten(&e.read(cx).editor.doc.composite_tree(), 0)
                .get(120, 150)
        });
        assert!(px[3] > 30000, "stroked edge is visible: {px:?}");

        // Drag the bottom-right anchor; the path follows and it is one undo step.
        let steps = cx.update(|_, cx| e.read(cx).editor.history.len());
        drag(&e, cx, (200.0, 150.0), (220.0, 170.0));
        let (p, steps2) = cx.update(|_, cx| {
            let e = e.read(cx);
            let NodeKind::Path { path, .. } = &e.editor.doc.node(id).unwrap().kind else {
                panic!()
            };
            (path.subpaths[0].anchors[2].p, e.editor.history.len())
        });
        assert!(
            (p.0 - 220.0).abs() < 1.0 && (p.1 - 170.0).abs() < 1.0,
            "{p:?}"
        );
        assert_eq!(steps2, steps + 1);

        // Paint along the path onto the photo with the current brush.
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                e.set_fg([0, 255, 0, 255], cx);
                e.pen_paint_along(cx);
            })
        });
        cx.run_until_parked();
        // It paints on a new layer above the path (the path itself is not pixels);
        // the curved top edge bows up to y ≈ 21.
        let px = cx.update(|_, cx| {
            emulsion_raster::composite::flatten(&e.read(cx).editor.doc.composite_tree(), 0)
                .get(120, 22)
        });
        assert!(px[1] > 40000, "green paint along the top edge: {px:?}");
    }

    #[gpui_kit::test]
    fn assistant_strokes_play_back_live(cx: &mut TestAppContext) {
        use emulsion_core::NodeKind;
        let (ws, cx) = open(cx, doc(&["Photo"], None));
        cx.run_until_parked();
        let e = editor(&ws, cx);
        let id = cx.update(|_, cx| e.read(cx).editor.doc.nodes[0].id);
        let script = cx.update(|_, cx| {
            emulsion_mcp::exec::paint_script(
                &e.read(cx).editor.doc,
                &serde_json::json!({ "node": id, "brush": "Fine liner", "color": "#ff0000", "strokes": [{ "points": [[10, 96], [246, 96]] }] }),
            )
            .unwrap()
        });
        let steps = cx.update(|_, cx| e.read(cx).editor.history.len());
        cx.update(|_, cx| e.update(cx, |e, cx| e.start_playback(None, script, cx)));
        let mut saw_ghost = false;
        for _ in 0..600 {
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(16));
            cx.run_until_parked();
            let (ghost, done) = cx.update(|_, cx| {
                let e = e.read(cx);
                (e.ghost_brush().is_some(), e.assistant.playback.is_none())
            });
            saw_ghost |= ghost;
            eprintln!();
            if done {
                break;
            }
        }
        cx.update(|_, cx| {
            let e = e.read(cx);
            assert!(e.assistant.playback.is_none(), "playback finished");
            assert!(saw_ghost, "the ghost brush was visible while painting");
            assert_eq!(
                e.editor.history.len(),
                steps + 1,
                "one undo step for the whole call"
            );
            let NodeKind::Raster { raster, .. } = &e.editor.doc.node(id).unwrap().kind else {
                panic!()
            };
            assert!(raster.get(128, 96)[0] > 60000, "the line is on the layer");
        });
    }

    #[gpui_kit::test]
    fn curves_editor_adds_and_drags_points(cx: &mut TestAppContext) {
        use emulsion_core::NodeKind;
        use emulsion_raster::Adjustment;
        let (ws, cx) = open(cx, doc(&["Photo"], None));
        cx.run_until_parked();
        let e = editor(&ws, cx);
        let curves = Adjustment::catalogue()
            .into_iter()
            .find(|a| matches!(a, Adjustment::Curves { .. }))
            .unwrap();
        let id = cx
            .update(|_, cx| {
                e.update(cx, |e, cx| {
                    e.execute(
                        emulsion_core::Command::AddNode {
                            node: Box::new(emulsion_core::Node::adjust(0, curves)),
                            slot: emulsion_core::command::Slot::TOP,
                        },
                        cx,
                    )
                })
            })
            .unwrap();
        cx.update(|_, cx| e.update(cx, |e, _| e.selected = Some(id)));
        cx.run_until_parked();
        // The editor square records its bounds during layout.
        let bounds =
            cx.update(|_, cx| e.read(cx).curve_bounds(id).expect("curves editor laid out"));
        let at = |fx: f32, fy: f32| {
            bounds.origin + gpui_kit::point(bounds.size.width * fx, bounds.size.height * (1.0 - fy))
        };
        // Click the middle of the line to add a point, then drag it up.
        cx.update(|window, cx| window.drag(at(0.5, 0.5), at(0.5, 0.75), cx));
        cx.run_until_parked();
        let (pts, steps) = cx.update(|_, cx| {
            let e = e.read(cx);
            let NodeKind::Adjust(Adjustment::Curves { master, .. }) =
                &e.editor.doc.node(id).unwrap().kind
            else {
                panic!()
            };
            (master.clone(), e.editor.history.len())
        });
        assert_eq!(pts.len(), 3, "{pts:?}");
        assert!(
            (pts[1][0] - 127.5).abs() < 6.0 && pts[1][1] > 170.0,
            "{pts:?}"
        );
        assert_eq!(steps, 2, "add node, then one curves step");
    }

    #[gpui_kit::test]
    fn recipes_panel_applies_a_recipe_as_one_step(cx: &mut TestAppContext) {
        let (ws, cx) = open(cx, doc(&["Photo"], None));
        cx.run_until_parked();
        let e = editor(&ws, cx);
        let recipe = emulsion_recipes::starter_set()
            .into_iter()
            .find(|r| r.name == "Slide Punch")
            .unwrap();
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                e.toggle_recipes(cx);
                e.apply_recipe(&recipe, cx);
            })
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = e.read(cx);
            let g = e
                .editor
                .doc
                .nodes
                .iter()
                .find(|n| n.is_group())
                .expect("recipe group");
            assert!(g.name.contains("Slide Punch"));
            assert!(e.editor.doc.children(Some(g.id)).len() >= 4);
            assert_eq!(e.editor.history.len(), 1, "one undo step");
            assert_eq!(e.selected, Some(g.id));
        });
    }

    #[gpui_kit::test]
    fn the_default_hand_tool_pans_without_moving_pixels(cx: &mut TestAppContext) {
        let (ws, cx) = open(cx, doc(&["Photo"], None));
        cx.run_until_parked();
        let e = editor(&ws, cx);
        assert_eq!(cx.update(|_, cx| e.read(cx).tool), Tool::Hand);
        let before = cx.update(|_, cx| e.read(cx).editor.doc.clone());
        let a = at(&e, cx, (100.0, 80.0));
        let moved = at(&e, cx, (100.0, 80.0));
        drag(&e, cx, (100.0, 80.0), (140.0, 100.0));
        let after = at(&e, cx, (100.0, 80.0));
        assert!(
            cx.update(|_, cx| e.read(cx).editor.doc == before),
            "pixels stay put"
        );
        assert!(after != a && a == moved, "the view moved under the pointer");
    }

    fn setup(
        cx: &mut TestAppContext,
        tool: Tool,
    ) -> (
        Entity<Workspace>,
        Entity<EditorView>,
        &mut VisualTestContext,
    ) {
        let (ws, cx) = open(cx, doc(&["Photo"], None));
        cx.run_until_parked();
        let e = editor(&ws, cx);
        cx.update(|_, cx| e.update(cx, |e, cx| e.set_tool(tool, cx)));
        cx.run_until_parked();
        (ws, e, cx)
    }

    #[gpui_kit::test]
    fn marquee_drag_selects_and_undoes(cx: &mut TestAppContext) {
        let (_, e, cx) = setup(cx, Tool::Select);
        drag(&e, cx, (20.0, 20.0), (120.0, 80.0));
        let b = cx.update(|_, cx| {
            e.read(cx)
                .editor
                .doc
                .selection
                .as_deref()
                .map(emulsion_raster::select::bounds)
        });
        let b = b.expect("a selection");
        assert!(
            (b.x - 20).abs() <= 1 && (b.w - 100).abs() <= 2 && (b.h - 60).abs() <= 2,
            "{b:?}"
        );
        cx.simulate_keystrokes("ctrl-z");
        assert!(cx.update(|_, cx| e.read(cx).editor.doc.selection.is_none()));
    }

    #[gpui_kit::test]
    fn brush_stroke_paints_inside_the_selection_only(cx: &mut TestAppContext) {
        let (_, e, cx) = setup(cx, Tool::Select);
        drag(&e, cx, (0.0, 0.0), (128.0, 192.0)); // left half
        cx.update(|_, cx| e.update(cx, |e, cx| e.set_tool(Tool::Brush, cx)));
        drag(&e, cx, (40.0, 96.0), (220.0, 96.0));
        let (inside, outside, steps) = cx.update(|_, cx| {
            let e = e.read(cx);
            let NodeKind::Raster { raster, .. } = &e.editor.doc.nodes[0].kind else {
                panic!()
            };
            (
                raster.get(80, 96),
                raster.get(200, 96),
                e.editor.history.len(),
            )
        });
        let base = emulsion_raster::color::f_to_px([0.2, 0.3, 0.4, 1.0]);
        assert_ne!(inside, base, "painted inside the selection");
        assert_eq!(outside, base, "untouched outside it");
        assert_eq!(
            steps, 2,
            "the selection, then one step for the whole stroke"
        );
    }

    #[gpui_kit::test]
    fn crop_drag_then_enter_resizes_without_resampling(cx: &mut TestAppContext) {
        let (_, e, cx) = setup(cx, Tool::Crop);
        drag(&e, cx, (50.0, 40.0), (150.0, 140.0));
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        let (w, h, x) = cx.update(|_, cx| {
            let d = &e.read(cx).editor.doc;
            let NodeKind::Raster { placement, .. } = &d.nodes[0].kind else {
                panic!()
            };
            (d.width, d.height, placement.x)
        });
        assert!(
            (w as i32 - 100).abs() <= 1 && (h as i32 - 100).abs() <= 1,
            "{w}×{h}"
        );
        assert!(
            (x + 50.0).abs() <= 1.0,
            "layer moved, not resampled: x = {x}"
        );
    }

    #[gpui_kit::test]
    fn shape_drag_adds_a_masked_fill_node(cx: &mut TestAppContext) {
        let (_, e, cx) = setup(cx, Tool::Shape);
        drag(&e, cx, (30.0, 30.0), (90.0, 70.0));
        let (kind, masked) = cx.update(|_, cx| {
            let n = e.read(cx).editor.doc.nodes.last().unwrap().clone();
            (n.name.clone(), n.mask.is_some())
        });
        assert_eq!((kind.as_str(), masked), ("Rectangle", true));
    }

    #[gpui_kit::test]
    fn content_aware_fill_adds_a_node_from_the_selection(cx: &mut TestAppContext) {
        let (_, e, cx) = setup(cx, Tool::Select);
        drag(&e, cx, (100.0, 80.0), (140.0, 110.0)); // selects and focuses the canvas
        cx.simulate_keystrokes("shift-backspace");
        cx.run_until_parked();
        let top = cx.update(|_, cx| e.read(cx).editor.doc.nodes.last().unwrap().name.clone());
        assert_eq!(top, "Content-aware fill");
    }

    #[gpui_kit::test]
    fn tool_keys_switch_tools_when_the_canvas_has_focus(cx: &mut TestAppContext) {
        let (_, e, cx) = setup(cx, Tool::Move);
        let p = at(&e, cx, (10.0, 10.0));
        cx.update(|window, cx| window.drag(p, p, cx));
        for (key, want) in [
            ("b", Tool::Brush),
            ("m", Tool::Select),
            ("c", Tool::Crop),
            ("j", Tool::Heal),
            ("v", Tool::Move),
            ("h", Tool::Hand),
        ] {
            cx.simulate_keystrokes(key);
            assert_eq!(cx.update(|_, cx| e.read(cx).tool), want, "key {key}");
        }
    }

    #[gpui_kit::test]
    fn wand_gradient_and_heal(cx: &mut TestAppContext) {
        // Left half red, right half blue; a white speck on the red.
        let (w, h) = (256u32, 192u32);
        let data: Vec<u8> = (0..h)
            .flat_map(|y| {
                (0..w).flat_map(move |x| {
                    if (60..64).contains(&x) && (90..94).contains(&y) {
                        [255, 255, 255, 255]
                    } else if x < 128 {
                        [200, 30, 30, 255]
                    } else {
                        [30, 30, 200, 255]
                    }
                })
            })
            .collect();
        let (ws, cx) = open(
            cx,
            doc(
                &["Photo"],
                Some(emulsion_raster::Raster::from_srgba8(w, h, &data)),
            ),
        );
        cx.run_until_parked();
        let e = editor(&ws, cx);

        // Wand on the blue half selects exactly it.
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                e.set_select(crate::editor::SelectShape::Wand, cx)
            })
        });
        drag(&e, cx, (200.0, 20.0), (200.0, 20.0));
        let b = cx
            .update(|_, cx| {
                e.read(cx)
                    .editor
                    .doc
                    .selection
                    .as_deref()
                    .map(emulsion_raster::select::bounds)
            })
            .expect("wand selection");
        assert_eq!((b.x, b.w), (128, 128));

        // A gradient drag fills only the selection, in a new node.
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                e.set_paint(crate::editor::PaintKind::Gradient, cx)
            })
        });
        drag(&e, cx, (130.0, 96.0), (250.0, 96.0));
        let (name, left_alpha, right_alpha) = cx.update(|_, cx| {
            let n = e.read(cx).editor.doc.nodes.last().unwrap().clone();
            let NodeKind::Raster { raster, .. } = &n.kind else {
                panic!()
            };
            (
                n.name.clone(),
                raster.get(20, 96)[3],
                raster.get(200, 96)[3],
            )
        });
        assert_eq!(name, "Gradient");
        assert_eq!(left_alpha, 0, "outside the selection");
        assert!(right_alpha > 60000, "inside it");

        // Spot heal over the white speck on the red layer brings back red.
        cx.simulate_keystrokes("ctrl-d");
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                let id = e.editor.doc.nodes[0].id;
                e.selected = Some(id);
                e.set_tool(Tool::Heal, cx)
            })
        });
        drag(&e, cx, (58.0, 92.0), (66.0, 92.0));
        for _ in 0..50 {
            cx.run_until_parked();
            if !cx.update(|_, cx| e.read(cx).editor.in_transaction()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let px = cx.update(|_, cx| {
            let NodeKind::Raster { raster, .. } = &e.read(cx).editor.doc.nodes[0].kind else {
                panic!()
            };
            emulsion_raster::color::premul_to_srgba8(emulsion_raster::color::px_to_f(
                raster.get(61, 91),
            ))
        });
        assert!(px[0] > 150 && px[1] < 90, "healed to red, got {px:?}");
    }
}
