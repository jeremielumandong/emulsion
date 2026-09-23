use super::*;
use emulsion_ai::generate::{Config, Provider};
use emulsion_core::NodeKind;
use emulsion_raster::{Mask, select};
use gpui_kit::test::TestWindowExt;
use std::io::{BufRead, BufReader, Read, Write};
use std::time::{Duration, Instant};

#[gpui_kit::test]
fn generative_removal_taskbar_open_cancel_returns_canvas_focus(cx: &mut TestAppContext) {
    let mut document = doc(&["Photo"], None);
    document.selection = Some(Arc::new(select::rect(256, 192, 100., 70., 40., 40.)));
    let (ws, cx) = open(cx, document);
    let editor = cx.update(|_, cx| {
        let editor = ws.read(cx).editor.clone().unwrap();
        editor.update(cx, |e, cx| e.set_tool(crate::editor::Tool::Select, cx));
        editor
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.click("context-generative-fill", cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        let e = editor.read(cx);
        assert!(e.generate.show_in_taskbar);
        assert!(!e.canvas_focus.is_focused(window));
        assert!(!e.generate.busy);
        assert!(window.find("context-generate-submit").bounds().size.width > gpui_kit::px(0.));
        window.click("context-generate-cancel", cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let e = editor.read(cx);
        assert!(!e.generate.show_in_taskbar);
        assert!(e.canvas_focus.is_focused(window));
        assert!(!e.generate.busy);
        assert_eq!(e.editor.history.len(), 0);
    });
}

#[gpui_kit::test]
fn generative_removal_rejects_missing_or_empty_selection(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    cx.update(|_, cx| {
        ws.read(cx).editor.clone().unwrap().update(cx, |e, cx| {
            let cfg = Config {
                provider: Provider::A1111,
                endpoint: None,
                model: None,
                api_key: None,
            };
            assert!(
                e.generate_text("  ".into(), cfg.clone(), cx)
                    .unwrap_err()
                    .contains("Select an object")
            );
            e.editor.doc.selection = Some(Arc::new(Mask::empty(256, 192, 0)));
            assert!(
                e.generate_text("".into(), cfg, cx)
                    .unwrap_err()
                    .contains("Select an area")
            );
            assert!(!e.generate.busy);
            assert!(e.ai.job.is_none());
            assert_eq!(e.editor.history.len(), 0);
        });
    });
}

#[gpui_kit::test]
fn generative_removal_sends_mask_and_preserves_original_layer(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx, doc(&["Photo"], None));
    let e = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
    let before = cx.update(|_, cx| {
        e.update(cx, |e, _| {
            e.editor.doc.selection = Some(Arc::new(select::rect(256, 192, 100., 70., 40., 40.)));
            e.editor.doc.clone()
        })
    });
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    listener.set_nonblocking(true).unwrap();
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut socket = loop {
            match listener.accept() {
                Ok((socket, _)) => break socket,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("fixture did not receive removal request: {error}"),
            }
        };
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
        assert!(line.starts_with("POST /sdapi/v1/img2img "));
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
        assert!(length > 0 && length < 1_000_000);
        let mut body = vec![0; length];
        reader.read_exact(&mut body).unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(
            body["prompt"]
                .as_str()
                .unwrap()
                .contains("Remove the object")
        );
        assert!(!body["mask"].as_str().unwrap().is_empty());
        assert_eq!(body["init_images"].as_array().unwrap().len(), 1);
        let response = r#"{"images":["iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg=="]}"#;
        write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).unwrap();
    });
    cx.update(|_, cx| {
        e.update(cx, |e, cx| {
            e.generate_text(
                " \n ".into(),
                Config {
                    provider: Provider::A1111,
                    endpoint: Some(endpoint),
                    model: Some("test-fixture".into()),
                    api_key: None,
                },
                cx,
            )
            .unwrap();
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
            assert_eq!(node.name, "Generative removal");
            assert_eq!(node.origin.as_deref(), Some("ai:a1111/test-fixture"));
            let NodeKind::Raster { raster, placement } = &node.kind else {
                panic!("removal raster");
            };
            let _ = placement;
            assert_eq!(
                raster.get(0, 0)[3],
                0,
                "context outside selection remains transparent"
            );
            assert!(
                raster
                    .read_rect(raster.bounds())
                    .iter()
                    .any(|pixel| pixel[3] > 0)
            );
            e.undo(cx);
            assert_eq!(e.editor.doc, before);
        })
    });
}
