//! AI on storyboard panels from the Stage and the Board, with a stand-in
//! for the models: batches, per-panel errors, cancel, one Undo step, and
//! the gates for missing models and providers.
use super::*;
use crate::tests::open;
use core::prelude::v1::test;
use emulsion_core::creation::{CanvasKind, CanvasSpec};
use emulsion_raster::{IRect, Mask};
use gpui_kit::test::TestWindowExt;

/// Denoise returns the picture unchanged; nothing else is installed.
struct Stub;

impl Backend for Stub {
    fn ready(&self, op: &Op) -> Result<(), String> {
        match op {
            Op::Denoise => Ok(()),
            _ => Err("Needs a model: install it under Settings › Local models.".into()),
        }
    }
    fn matte(&self, _: &Raster, _: &Job) -> Result<Mask, String> {
        unreachable!()
    }
    fn upscale_factor(&self) -> u32 {
        4
    }
    fn upscale(&self, _: &Raster, _: &Job) -> Result<Raster, String> {
        unreachable!()
    }
    fn denoise(&self, image: &Raster, _: &Job) -> Result<Raster, String> {
        Ok(image.clone())
    }
    fn fill(
        &self,
        _: &Raster,
        _: &Mask,
        _: Option<&str>,
        _: &Job,
    ) -> Result<(Raster, IRect), String> {
        unreachable!()
    }
    fn model_id(&self, _: &Op) -> String {
        "stub".into()
    }
}

/// Three panels; the first and third have a "Pose" layer.
fn setup(cx: &mut TestAppContext) -> (Entity<EditorView>, Vec<PageId>, &mut VisualTestContext) {
    let (ws, cx) = open(cx, Document::new(64, 36));
    cx.simulate_resize(size(px(1600.), px(1200.)));
    let project = CanvasSpec {
        name: "Board".into(),
        kind: CanvasKind::Storyboard,
        width: 32.,
        height: 18.,
        pages: 3,
        ..Default::default()
    }
    .create_project()
    .unwrap();
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(project, "Board".into(), window, cx)
        });
        ws.read(cx).editor.clone().unwrap()
    });
    let ids = cx.update(|_, cx| {
        view.update(cx, |e, cx| {
            let ids: Vec<_> = e.editor.page_list().iter().map(|m| m.id).collect();
            for id in [ids[0], ids[2]] {
                e.select_page(id, cx);
                e.execute(
                    Command::AddNode {
                        node: Box::new(Node::raster(
                            0,
                            "Pose",
                            Arc::new(Raster::solid(8, 6, [1., 0., 0., 1.])),
                            Placement::at(4., 2.),
                        )),
                        slot: Slot::TOP,
                    },
                    cx,
                );
            }
            e.select_page(ids[0], cx);
            ids
        })
    });
    settle(cx);
    (view, ids, cx)
}

fn settle(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(400));
    cx.run_until_parked();
    cx.update(|window, cx| window.render_frame(cx));
    cx.run_until_parked();
}

fn layers(e: &Entity<EditorView>, id: PageId, cx: &mut VisualTestContext) -> Vec<String> {
    cx.update(|_, cx| {
        e.read(cx)
            .editor
            .page(id)
            .unwrap()
            .doc
            .nodes
            .iter()
            .map(|n| n.name.clone())
            .collect()
    })
}

fn denoise_pose() -> Request {
    Request {
        op: Op::Denoise,
        target: Target::Named("Pose".into()),
    }
}

#[gpui_kit::test]
fn a_board_batch_reports_each_panel_and_undoes_in_one_step(cx: &mut TestAppContext) {
    let (e, ids, cx) = setup(cx);
    let before: Vec<_> = ids.iter().map(|id| layers(&e, *id, cx)).collect();
    let (job, outcome) = cx
        .update(|_, cx| {
            e.update(cx, |e, cx| {
                e.run_panel_ai_with(ids.clone(), denoise_pose(), Arc::new(Stub), cx)
            })
        })
        .unwrap();
    settle(cx);
    assert!(job.is_finished());
    let report = outcome.lock().unwrap().clone().unwrap();
    assert!(
        report.starts_with("Denoise: 2 panels changed, 1 panel failed"),
        "{report}"
    );
    assert!(report.contains("No layer named “Pose”."), "{report}");
    assert!(layers(&e, ids[0], cx).contains(&"Pose denoised".to_string()));
    assert!(layers(&e, ids[2], cx).contains(&"Pose denoised".to_string()));
    assert_eq!(layers(&e, ids[1], cx), before[1]);
    let size = cx.update(|_, cx| {
        let d = &e.read(cx).editor.page(ids[2]).unwrap().doc;
        (d.width, d.height)
    });
    assert_eq!(size, (32, 18), "the panel keeps the project resolution");
    // One Undo step for the whole batch.
    cx.update(|_, cx| e.update(cx, |e, cx| e.undo(cx)));
    settle(cx);
    let after: Vec<_> = ids.iter().map(|id| layers(&e, *id, cx)).collect();
    assert_eq!(after, before);
}

