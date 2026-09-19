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
            w.install(d, None, None, "test".into(), window, cx)
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
