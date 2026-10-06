use super::*;
use ::core::prelude::v1::test;
use emulsion_core::{project::ProjectEditor, project::ProjectKind, storyboard::Panel};
use emulsion_raster::blend::BlendSpace;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;

// Use the real local relay so the visible transcript and the actual tool
// response are checked together, without requiring a provider or CLI.
fn call(relay: &Relay, name: &str, args: Value) -> (RelayCall, std::thread::JoinHandle<Value>) {
    let (addr, token, name) = (relay.addr, relay.token.clone(), name.to_string());
    let client = std::thread::spawn(move || {
        let mut stream = TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        writeln!(
            stream,
            "{}",
            json!({"token":token,"name":name,"arguments":args})
        )
        .unwrap();
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    });
    (relay.calls.recv_blocking().unwrap(), client)
}

fn editor(cx: &mut TestAppContext, protected: bool) -> Entity<EditorView> {
    let mut doc = Document::new(32, 18);
    if protected {
        doc.blend_space = BlendSpace::PhotoshopSrgbV1;
    }
    let mut project = ProjectEditor::new_project(ProjectKind::Storyboard, doc).unwrap();
    project
        .insert_panels(
            Some(1),
            &Document::new(32, 18),
            vec![("Remaining panel".into(), Panel::new(0, 24))],
            None,
        )
        .unwrap();
    project.create_board_version("Before removal").unwrap();
    project.remove_page(1).unwrap();
    cx.update(|cx| {
        crate::theme::install(cx);
        cx.set_global(app_state::AppSettings(emulsion_io::settings::Settings {
            suggestions: false,
            ..Default::default()
        }));
        cx.new(|cx| {
            let mut view = EditorView::new(
                project.doc.clone(),
                None,
                None,
                None,
                "Save notice".into(),
                cx,
            );
            view.editor = project;
            view.assistant.running = true;
            view.assistant.native_tool_steps = true;
            let mut turn = Turn {
                text: "Earlier result".into(),
                ..Default::default()
            };
            turn.review.stop();
            view.assistant.turn = Some(turn);
            view
        })
    })
}

#[gpui_kit::test]
fn project_save_and_document_alias_keep_v2_warning_visible_after_turn_completion(
    cx: &mut TestAppContext,
) {
    let folder = tempfile::tempdir().unwrap();
    for name in ["save_project", "save_document"] {
        for protected in [false, true] {
            let relay = Relay::start().unwrap();
            let view = editor(cx, protected);
            let path = folder.path().join(format!("{name}-{protected}.emu"));
            let (call, reply) = call(&relay, name, json!({"path":path}));
            view.update(cx, |view, cx| {
                view.run_tool_now(call, cx);
                assert!(view.assistant.tool_busy);
                assert_eq!(view.assistant.turn.as_ref().unwrap().text, "Earlier result");
                view.complete_provider_turn(0.125, cx);
                assert!(view.assistant.completion_pending);
            });
            cx.run_until_parked();
            let response = reply.join().unwrap();
            assert_eq!(response["isError"], false, "{response}");
            let result: Value =
                serde_json::from_str(response["content"][0]["text"].as_str().unwrap()).unwrap();
            view.read_with(cx, |view, _| {
                assert!(!view.assistant.running);
                assert!(!view.assistant.tool_busy);
                assert!(!view.editor.is_modified());
                assert_eq!(view.editor.path.as_ref(), Some(&path));
                let current = view.assistant.turn.as_ref().unwrap();
                let history = view.assistant.history.last().unwrap();
                assert_eq!(current.text, history.text);
                if protected {
                    let notice = crate::workspace::save_notice::project_notice(2).unwrap();
                    assert_eq!(current.text, format!("Earlier result\n\n{notice}"));
                    assert_eq!(result["project_format_version"], 2);
                    assert_eq!(result["warnings"].as_array().unwrap().len(), 1);
                } else {
                    assert_eq!(current.text, "Earlier result");
                    assert_eq!(result, json!({"path":path}));
                }
            });
        }
    }
}

#[gpui_kit::test]
fn failed_project_save_does_not_publish_success_notice(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let blocked = folder.path().join("blocked");
    std::fs::write(&blocked, b"not a directory").unwrap();
    let path = blocked.join("protected.emu");
    let relay = Relay::start().unwrap();
    let view = editor(cx, true);
    let (call, reply) = call(&relay, "save_project", json!({"path":path}));
    view.update(cx, |view, cx| {
        view.set_status("Existing status", false, cx);
        view.run_tool_now(call, cx);
    });
    cx.run_until_parked();
    assert_eq!(reply.join().unwrap()["isError"], true);
    view.read_with(cx, |view, _| {
        assert_eq!(view.assistant.turn.as_ref().unwrap().text, "Earlier result");
        assert_eq!(view.status.as_ref().unwrap().0.as_ref(), "Existing status");
        assert!(view.editor.is_modified());
        assert!(view.editor.path.is_none());
        assert!(!view.assistant.tool_busy);
    });
    assert!(!path.exists());
}

#[gpui_kit::test]
fn completed_save_does_not_insert_warning_into_a_newer_assistant_turn(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("protected.emu");
    let relay = Relay::start().unwrap();
    let view = editor(cx, true);
    let (call, reply) = call(&relay, "save_project", json!({"path":path}));
    view.update(cx, |view, cx| {
        view.run_tool_now(call, cx);
        // A queued write already owns its immutable snapshot. It can finish
        // after Stop, but cannot attach its warning to a newer conversation.
        view.stop_assistant(cx);
        view.assistant.turn = Some(Turn {
            text: "New turn".into(),
            ..Default::default()
        });
    });
    cx.run_until_parked();
    let response = reply.join().unwrap();
    assert_eq!(
        response["isError"], false,
        "actual file publication is still reported"
    );
    view.read_with(cx, |view, _| {
        assert_eq!(view.assistant.turn.as_ref().unwrap().text, "New turn");
        assert_eq!(
            view.status.as_ref().unwrap().0.as_ref(),
            crate::workspace::save_notice::project_notice(2).unwrap()
        );
    });
    assert_eq!(
        emulsion_io::project::required_version(&emulsion_io::project::read(&path).unwrap()),
        2
    );
}

#[test]
fn only_successful_native_results_can_produce_a_saved_format_notice() {
    let v2 = Ok(json!({"project_format_version":2,"warnings":["compatibility"]}));
    assert_eq!(
        completed_save_notice(true, &v2),
        crate::workspace::save_notice::project_notice(2)
    );
    assert!(
        completed_save_notice(false, &v2).is_none(),
        "exports are not native saves"
    );
    assert!(completed_save_notice(true, &Ok(json!({"path":"legacy.emu"}))).is_none());
    assert!(completed_save_notice(true, &Err("save failed".into())).is_none());
}