#[gpui_kit::test]
fn cancel_leaves_every_panel_as_it_was(cx: &mut TestAppContext) {
    let (e, ids, cx) = setup(cx);
    let before: Vec<_> = ids.iter().map(|id| layers(&e, *id, cx)).collect();
    let (job, outcome) = cx
        .update(|_, cx| {
            e.update(cx, |e, cx| {
                let started = e.run_panel_ai_with(ids.clone(), denoise_pose(), Arc::new(Stub), cx);
                // The progress card's Cancel.
                e.cancel_ai(cx);
                started
            })
        })
        .unwrap();
    settle(cx);
    assert!(job.cancelled());
    assert_eq!(
        outcome.lock().unwrap().clone().unwrap(),
        "Denoise cancelled: no panel was changed."
    );
    let after: Vec<_> = ids.iter().map(|id| layers(&e, *id, cx)).collect();
    assert_eq!(after, before);
}

#[gpui_kit::test]
fn missing_models_and_providers_stop_before_anything_runs(cx: &mut TestAppContext) {
    let (e, ids, cx) = setup(cx);
    let error = cx
        .update(|_, cx| {
            e.update(cx, |e, cx| {
                let request = Request {
                    op: Op::Upscale,
                    target: Target::Panel,
                };
                e.run_panel_ai_with(ids.clone(), request, Arc::new(Stub), cx)
            })
        })
        .unwrap_err();
    assert!(error.contains("Settings › Local models"));
    assert!(cx.update(|_, cx| e.read(cx).ai.job.is_none()));
    // No provider is set in tests: a prompted fill is refused by the real
    // backend before anything is sent.
    let error = cx
        .update(|_, cx| {
            e.update(cx, |e, cx| {
                let request = Request {
                    op: Op::Fill {
                        prompt: Some("a lighthouse".into()),
                        area: Area::Whole,
                    },
                    target: Target::Panel,
                };
                e.run_panel_ai(ids.clone(), request, cx)
            })
        })
        .unwrap_err();
    assert!(error.contains("Settings › Image generation"), "{error}");
    // Paint's Upscale on a panel never grows the canvas.
    cx.update(|_, cx| e.update(cx, |e, cx| e.ai_upscale(cx)));
    settle(cx);
    let (w, h) = cx.update(|_, cx| (e.read(cx).editor.doc.width, e.read(cx).editor.doc.height));
    assert_eq!((w, h), (32, 18));
}

#[gpui_kit::test]
fn the_dialog_hides_the_prompt_without_a_provider(cx: &mut TestAppContext) {
    let (e, _, cx) = setup(cx);
    let dialog = cx
        .update(|window, cx| {
            e.update(cx, |e, cx| {
                e.storyboard_ai_dialog(Kind::Fill, false, window, cx)
            })
        })
        .unwrap();
    settle(cx);
    cx.update(|window, _| {
        let note = window.find("panel-ai-prompt-note");
        assert!(note.visible());
    });
    cx.update(|window, cx| {
        dialog.update(cx, |d, cx| d.set_prompt("a lighthouse", window, cx));
        dialog.update(cx, |d, cx| d.set_layer("", window, cx));
    });
    let request = cx.update(|_, cx| dialog.read(cx).request(cx)).unwrap();
    assert_eq!(
        request.op,
        Op::Fill {
            prompt: None,
            area: Area::Selection
        },
        "a prompt is never used without a provider"
    );
    // Subject mask needs a layer; Run reports it in the dialog.
    cx.update(|window, cx| window.click("panel-ai-SubjectMask", cx));
    cx.update(|window, cx| window.click("panel-ai-run", cx));
    settle(cx);
    assert_eq!(
        cx.update(|_, cx| dialog.read(cx).message.clone()),
        Some("Name the layer to mask.".into())
    );
}
