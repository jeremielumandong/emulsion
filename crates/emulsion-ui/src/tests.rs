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

#[path = "editor_layout_tests.rs"]
mod editor_layout_tests;

#[path = "tool_usability_tests.rs"]
mod tool_usability_tests;

#[path = "navigation_functionality_tests.rs"]
mod navigation_functionality_tests;

#[path = "paint_functionality_tests.rs"]
mod paint_functionality_tests;

#[path = "geometry_functionality_tests.rs"]
mod geometry_functionality_tests;

#[path = "clipboard_tests.rs"]
mod clipboard_tests;

#[path = "crop_workflow_tests.rs"]
mod crop_workflow_tests;

#[path = "transform_workflow_tests.rs"]
mod transform_workflow_tests;

#[path = "context_menu_tests.rs"]
mod context_menu_tests;

#[path = "blending_workflow_tests.rs"]
mod blending_workflow_tests;

#[path = "versioning_workflow_tests.rs"]
mod versioning_workflow_tests;

#[path = "text_color_workflow_tests.rs"]
mod text_color_workflow_tests;

#[path = "on_canvas_text_tests.rs"]
mod on_canvas_text_tests;

#[path = "pen_workflow_tests.rs"]
mod pen_workflow_tests;

#[path = "channel_workflow_tests.rs"]
mod channel_workflow_tests;

#[path = "filter_menu_tests.rs"]
mod filter_menu_tests;

#[path = "tool_safety_tests.rs"]
mod tool_safety_tests;

#[path = "painting_tests.rs"]
mod painting_tests;

#[path = "layer_tests.rs"]
mod layer_tests;

#[path = "layer_selection_tests.rs"]
mod layer_selection_tests;

#[path = "shape_color_workflow_tests.rs"]
mod shape_color_workflow_tests;
#[path = "shape_fill_tests.rs"]
mod shape_fill_tests;
#[path = "shape_vector_fill_tests.rs"]
mod shape_vector_fill_tests;
#[path = "shape_workflow_tests.rs"]
mod shape_workflow_tests;

#[path = "layer_effect_rows_tests.rs"]
mod layer_effect_rows_tests;
#[path = "layer_panel_workflow_tests.rs"]
mod layer_panel_workflow_tests;

#[path = "layer_menu_tests.rs"]
mod layer_menu_tests;

#[path = "advanced_style_workflow_tests.rs"]
mod advanced_style_workflow_tests;
#[path = "mask_style_workflow_tests.rs"]
mod mask_style_workflow_tests;

#[path = "filter_gesture_tests.rs"]
mod filter_gesture_tests;

#[path = "movement_tests.rs"]
mod movement_tests;

#[path = "workflow_recipe_tests.rs"]
mod workflow_recipe_tests;

#[path = "alignment_tests.rs"]
mod alignment_tests;

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
        cx.set_reduce_motion(true);
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
fn long_multiline_status_preserves_canvas_and_footer_layout(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt;

    let (ws, cx) = open(cx, doc(&["Photo"], None));
    let e = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    cx.update(|_, cx| e.update(cx, |e, cx| e.set_status("Ready", false, cx)));
    cx.run_until_parked();
    let (strip, canvas_origin, canvas_corner) = cx.update(|window, cx| {
        let e = e.read(cx);
        (
            window.find("editor-status-strip").bounds(),
            e.doc_to_window((0.0, 0.0)).unwrap(),
            e.doc_to_window((256.0, 192.0)).unwrap(),
        )
    });
    let diagnostic = format!(
        "Generate: HTTP 429\n{}\nhttps://example.invalid/{}",
        "Quota exceeded; enable billing and retry.\n".repeat(30),
        "long-provider-diagnostic".repeat(60)
    );
    cx.update(|_, cx| e.update(cx, |e, cx| e.set_status(diagnostic.clone(), true, cx)));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert_eq!(window.find("editor-status-strip").bounds(), strip);
        let message = window.find("editor-status-message");
        let meta = window.find("editor-status-meta");
        assert!(message.visible() && meta.visible());
        assert!(message.bounds().size.width > gpui_kit::px(0.));
        assert!(message.bounds().size.height <= gpui_kit::px(16.));
        assert!(message.bounds().right() <= meta.bounds().left());
        assert!(meta.bounds().right() <= strip.right());
        let e = e.read(cx);
        assert_eq!(e.doc_to_window((0.0, 0.0)).unwrap(), canvas_origin);
        assert_eq!(e.doc_to_window((256.0, 192.0)).unwrap(), canvas_corner);
        assert_eq!(
            e.status.as_ref().unwrap().0.as_ref(),
            diagnostic.as_str(),
            "full diagnostic remains available to the tooltip"
        );
    });
}

#[gpui_kit::test]
fn ask_image_choices_preserve_prompt_and_busy_requests_do_not_edit(cx: &mut TestAppContext) {
    use emulsion_ai::generate::Provider;
    use gpui_kit::test::TestWindowExt;

    let (ws, cx) = open(cx, doc(&["Photo"], None));
    let e = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    let before = cx.update(|_, cx| e.read(cx).editor.doc.clone());
    cx.simulate_keystrokes("ctrl-k");
    cx.simulate_input("hide Photo");
    cx.run_until_parked();
    for (id, choice) in [
        ("ask-local", Some(Provider::A1111)),
        ("ask-openai", Some(Provider::OpenAi)),
        ("ask-google", Some(Provider::Google)),
        ("ask-assistant", None),
        ("ask-openai", Some(Provider::OpenAi)),
    ] {
        cx.update(|window, cx| window.click(id, cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            let e = e.read(cx);
            assert_eq!(e.generate.ask_provider, choice);
            assert_eq!(
                e.ask.as_ref().unwrap().state.read(cx).value().as_str(),
                "hide Photo"
            );
            assert_eq!(window.try_find("ask-reference").is_some(), choice.is_none());
        });
    }
    // A busy job rejects the request before any provider can be contacted,
    // even when the machine running this test has cloud credentials.
    cx.update(|_, cx| e.update(cx, |e, _| e.generate.busy = true));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    cx.update(|_, cx| {
        let e = e.read(cx);
        assert!(e.status.as_ref().unwrap().0.contains("Still generating"));
        assert_eq!(
            e.ask.as_ref().unwrap().state.read(cx).value().as_str(),
            "hide Photo"
        );
        assert_eq!(e.editor.doc, before);
        assert_eq!(e.editor.history.len(), 0);
        assert!(e.assistant.turn.is_none());
        assert!(!e.assistant.running);
    });
    cx.simulate_keystrokes("escape");
    cx.simulate_keystrokes("ctrl-k");
    cx.run_until_parked();
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            assert_eq!(e.generate.ask_provider, Some(Provider::OpenAi));
            e.submit_ask("hide Photo".into(), cx);
            assert!(
                e.status
                    .as_ref()
                    .unwrap()
                    .0
                    .contains("Wait for image generation")
            );
            assert_eq!(e.editor.doc, before);
            assert_eq!(e.editor.history.len(), 0);
            e.generate.busy = false;
        });
    });
}

