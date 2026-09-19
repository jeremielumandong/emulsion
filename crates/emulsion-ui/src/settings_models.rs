//! Settings › Local models: the manifest with install, remove and progress.

use crate::theme::Palette;
use crate::widgets::{chip, mono};
use crate::workspace::Workspace;
use emulsion_ai::jobs::Job;
use emulsion_ai::models::{self, MANIFEST, Status};
use gpui_kit::*;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

#[derive(Default)]
pub(crate) struct ModelJobs {
    pub running: HashMap<&'static str, Arc<Job>>,
    pub errors: HashMap<&'static str, String>,
    polling: bool,
}

impl Workspace {
    pub fn install_model(&mut self, id: &'static str, cx: &mut Context<Self>) {
        let Some(spec) = models::spec(id) else {
            return;
        };
        if self.model_jobs.running.contains_key(id) {
            return;
        }
        let job = Job::new();
        job.set_stage("downloading");
        self.model_jobs.errors.remove(id);
        self.model_jobs.running.insert(id, job.clone());
        // Blocking network I/O gets its own thread; the UI polls the job.
        std::thread::Builder::new()
            .name(format!("download-{id}"))
            .spawn(move || {
                let j = job.clone();
                let r = models::download(
                    spec,
                    &move |done, total| {
                        if total > 0 {
                            j.progress(done as f32 / total as f32);
                        }
                    },
                    job.cancel_flag(),
                );
                if let Err(e) = r {
                    job.set_stage(format!("failed: {e}"));
                } else {
                    job.set_stage("installed");
                    job.progress(1.0);
                }
                job.finish();
            })
            .ok();
        self.poll_models(cx);
        cx.notify();
    }

    pub fn cancel_model(&mut self, id: &'static str, cx: &mut Context<Self>) {
        if let Some(j) = self.model_jobs.running.get(id) {
            j.cancel();
        }
        cx.notify();
    }

    pub fn remove_model(&mut self, id: &'static str, cx: &mut Context<Self>) {
        if let Some(spec) = models::spec(id) {
            if let Err(e) = models::remove(spec) {
                self.model_jobs.errors.insert(id, e.to_string());
            }
            emulsion_ai::runner::clear_cache();
        }
        cx.notify();
    }

    fn poll_models(&mut self, cx: &mut Context<Self>) {
        if self.model_jobs.polling {
            return;
        }
        self.model_jobs.polling = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(200))
                    .await;
                let more = this.update(cx, |this, cx| {
                    let done: Vec<&'static str> = this
                        .model_jobs
                        .running
                        .iter()
                        .filter(|(_, j)| j.is_finished())
                        .map(|(id, _)| *id)
                        .collect();
                    for id in done {
                        if let Some(j) = this.model_jobs.running.remove(id) {
                            let stage = j.stage();
                            if let Some(msg) = stage.strip_prefix("failed: ") {
                                this.model_jobs.errors.insert(id, msg.to_string());
                            }
                        }
                    }
                    cx.notify();
                    let more = !this.model_jobs.running.is_empty();
                    if !more {
                        this.model_jobs.polling = false;
                    }
                    more
                });
                if !matches!(more, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    pub(crate) fn models_list(&self, p: &Palette, cx: &mut Context<Self>) -> Div {
        let mut list = div().flex().flex_col().gap(px(8.)).pt(px(4.));
        let mut task = None;
        for (i, m) in MANIFEST.iter().enumerate() {
            let status = models::status(m);
            let job = self.model_jobs.running.get(m.id).cloned();
            let err = self.model_jobs.errors.get(m.id).cloned();
            if task != Some(m.task) {
                task = Some(m.task);
                list = list.child(mono(m.task.label().to_uppercase(), 9.5, p.muted).pt(px(6.)));
            }
            let state = match (&job, status) {
                (Some(j), _) => j.summary(),
                (None, Status::Installed) => "installed".into(),
                (None, Status::Partial) => "incomplete".into(),
                (None, Status::Missing) => models::human_bytes(m.total_bytes()),
            };
            let id = m.id;
            let mut row = div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(
                    div()
                        .w(px(260.))
                        .flex_none()
                        .text_size(px(13.))
                        .child(m.name.to_string()),
                )
                .child(
                    mono(
                        state,
                        10.5,
                        if status == Status::Installed {
                            p.accent
                        } else {
                            p.ink
                        },
                    )
                    .w(px(150.))
                    .flex_none(),
                )
                .child(mono(m.license, 9.5, p.muted).w(px(200.)).flex_none());
            row = match (&job, status) {
                (Some(_), _) => row.child(
                    chip(("model-cancel", i), "cancel", false, p)
                        .on_click(cx.listener(move |this, _, _, cx| this.cancel_model(id, cx))),
                ),
                (None, Status::Installed) => row.child(
                    chip(("model-remove", i), "remove", false, p)
                        .on_click(cx.listener(move |this, _, _, cx| this.remove_model(id, cx))),
                ),
                (None, _) => row.child(
                    chip(("model-install", i), "install", true, p)
                        .on_click(cx.listener(move |this, _, _, cx| this.install_model(id, cx))),
                ),
            };
            let mut block = div().flex().flex_col().gap(px(3.)).child(row);
            if let Some(j) = &job {
                let f = j.fraction();
                block = block.child(
                    div()
                        .w(px(620.))
                        .h(px(3.))
                        .bg(p.line)
                        .child(div().h_full().w(px(620.0 * f)).bg(p.accent)),
                );
            }
            block = block.child(mono(m.note, 10., p.muted).max_w(px(640.)));
            if let Some(e) = err {
                block = block.child(mono(e, 10., p.accent));
            }
            list = list.child(block);
        }
        list.child(
            mono(
                format!(
                    "Models are stored in {} and run on the {} with ONNX Runtime. Nothing downloads until you ask.",
                    models::models_dir().display(),
                    emulsion_ai::runner::provider().label()
                ),
                10.,
                p.muted,
            )
            .pt(px(8.)),
        )
    }
}