#[gpui_kit::test]
fn image_generation_validates_credentials_and_excludes_active_edits(cx: &mut TestAppContext) {
    use emulsion_ai::generate::{Config, Provider};

    let (ws, cx) = open(cx, doc(&["Photo"], None));
    cx.update(|_, cx| {
        let e = ws.read(cx).editor.clone().unwrap();
        e.update(cx, |e, cx| {
            let before = e.editor.doc.clone();
            // Explicit empty credentials keep this test independent of saved
            // keys and environment variables. Neither request reaches a server.
            for provider in [Provider::OpenAi, Provider::Google] {
                let cfg = Config {
                    provider,
                    endpoint: None,
                    model: None,
                    api_key: None,
                };
                let error = e.generate_text("hide Photo".into(), cfg, cx).unwrap_err();
                assert!(error.contains("API key"));
                assert!(!e.generate.busy);
                assert!(e.ai.job.is_none());
            }
            let local = Config {
                provider: Provider::A1111,
                endpoint: None,
                model: None,
                api_key: None,
            };
            e.assistant.running = true;
            assert!(
                e.generate_text("a landscape".into(), local.clone(), cx)
                    .unwrap_err()
                    .contains("current edit")
            );
            e.assistant.running = false;
            e.editor.begin("in-progress edit");
            assert!(
                e.generate_text("a landscape".into(), local, cx)
                    .unwrap_err()
                    .contains("current edit")
            );
            e.editor.end();
            assert_eq!(e.editor.doc, before);
            assert_eq!(e.editor.history.len(), 0);
            assert!(!e.generate.busy);
            assert!(e.assistant.turn.is_none());
        });
    });
}

mod generated_images {
    use super::*;
    use emulsion_ai::generate::{Config, Provider};
    use emulsion_core::NodeKind;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::time::{Duration, Instant};

    /// One local A1111 response containing a red 1×1 PNG. All socket waits
    /// have deadlines; no saved credentials or remote endpoints are used.
    fn image_server() -> (Config, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("fixture server did not receive a request: {error}"),
                }
            };
            // macOS can inherit the listener's nonblocking flag on accept.
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            socket
                .set_write_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut reader = BufReader::new(socket.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            assert!(line.starts_with("POST /sdapi/v1/txt2img "));
            let mut length = 0;
            loop {
                line.clear();
                assert!(reader.read_line(&mut line).unwrap() > 0);
                if line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap();
                }
            }
            assert!(length > 0 && length < 100_000);
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(body["prompt"], "a red square");
            let response = r#"{"images":["iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg=="]}"#;
            write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).unwrap();
        });
        (
            Config {
                provider: Provider::A1111,
                endpoint: Some(endpoint),
                model: Some("test-fixture".into()),
                api_key: None,
            },
            server,
        )
    }

    #[gpui_kit::test]
    fn generated_image_is_a_separate_layer_with_provenance_and_undo(cx: &mut TestAppContext) {
        let (ws, cx) = open(cx, doc(&["Photo"], None));
        // Start the request deadline after potentially slow window initialization.
        let (cfg, server) = image_server();
        let e = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
        let before = cx.update(|_, cx| e.read(cx).editor.doc.clone());
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                e.generate_text("a red square".into(), cfg, cx).unwrap()
            })
        });
        cx.run_until_parked();
        server.join().unwrap();
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                assert!(!e.generate.busy);
                assert_eq!(e.editor.history.len(), 1);
                assert_eq!(e.editor.doc.nodes.len(), before.nodes.len() + 1);
                assert_eq!(
                    e.editor.doc.node(before.nodes[0].id),
                    before.node(before.nodes[0].id)
                );
                let node = e.editor.doc.node(e.selected.unwrap()).unwrap();
                assert_eq!(node.origin.as_deref(), Some("ai:a1111/test-fixture"));
                let NodeKind::Raster { raster, .. } = &node.kind else {
                    panic!("generated raster layer");
                };
                assert_eq!(
                    (raster.width(), raster.height()),
                    (before.width, before.height)
                );
                assert_eq!(raster.get(128, 96), [65535, 0, 0, 65535]);
                e.undo(cx);
                assert_eq!(e.editor.doc, before);
            })
        });
    }

    #[gpui_kit::test]
    fn generated_image_waiting_for_an_edit_can_still_be_cancelled(cx: &mut TestAppContext) {
        let (ws, cx) = open(cx, doc(&["Photo"], None));
        // Start the request deadline after potentially slow window initialization.
        let (cfg, server) = image_server();
        let e = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
        let before = cx.update(|_, cx| e.read(cx).editor.doc.clone());
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                e.generate_text("a red square".into(), cfg, cx).unwrap();
                // A user edit starts while the provider is working.
                e.editor.begin("ongoing brush edit");
            })
        });
        cx.run_until_parked();
        server.join().unwrap();
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                assert!(e.generate.busy);
                assert!(
                    !e.ai
                        .job
                        .as_ref()
                        .expect("cancel remains available")
                        .is_finished()
                );
                assert_eq!(e.editor.doc, before);
                e.cancel_ai(cx);
                e.editor.end();
            })
        });
        cx.executor().advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = e.read(cx);
            assert!(!e.generate.busy);
            assert_eq!(e.editor.doc, before);
            assert_eq!(e.editor.history.len(), 0);
            assert!(e.status.as_ref().unwrap().0.contains("cancelled"));
        });
    }
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

mod reference_images {
    use super::*;
    use crate::editor::EditorView;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Fixture(PathBuf);

    impl Fixture {
        fn new(bytes: &[u8]) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "emulsion-ui-reference-{}-{}.png",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::write(&path, bytes).unwrap();
            Self(path)
        }

        fn image(width: u32, height: u32) -> Self {
            let pixels = image::RgbaImage::from_pixel(width, height, image::Rgba([255, 0, 0, 255]));
            Self::new(&emulsion_io::export::png8(width, height, pixels.as_raw()).unwrap())
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn load(view: &Entity<EditorView>, path: PathBuf, cx: &mut VisualTestContext) {
        cx.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.load_reference(path, cx);
                assert!(view.assistant.reference_loading);
            });
        });
        cx.run_until_parked();
        cx.update(|_, cx| assert!(!view.read(cx).assistant.reference_loading));
    }

    #[gpui_kit::test]
    fn reference_attachment_preserves_canvas_and_survives_failed_replacement(
        cx: &mut TestAppContext,
    ) {
        let source = Fixture::image(40, 20);
        let invalid = Fixture::new(b"This is not an image.");
        let (ws, cx) = open(cx, doc(&["Grass", "Sun"], None));
        let view = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
        let (before, revision, steps) = cx.update(|_, cx| {
            let view = view.read(cx);
            assert!(view.reference_result().is_error);
            (
                view.editor.doc.clone(),
                view.editor.revision,
                view.editor.history.len(),
            )
        });
        load(&view, source.0.clone(), cx);
        let attached = cx.update(|_, cx| {
            let view = view.read(cx);
            let reference = &view
                .assistant
                .reference
                .as_ref()
                .expect("reference loaded")
                .image;
            assert_eq!((reference.width(), reference.height()), (40, 20));
            let pixels = image::load_from_memory(reference.png()).unwrap().to_rgba8();
            assert_eq!(pixels.dimensions(), (40, 20));
            assert_eq!(pixels.get_pixel(20, 10).0, [255, 0, 0, 255]);
            let result = view.reference_result();
            assert!(!result.is_error);
            assert_eq!(result.content[0]["type"], "image");
            assert_eq!(result.content[0]["mimeType"], "image/png");
            assert!(!result.content[0]["data"].as_str().unwrap().is_empty());
            assert_eq!(result.content, reference.tool_result().content);
            assert_eq!(view.editor.doc, before);
            assert_eq!(view.editor.revision, revision);
            assert_eq!(view.editor.history.len(), steps);
            reference.clone()
        });

        for path in [source.0.with_extension("missing"), invalid.0.clone()] {
            load(&view, path, cx);
            cx.update(|_, cx| {
                let view = view.read(cx);
                assert!(Arc::ptr_eq(
                    &view.assistant.reference.as_ref().unwrap().image,
                    &attached
                ));
                assert!(
                    !view.reference_result().is_error,
                    "the previous image remains available"
                );
                let (message, error) = view.status.as_ref().unwrap();
                assert!(
                    *error && message.contains("Could not load reference"),
                    "{message}"
                );
                assert_eq!(view.editor.doc, before);
                assert_eq!(view.editor.revision, revision);
                assert_eq!(view.editor.history.len(), steps);
            });
        }

        cx.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.remove_reference(cx);
                assert!(view.assistant.reference.is_none());
                assert!(view.reference_result().is_error);
                assert_eq!(view.editor.doc, before);
                assert_eq!(view.editor.revision, revision);
                assert_eq!(view.editor.history.len(), steps);
            });
        });
    }

    #[gpui_kit::test]
    fn reference_is_fixed_during_a_turn_and_prompts_bypass_keyword_edits(cx: &mut TestAppContext) {
        let source = Fixture::image(40, 20);
        let replacement = Fixture::image(20, 40);
        let (ws, cx) = open(cx, doc(&["Grass", "Sun"], None));
        let view = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
        load(&view, source.0.clone(), cx);
        let (before, revision, steps) = cx.update(|_, cx| {
            view.update(cx, |view, cx| {
                let attached = view.assistant.reference.as_ref().unwrap().image.clone();
                view.assistant.running = true;
                view.load_reference(replacement.0.clone(), cx);
                view.remove_reference(cx);
                assert!(
                    !view.assistant.reference_loading,
                    "replacement must not even start"
                );
                assert!(Arc::ptr_eq(
                    &view.assistant.reference.as_ref().unwrap().image,
                    &attached
                ));
                view.assistant.running = false;
                let snapshot = (
                    view.editor.doc.clone(),
                    view.editor.revision,
                    view.editor.history.len(),
                );
                view.submit_ask("hide sun".into(), cx);
                snapshot
            })
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            let view = view.read(cx);
            let (message, error) = view.status.as_ref().unwrap();
            assert!(
                *error && message.contains("Claude Code is not installed"),
                "{message}"
            );
            assert!(!view.assistant.running);
            assert!(view.assistant.reference.is_some());
            assert_eq!(
                view.editor.doc, before,
                "the reference request must not hide Sun through keyword fallback"
            );
            assert_eq!(view.editor.revision, revision);
            assert_eq!(view.editor.history.len(), steps);
        });
    }
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
    assert_eq!((name.as_str(), size, nodes), ("landing", (1586, 992), 1));
}

// ── Phase 3 tools, driven through real pointer and key events ────────────

#[gpui_kit::test]
fn batch_recipe_browser_preserves_photo_selection_and_export_settings(cx: &mut TestAppContext) {
    use crate::batch::BatchItem;
    use crate::workspace::Screen;
    use emulsion_recipes::Recipe;
    use gpui_kit::test::TestWindowExt;

    let (ws, cx) = open(cx, doc(&["Photo"], None));
    let source =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/landing/landing.jpg");
    let out_dir = std::env::temp_dir().join("emulsion-batch-layout-export-unused");
    cx.update(|_, cx| {
        ws.update(cx, |ws, cx| {
            let thumb = Arc::new(crate::viewport::bgra_image(1, 1, vec![80, 80, 80, 255]));
            ws.batch.items = [true, false, true]
                .into_iter()
                .map(|selected| BatchItem {
                    path: source.clone(),
                    selected,
                    thumb: Some(thumb.clone()),
                })
                .collect();
            ws.batch.current = Some(1);
            ws.batch.format = "jpg".into();
            ws.batch.out_dir = Some(out_dir.clone());
            ws.batch.recipes = Some(std::sync::Arc::new(
                (0..40)
                    .map(|i| Recipe {
                        name: format!("Test recipe {i:02}"),
                        tags: vec![if i % 2 == 0 { "cool" } else { "warm" }.into()],
                        ..Recipe::default()
                    })
                    .collect(),
            ));
            ws.screen = Screen::Batch;
            cx.notify();
        });
    });
    cx.run_until_parked();
    let toolbar_bounds = cx.update(|window, _| {
        assert!(window.find("batch-settings").visible());
        assert!(window.try_find("batch-recipe-browser").is_none());
        assert!(window.try_find("batch-tag-list").is_none());
        window.find("batch-toolbar").bounds()
    });

    cx.update(|window, cx| window.click("batch-recipe-toggle", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("batch-recipe-browser").visible());
        assert!(window.try_find("batch-tag-list").is_none());
        let list = window.find("batch-recipe-list").bounds();
        let row = window.find(("batch-rc", 0usize)).bounds();
        assert!(list.size.height > gpui_kit::px(0.));
        assert!(
            list.size.height < row.size.height * 40.,
            "the recipe catalog scrolls within a bounded list"
        );
        assert_eq!(window.find("batch-toolbar").bounds(), toolbar_bounds);
        window.click(("batch-rc", 0usize), cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.try_find("batch-recipe-browser").is_none());
        assert_eq!(ws.read(cx).batch.recipe.as_deref(), Some("Test recipe 00"));
        window.click("batch-recipe-toggle", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("batch-filter-toggle", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("batch-tag-list").visible());
        window.click(("batch-tag", 1usize), cx); // Sorted tags: cool, warm.
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.try_find(("batch-rc", 0usize)).is_none());
        assert!(window.find(("batch-rc", 1usize)).visible());
        let search = ws
            .read(cx)
            .batch
            .search
            .as_ref()
            .expect("recipe search input")
            .0
            .clone();
        search.update(cx, |search, cx| search.set_value("recipe 03", window, cx));
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.try_find(("batch-rc", 1usize)).is_none());
        assert!(window.find(("batch-rc", 3usize)).visible());
        assert_eq!(ws.read(cx).batch.recipe.as_deref(), Some("Test recipe 00"));
        window.click("batch-filter-toggle", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("batch-tag-all", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        let search = ws.read(cx).batch.search.as_ref().unwrap().0.clone();
        search.update(cx, |search, cx| search.set_value("", window, cx));
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find(("batch-rc", 0usize)).visible());
        window.click("batch-recipe-toggle", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click(("batch-fmt", 13usize), cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.find("batch-settings").visible());
        assert!(window.find("batch-out").visible());
        assert!(window.find("batch-run").visible());
        assert_eq!(window.find("batch-toolbar").bounds(), toolbar_bounds);
        let batch = &ws.read(cx).batch;
        assert_eq!(batch.recipe.as_deref(), Some("Test recipe 00"));
        assert_eq!(batch.current, Some(1));
        assert_eq!(
            batch
                .items
                .iter()
                .map(|item| item.selected)
                .collect::<Vec<_>>(),
            [true, false, true]
        );
        assert_eq!(batch.items.iter().filter(|item| item.selected).count(), 2);
        assert_eq!(batch.format, "png");
        assert_eq!(batch.out_dir.as_ref(), Some(&out_dir));
        assert!(
            batch.running.is_none(),
            "changing export settings does not start an export"
        );
    });
}

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
    fn grade_rail_adds_editable_adjustments_without_painting(cx: &mut TestAppContext) {
        let (_, e, cx) = setup(cx, Tool::Brush);
        let (before, revision, steps, selected) = cx.update(|_, cx| {
            let e = e.read(cx);
            (
                e.editor.doc.clone(),
                e.editor.revision,
                e.editor.history.len(),
                e.selected,
            )
        });
        let original =
            emulsion_raster::composite::flatten(&before.composite_tree(), 0).get(128, 96);

        cx.update(|window, cx| window.click("Grade", cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.find("sidebar-adjustments-content").visible());
            let e = e.read(cx);
            assert!(e.tool == Tool::Grade);
            assert_eq!(e.editor.doc, before);
            assert_eq!(e.editor.revision, revision);
            assert_eq!(e.editor.history.len(), steps);
            assert_eq!(e.selected, selected);
        });

        // A canvas click after leaving Brush must not deposit paint.
        let center = at(&e, cx, (128.0, 96.0));
        cx.simulate_click(center, gpui_kit::Modifiers::none());
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = e.read(cx);
            assert_eq!(e.editor.doc, before);
            assert_eq!(e.editor.revision, revision);
            assert_eq!(e.editor.history.len(), steps);
        });

        cx.update(|window, cx| window.click("grade-exposure", cx));
        cx.run_until_parked();
        let id = cx.update(|window, cx| {
            assert!(window.find("sidebar-properties-content").visible());
            let e = e.read(cx);
            let id = e.selected.expect("new adjustment selected");
            let NodeKind::Adjust(adjustment) = &e.editor.doc.node(id).unwrap().kind else {
                panic!("Grade must create an editable adjustment");
            };
            assert_eq!(adjustment.key(), "exposure");
            assert_eq!(e.editor.doc.nodes.len(), before.nodes.len() + 1);
            assert_eq!(e.editor.history.len(), steps + 1);
            id
        });
        // Use the same parameter command as the Properties exposure slider.
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                e.execute(
                    Command::SetParam {
                        id,
                        key: "exposure".into(),
                        value: 1.0,
                    },
                    cx,
                );
            });
        });
        cx.run_until_parked();
        let (graded, graded_revision) = cx.update(|_, cx| {
            let e = e.read(cx);
            let pixel =
                emulsion_raster::composite::flatten(&e.editor.doc.composite_tree(), 0).get(128, 96);
            for channel in 0..3 {
                assert!(
                    pixel[channel] > original[channel],
                    "exposure changes the composite"
                );
            }
            assert_eq!(pixel[3], original[3]);
            assert_eq!(
                e.editor.doc.node(before.nodes[0].id),
                before.node(before.nodes[0].id)
            );
            assert_eq!(e.editor.history.len(), steps + 2);
            (e.editor.doc.clone(), e.editor.revision)
        });

        cx.update(|window, cx| window.click("grade-adjustments", cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.find("sidebar-adjustments-content").visible());
            window.click("Grade", cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.find("sidebar-properties-content").visible());
            let e = e.read(cx);
            assert_eq!(e.selected, Some(id));
            assert_eq!(e.editor.doc, graded);
            assert_eq!(e.editor.revision, graded_revision);
            assert_eq!(e.editor.history.len(), steps + 2);
        });

        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                e.undo(cx);
                assert!(matches!(
                    e.editor.doc.node(id).unwrap().kind,
                    NodeKind::Adjust(_)
                ));
                assert_eq!(
                    emulsion_raster::composite::flatten(&e.editor.doc.composite_tree(), 0)
                        .get(128, 96),
                    original,
                    "undo restores the exposure while retaining the adjustment"
                );
                e.undo(cx);
                assert_eq!(e.editor.doc, before, "the next undo removes the adjustment");
            });
        });
    }

    #[gpui_kit::test]
    fn sidebar_tabs_preserve_document_and_adjustments_open_properties(cx: &mut TestAppContext) {
        let (ws, cx) = open(cx, doc(&["Paper", "Ink", "Tone"], None));
        let e = editor(&ws, cx);
        let (before, revision, steps, selected) = cx.update(|_, cx| {
            let e = e.read(cx);
            (
                e.editor.doc.clone(),
                e.editor.revision,
                e.editor.history.len(),
                e.selected,
            )
        });
        cx.update(|window, _| {
            assert!(window.find("sidebar-history-content").visible());
            assert!(window.try_find("sidebar-adjustments-content").is_none());
            assert!(window.try_find("reference-panel").is_none());
            assert!(window.find(("row", 3u64)).visible());
        });

        for (tab, content) in [
            ("sidebar-adjustments", "sidebar-adjustments-content"),
            ("sidebar-reference", "reference-panel"),
            ("sidebar-properties", "sidebar-properties-content"),
        ] {
            cx.update(|window, cx| window.click(tab, cx));
            cx.run_until_parked();
            cx.update(|window, cx| {
                assert!(
                    window.find(content).visible(),
                    "{tab} displays its own controls"
                );
                assert!(
                    window.find(("row", 3u64)).visible(),
                    "Layers remains available"
                );
                let e = e.read(cx);
                assert_eq!(e.editor.doc, before);
                assert_eq!(e.editor.revision, revision);
                assert_eq!(e.editor.history.len(), steps);
                assert_eq!(e.selected, selected);
            });
        }

        cx.update(|window, cx| window.click("sidebar-panels-toggle", cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.find("sidebar-history").visible());
            window.click("sidebar-history", cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.find("sidebar-history-content").visible());
            assert!(window.try_find("sidebar-panel-menu").is_none());
            assert!(window.find(("row", 3u64)).visible());
            let e = e.read(cx);
            assert_eq!(e.editor.doc, before);
            assert_eq!(e.editor.history.len(), steps);
            assert_eq!(e.selected, selected);
        });

        cx.update(|window, cx| window.click("sidebar-adjustments", cx));
        cx.run_until_parked();
        // Exposure is the first quick adjustment in the Light group.
        cx.update(|window, cx| window.click(("qa", 500usize), cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.find("sidebar-properties-content").visible());
            assert!(window.try_find("sidebar-adjustments-content").is_none());
            let e = e.read(cx);
            let id = e.selected.expect("new adjustment selected");
            let NodeKind::Adjust(adjustment) = &e.editor.doc.node(id).unwrap().kind else {
                panic!("the selected node must be the new adjustment");
            };
            assert_eq!(adjustment.key(), "exposure");
            assert_eq!(e.editor.doc.nodes.len(), before.nodes.len() + 1);
            assert_eq!(e.editor.history.len(), steps + 1);
        });
    }

    #[gpui_kit::test]
    fn sidebar_leaving_recipes_cancels_preview_before_the_next_edit(cx: &mut TestAppContext) {
        let (ws, cx) = open(cx, doc(&["Photo"], None));
        let e = editor(&ws, cx);
        let (before, steps) = cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                e.execute(
                    Command::SetOpacity {
                        id: 1,
                        opacity: 0.8,
                    },
                    cx,
                );
                (e.editor.doc.clone(), e.editor.history.len())
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.click("sidebar-panels-toggle", cx));
        cx.run_until_parked();
        cx.update(|window, cx| window.click("sidebar-recipes", cx));
        cx.run_until_parked();
        let recipe = emulsion_recipes::starter_set()
            .into_iter()
            .find(|recipe| recipe.name == "Slide Punch")
            .unwrap();
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                e.preview_recipe(&recipe, cx);
                assert!(e.recipes.preview.is_some());
                assert!(e.editor.in_transaction());
                assert_ne!(
                    e.editor.doc, before,
                    "the preview is visible but not committed"
                );
                assert_eq!(e.editor.history.len(), steps);
            });
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.click("sidebar-properties", cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.find("sidebar-properties-content").visible());
            e.update(cx, |e, cx| {
                assert!(e.recipes.preview.is_none());
                assert!(
                    !e.editor.in_transaction(),
                    "leaving Recipes closes its preview transaction"
                );
                assert_eq!(e.editor.doc, before);
                assert_eq!(e.editor.history.len(), steps);
                e.execute(
                    Command::SetOpacity {
                        id: 1,
                        opacity: 0.4,
                    },
                    cx,
                );
                assert_eq!(
                    e.editor.history.len(),
                    steps + 1,
                    "the next edit gets its own undo step"
                );
                // A later Recipes cleanup must not roll that unrelated edit back.
                e.cancel_preview(cx);
                assert_eq!(e.editor.doc.node(1).unwrap().opacity, 0.4);
                e.undo(cx);
                assert_eq!(
                    e.editor.doc, before,
                    "undo restores the pre-preview document"
                );
                assert_eq!(e.editor.history.len(), steps);
                e.redo(cx);
                assert_eq!(e.editor.doc.node(1).unwrap().opacity, 0.4);
                assert_eq!(
                    e.editor.doc.nodes.len(),
                    before.nodes.len(),
                    "redo never resurrects the discarded recipe"
                );
            });
        });
    }

    #[gpui_kit::test]
    fn sidebar_leaving_timeline_stops_playback_and_restores_the_full_canvas(
        cx: &mut TestAppContext,
    ) {
        let (ws, cx) = open(cx, doc(&["Frame 1", "Frame 2", "Frame 3"], None));
        let e = editor(&ws, cx);
        let (before, revision, steps) = cx.update(|_, cx| {
            let e = e.read(cx);
            (
                e.editor.doc.clone(),
                e.editor.revision,
                e.editor.history.len(),
            )
        });
        cx.update(|window, cx| window.click("sidebar-panels-toggle", cx));
        cx.run_until_parked();
        cx.update(|window, cx| window.click("sidebar-timeline", cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                assert!(e.anim.open);
                assert_ne!(
                    e.render_doc(),
                    before,
                    "Timeline previews an individual frame"
                );
                e.anim_play(true, cx);
                assert!(e.anim.playing);
            });
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.click("sidebar-properties", cx));
        cx.run_until_parked();
        let stopped_frame = cx.update(|window, cx| {
            assert!(window.find("sidebar-properties-content").visible());
            let e = e.read(cx);
            assert!(!e.anim.open && !e.anim.playing);
            assert_eq!(
                e.render_doc(),
                before,
                "all original layers return to the canvas"
            );
            e.anim.frame
        });
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(1));
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = e.read(cx);
            assert_eq!(
                e.anim.frame, stopped_frame,
                "the old playback timer stays stopped"
            );
            assert_eq!(e.render_doc(), before);
            assert_eq!(e.editor.doc, before);
            assert_eq!(e.editor.revision, revision);
            assert_eq!(e.editor.history.len(), steps);
        });
    }

    #[gpui_kit::test]
    fn replay_plays_the_history_back_and_closes_with_the_timeline(cx: &mut TestAppContext) {
        let (ws, cx) = open(cx, doc(&["Sketch"], None));
        let e = editor(&ws, cx);
        let id = cx.update(|_, cx| e.read(cx).editor.doc.nodes[0].id);
        // Three edits: the root commit plus the moments before each edit
        // and the present make four distinct pictures (the first edit's
        // "before" equals the root and is not repeated).
        for o in [0.8, 0.6, 0.4] {
            cx.update(|_, cx| {
                e.update(cx, |e, cx| {
                    e.execute(Command::SetOpacity { id, opacity: o }, cx);
                })
            });
        }
        cx.update(|window, cx| window.click("sidebar-panels-toggle", cx));
        cx.run_until_parked();
        cx.update(|window, cx| window.click("sidebar-timeline", cx));
        cx.run_until_parked();
        cx.update(|window, cx| window.click("replay", cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = e.read(cx);
            let r = e.anim.replay.as_ref().expect("replay open");
            assert_eq!(r.len(), 4);
            assert!(
                e.editor.doc.nodes[0].opacity == 0.4,
                "the document itself is untouched"
            );
        });
        for _ in 0..8 {
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(400));
            cx.run_until_parked();
        }
        cx.update(|window, cx| {
            assert!(window.find("replay-overlay").visible());
            let e = e.read(cx);
            let r = e.anim.replay.as_ref().expect("still open after playing");
            assert!(!r.playing, "playback stops at the present");
            assert_eq!(r.frame, 3);
        });
        cx.update(|window, cx| window.click("sidebar-properties", cx));
        cx.run_until_parked();
        cx.update(|_, cx| assert!(e.read(cx).anim.replay.is_none()));
    }

    #[gpui_kit::test]
    fn timeline_frames_are_top_level_layers_and_groups_keep_their_children(
        cx: &mut TestAppContext,
    ) {
        let mut d = doc(&["Loose"], None);
        let g = Command::AddNode {
            node: Box::new(Node::group(0, "Scene")),
            slot: Slot::TOP,
        }
        .apply(&mut d)
        .unwrap()
        .unwrap();
        Command::AddNode {
            node: Box::new(Node::raster(
                0,
                "Inside",
                Arc::new(Raster::solid(256, 192, [0.9, 0.1, 0.1, 1.0])),
                Placement::default(),
            )),
            slot: Slot::top_of(Some(g)),
        }
        .apply(&mut d)
        .unwrap();
        let (ws, cx) = open(cx, d);
        let e = editor(&ws, cx);
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                assert_eq!(e.frame_count(), 2, "Loose and Scene; Inside is not a frame");
                e.toggle_animation(cx);
                e.anim_step(1, cx);
                let shown = e.render_doc();
                let by = |name: &str| shown.nodes.iter().find(|n| n.name == name).unwrap().visible;
                assert!(
                    by("Scene") && by("Inside"),
                    "the group shows with its child"
                );
                assert!(!by("Loose"));
                e.anim_step(1, cx);
                let shown = e.render_doc();
                let by = |name: &str| shown.nodes.iter().find(|n| n.name == name).unwrap().visible;
                assert!(by("Loose") && !by("Scene"));
                assert!(by("Inside"), "children keep their own visibility flag");
            })
        });
    }

    #[gpui_kit::test]
    fn sidebar_layers_scroll_independently_and_keep_reference_open_on_selection(
        cx: &mut TestAppContext,
    ) {
        let names: Vec<String> = (1..=40).map(|i| format!("Layer {i}")).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let (ws, cx) = open(cx, doc(&names, None));
        let e = editor(&ws, cx);
        let (before, revision, steps) = cx.update(|_, cx| {
            let e = e.read(cx);
            (
                e.editor.doc.clone(),
                e.editor.revision,
                e.editor.history.len(),
            )
        });
        cx.update(|window, cx| window.click("sidebar-reference", cx));
        cx.run_until_parked();
        let tab_bounds = cx.update(|window, cx| {
            let list = window.find("sidebar-layers-list").bounds();
            let first = window.find(("row", 40u64));
            assert!(first.visible());
            assert!(list.size.height > gpui_kit::px(0.));
            assert!(
                list.size.height < first.bounds().size.height * 40.,
                "the list is bounded rather than pushing panels out of view"
            );
            assert!(window.find("reference-panel").visible());
            let bounds = window.find("sidebar-reference").bounds();
            window.scroll(
                "sidebar-layers-list",
                gpui_kit::ScrollDelta::Pixels(gpui_kit::point(
                    gpui_kit::px(0.),
                    gpui_kit::px(-5000.),
                )),
                cx,
            );
            bounds
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(
                window.find(("row", 1u64)).visible(),
                "bottom layer can be reached by scrolling Layers"
            );
            assert_eq!(
                window.find("sidebar-reference").bounds(),
                tab_bounds,
                "only the Layers list scrolls"
            );
            assert!(window.find("reference-panel").visible());
            window.click(("row", 1u64), cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(
                window.find("reference-panel").visible(),
                "selecting a layer keeps the reference beside the canvas"
            );
            assert!(window.try_find("sidebar-properties-content").is_none());
            let e = e.read(cx);
            assert_eq!(e.selected, Some(1));
            assert_eq!(e.editor.doc, before);
            assert_eq!(e.editor.revision, revision);
            assert_eq!(e.editor.history.len(), steps);
        });
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
                    false,
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
    fn type_tool_places_text_and_move_drags_it(cx: &mut TestAppContext) {
        use emulsion_core::NodeKind;
        let (_, e, cx) = setup(cx, Tool::Type);
        let p = at(&e, cx, (30.0, 20.0));
        cx.simulate_click(p, gpui_kit::Modifiers::none());
        cx.run_until_parked();
        let (id, spec) = cx.update(|_, cx| {
            let e = e.read(cx);
            let n = e.editor.doc.nodes.last().unwrap();
            match &n.kind {
                NodeKind::Text { spec, .. } => (n.id, (**spec).clone()),
                _ => panic!("expected a text node, got {:?}", n.kind.tag()),
            }
        });
        assert_eq!((spec.x, spec.y), (30.0, 20.0));
        assert_eq!(spec.text, "Text");
        // The field is open and focused: typing re-shapes the layer.
        cx.simulate_keystrokes("H i");
        cx.run_until_parked();
        let (text, name) = cx.update(|_, cx| {
            let e = e.read(cx);
            let n = e.editor.doc.node(id).unwrap();
            match &n.kind {
                NodeKind::Text { spec, .. } => (spec.text.clone(), n.name.clone()),
                _ => unreachable!(),
            }
        });
        assert_eq!(text, "Hi");
        assert_eq!(name, "Hi", "layer named after its first line");
        cx.simulate_keystrokes("ctrl-enter");
        cx.run_until_parked();
        // Bold via the options bar applies to the selected layer.
        cx.update(|_, cx| e.update(cx, |e, cx| e.restyle_text(|s| s.bold = true, cx)));
        // Check exact pointer displacement without the shared snapping behavior.
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                e.snap = false;
                e.set_tool(Tool::Move, cx);
            })
        });
        cx.run_until_parked();
        drag(&e, cx, (35.0, 30.0), (85.0, 60.0));
        let spec = cx.update(|_, cx| {
            let e = e.read(cx);
            match &e.editor.doc.node(id).unwrap().kind {
                NodeKind::Text { spec, .. } => (**spec).clone(),
                _ => unreachable!(),
            }
        });
        assert!(spec.bold);
        assert_eq!((spec.x, spec.y), (80.0, 50.0), "{spec:?}");
        // One undo per gesture: move, bold, then the typing session.
        cx.update(|_, cx| e.update(cx, |e, cx| e.undo(cx)));
        cx.update(|_, cx| e.update(cx, |e, cx| e.undo(cx)));
        let spec = cx.update(|_, cx| {
            let e = e.read(cx);
            match &e.editor.doc.node(id).unwrap().kind {
                NodeKind::Text { spec, .. } => (**spec).clone(),
                _ => unreachable!(),
            }
        });
        assert!(
            !spec.bold && spec.x == 30.0 && spec.text == "Hi",
            "{spec:?}"
        );
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
    fn assistant_strokes_pressure_matches_immediate_render(cx: &mut TestAppContext) {
        assert_assistant_strokes_pressure(
            cx,
            1.0,
            serde_json::json!([[10, 96, 0.2], [246, 96, 1.0]]),
        );
    }

    #[gpui_kit::test]
    fn assistant_strokes_curved_pressure_matches_immediate_render(cx: &mut TestAppContext) {
        assert_assistant_strokes_pressure(
            cx,
            2.0,
            serde_json::json!([[10, 96, 0.2], [246, 96, 1.0]]),
        );
    }

    #[gpui_kit::test]
    fn assistant_strokes_optional_pressure_matches_immediate_render(cx: &mut TestAppContext) {
        assert_assistant_strokes_pressure(cx, 2.0, serde_json::json!([[10, 96], [246, 96, 0.2]]));
    }

    #[gpui_kit::test]
    fn assistant_strokes_optional_end_pressure_matches_immediate_render(cx: &mut TestAppContext) {
        assert_assistant_strokes_pressure(cx, 2.0, serde_json::json!([[10, 96, 0.2], [246, 96]]));
    }

    fn assert_assistant_strokes_pressure(
        cx: &mut TestAppContext,
        pressure_curve: f32,
        points: serde_json::Value,
    ) {
        use emulsion_core::NodeKind;
        let base = Raster::transparent(256, 192);
        let (ws, cx) = open(cx, doc(&["Ink"], Some(base.clone())));
        cx.run_until_parked();
        let e = editor(&ws, cx);
        let id = cx.update(|_, cx| e.read(cx).editor.doc.nodes[0].id);
        let script = cx.update(|_, cx| {
            emulsion_mcp::exec::paint_script(
                &e.read(cx).editor.doc,
                &serde_json::json!({
                    "node": id,
                    "color": "#000000",
                    "settings": {
                        "size": 32, "hardness": 1, "spacing": 0.05,
                        "opacity": 1, "flow": 1, "size_pressure": 1,
                        "flow_pressure": 0, "pressure_curve": pressure_curve,
                        "grain": "None", "grain_strength": 0,
                        "wetness": 0, "taper_start": 0, "taper_end": 0,
                        "stabilizer": 0, "size_jitter": 0, "scatter": 0
                    },
                    "strokes": [{"points": points}]
                }),
            )
            .unwrap()
        });
        let (expected, _) = script.render(&base);
        let steps = cx.update(|_, cx| e.read(cx).editor.history.len());
        cx.update(|_, cx| e.update(cx, |e, cx| e.start_playback(None, script, cx)));
        let mut ticks = 0;
        for _ in 0..600 {
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(16));
            cx.run_until_parked();
            ticks += 1;
            if cx.update(|_, cx| e.read(cx).assistant.playback.is_none()) {
                break;
            }
        }
        cx.update(|_, cx| {
            e.update(cx, |e, _| {
                assert!(e.assistant.playback.is_none(), "playback finished");
                assert!(ticks > 1, "the stroke crossed playback tick boundaries");
                assert_eq!(e.editor.history.len(), steps + 1, "one undo step");
                let NodeKind::Raster { raster, .. } = &e.editor.doc.node(id).unwrap().kind else {
                    panic!("expected the ink layer");
                };
                let mut error = 0u64;
                let mut coverage = 0u64;
                for y in 0..192 {
                    for x in 0..256 {
                        let want = expected.get(x, y)[3];
                        error += raster.get(x, y)[3].abs_diff(want) as u64;
                        coverage += want as u64;
                    }
                }
                assert!(coverage > 0, "the reference stroke is visible");
                assert!(
                    error as f64 / (coverage as f64) < 0.005,
                    "animated coverage differs from immediate rendering: {error}/{coverage}"
                );
                let width = |r: &Raster, x| (0..192).filter(|&y| r.get(x, y)[3] > 6553).count();
                assert!(
                    width(&expected, 32) != width(&expected, 224),
                    "pressure changes the reference stroke's width"
                );
                for x in [32, 64, 128, 192, 224] {
                    assert!(
                        width(raster, x).abs_diff(width(&expected, x)) <= 1,
                        "animated width differs at x={x}"
                    );
                }
                assert!(e.editor.undo(), "the stroke can be undone once");
                let NodeKind::Raster { raster, .. } = &e.editor.doc.node(id).unwrap().kind else {
                    panic!("expected the ink layer after undo");
                };
                assert_eq!(
                    raster.read_rect(base.bounds()),
                    base.read_rect(base.bounds())
                );
            });
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
        cx.update(|window, cx| window.click("sidebar-properties", cx));
        cx.run_until_parked();
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
    fn smart_layer_filters_are_editable_and_undoable(cx: &mut TestAppContext) {
        use emulsion_core::NodeKind;
        use emulsion_filters::Filter;
        let (ws, cx) = open(cx, doc(&["Photo"], None));
        cx.run_until_parked();
        let e = editor(&ws, cx);
        let id = cx.update(|_, cx| e.read(cx).editor.doc.nodes[0].id);
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                e.selected = Some(id);
                e.convert_smart(cx);
                e.add_filter(id, Filter::GaussianBlur { radius: 8.0 }, cx);
            })
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            let e = e.read(cx);
            let NodeKind::Smart {
                filters,
                cache,
                source,
                ..
            } = &e.editor.doc.node(id).unwrap().kind
            else {
                panic!("expected a smart layer");
            };
            assert_eq!(filters.len(), 1);
            assert!(
                cache.width() > source.width(),
                "the blur spread past the edge"
            );
            assert_eq!(e.editor.history.len(), 2, "convert, then one filter step");
        });
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                e.set_filter_param(id, 0, "radius", 2.0, true, cx)
            })
        });
        cx.run_until_parked();
        let radius = cx.update(
            |_, cx| match &e.read(cx).editor.doc.node(id).unwrap().kind {
                NodeKind::Smart { filters, .. } => filters[0].params()[0].value,
                _ => unreachable!(),
            },
        );
        assert_eq!(radius, 2.0);
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                e.undo(cx);
                e.undo(cx);
                e.undo(cx);
            })
        });
        assert_eq!(
            cx.update(|_, cx| e.read(cx).editor.doc.node(id).unwrap().kind.tag()),
            "px"
        );
    }

    #[gpui_kit::test]
    fn mask_painting_hides_pixels_and_mask_ops_work(cx: &mut TestAppContext) {
        use crate::editor::PaintKind;
        let (_, e, cx) = setup(cx, Tool::Brush);
        let id = cx.update(|_, cx| e.read(cx).editor.doc.nodes[0].id);
        cx.update(|_, cx| {
            e.update(cx, |e, cx| {
                e.selected = Some(id);
                e.add_mask(cx);
                e.set_mask_edit(true, cx);
                e.set_paint(PaintKind::Eraser, cx);
                e.apply_preset_named("Hard eraser", cx);
            })
        });
        cx.run_until_parked();
        drag(&e, cx, (60.0, 96.0), (200.0, 96.0));
        let (hidden, shown, steps) = cx.update(|_, cx| {
            let e = e.read(cx);
            let m = e.editor.doc.node(id).unwrap().mask.as_ref().expect("mask");
            (m.get(128, 96), m.get(128, 20), e.editor.history.len())
        });
        assert!(
            hidden < 30 && shown == 255,
            "eraser painted the mask black along the stroke: {hidden} {shown}"
        );
        assert_eq!(steps, 2, "add mask, then one stroke");
        let px = cx.update(|_, cx| {
            emulsion_raster::composite::flatten(&e.read(cx).editor.doc.composite_tree(), 0)
                .get(128, 96)
        });
        assert!(
            px[3] < 5000,
            "the masked pixels vanish from the composite: {px:?}"
        );
        cx.update(|_, cx| e.update(cx, |e, cx| e.invert_mask(cx)));
        assert_eq!(
            cx.update(|_, cx| e
                .read(cx)
                .editor
                .doc
                .node(id)
                .unwrap()
                .mask
                .as_ref()
                .unwrap()
                .get(128, 20)),
            0
        );
        cx.update(|_, cx| e.update(cx, |e, cx| e.remove_mask(cx)));
        assert!(cx.update(|_, cx| e.read(cx).editor.doc.node(id).unwrap().mask.is_none()));
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
    fn crop_cuts_layers_to_the_canvas_by_default_and_only_moves_them_when_asked(
        cx: &mut TestAppContext,
    ) {
        for delete in [true, false] {
            let (_, e, cx) = setup(cx, Tool::Crop);
            cx.update(|_, cx| e.update(cx, |e, _| e.tools.crop_delete = delete));
            drag(&e, cx, (50.0, 40.0), (150.0, 140.0));
            cx.simulate_keystrokes("enter");
            cx.run_until_parked();
            let (w, h, x, lw, steps) = cx.update(|_, cx| {
                let e = e.read(cx);
                let d = &e.editor.doc;
                let NodeKind::Raster { placement, raster } = &d.nodes[0].kind else {
                    panic!()
                };
                (
                    d.width,
                    d.height,
                    placement.x,
                    raster.width(),
                    e.editor.history.len(),
                )
            });
            assert!(
                (w as i32 - 100).abs() <= 1 && (h as i32 - 100).abs() <= 1,
                "{w}×{h}"
            );
            assert_eq!(steps, 1, "crop and cut are one undo step (delete {delete})");
            if delete {
                assert!((x.abs()) <= 1.0, "cut layer starts at the canvas: x = {x}");
                assert!((lw as i32 - 100).abs() <= 1, "layer cut to {lw} px wide");
            } else {
                assert!(
                    (x + 50.0).abs() <= 1.0,
                    "layer moved, not resampled: x = {x}"
                );
                assert_eq!(lw, 256, "layer kept whole");
            }
        }
    }

    #[gpui_kit::test]
    fn shape_drag_adds_an_editable_vector_node(cx: &mut TestAppContext) {
        let (_, e, cx) = setup(cx, Tool::Shape);
        drag(&e, cx, (30.0, 30.0), (90.0, 70.0));
        let (kind, masked, vector) = cx.update(|_, cx| {
            let n = e.read(cx).editor.doc.nodes.last().unwrap().clone();
            (
                n.name.clone(),
                n.mask.is_some(),
                matches!(n.kind, emulsion_core::NodeKind::Path { .. }),
            )
        });
        assert_eq!((kind.as_str(), masked, vector), ("Rectangle", false, true));
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
