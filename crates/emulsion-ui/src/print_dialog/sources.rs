use super::*;
/// A worker prepares immutable sources; the dialog owns cancellation and errors.
pub(crate) fn open_prepared(
    name: String,
    loader: impl FnOnce(Arc<AtomicBool>) -> anyhow::Result<Vec<Source>> + Send + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    let view = cx.new(|cx| PrintDialog::new(name, 0, window, cx));
    view.update(cx, |s, cx| {
        s.scope = "all".into();
        s.settings.layout = Layout::Contact;
        s.settings.creative.labels = print::LabelMode::Name;
        s.load_presets(cx);
        s.load_prepared(loader, true, cx);
        s.refresh(cx);
    });
    show(view, window, cx);
}
impl PrintDialog {
    pub(super) fn load_prepared(
        &mut self,
        loader: impl FnOnce(Arc<AtomicBool>) -> anyhow::Result<Vec<Source>> + Send + 'static,
        original: bool,
        cx: &mut Context<Self>,
    ) {
        if self.source_pending {
            return;
        }
        self.source_pending = true;
        self.source_error = None;
        self.preview = None;
        self.preview_generation += 1;
        let cancel = self.cancel.clone();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move { loader(cancel) }).await;
            this.update(cx, |s, cx| {
                s.source_pending = false;
                match result {
                    Ok(sources) if !sources.is_empty() => {
                        let sources = Arc::new(sources);
                        if original {
                            s.original_sources = Some(sources.clone());
                        } else {
                            s.active = 0;
                            s.scope = "all".into();
                            s.settings.layout = Layout::Contact;
                            s.settings.creative.labels = print::LabelMode::Name;
                        }
                        s.sources = Some(sources);
                        s.sheet = 0;
                        s.suggest_paper();
                    }
                    Ok(_) => s.source_error = Some("No printable sources were selected.".into()),
                    Err(e) => s.source_error = Some(e.to_string()),
                }
                s.changed(cx);
            })
            .ok();
        })
        .detach();
    }
    fn animation_frames(&mut self, cx: &mut Context<Self>) {
        let times = match print::sources::timestamps(&self.fields[17].read(cx).value()) {
            Ok(t) => t,
            Err(e) => {
                self.notice = Some(e.to_string());
                cx.notify();
                return;
            }
        };
        let Some(source) = self
            .original_sources
            .as_ref()
            .and_then(|s| s.get(self.original_active))
            .cloned()
        else {
            return;
        };
        let Some(doc) = source.document else {
            self.notice = Some("This source has no editable animation.".into());
            cx.notify();
            return;
        };
        self.load_prepared(
            move |cancel| print::sources::animation(&source.name, &doc, &times, &cancel),
            false,
            cx,
        );
    }
    fn video_frames(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let times = match print::sources::timestamps(&self.fields[17].read(cx).value()) {
            Ok(t) => t,
            Err(e) => {
                self.notice = Some(e.to_string());
                cx.notify();
                return;
            }
        };
        let request = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = request.await
                && let Some(path) = paths.into_iter().next()
            {
                this.update(cx, |s, cx| {
                    s.load_prepared(
                        move |cancel| print::sources::video(&path, &times, &cancel),
                        false,
                        cx,
                    )
                })
                .ok();
            }
        })
        .detach();
    }
    fn design_frames(&mut self, ids: Option<Vec<u64>>, cx: &mut Context<Self>) {
        let Some(source) = self
            .original_sources
            .as_ref()
            .and_then(|s| s.get(self.original_active))
            .cloned()
        else {
            return;
        };
        let Some(doc) = source.document else { return };
        let ids = ids.unwrap_or_else(|| doc.design.frames.keys().copied().collect());
        self.load_prepared(
            move |cancel| print::sources::frames(&source.name, &doc, &ids, &cancel),
            false,
            cx,
        );
    }
    pub(super) fn source_controls(&self, cx: &Context<Self>) -> AnyElement {
        let frames = self
            .original_sources
            .as_ref()
            .and_then(|s| s.get(self.original_active))
            .and_then(|s| s.document.as_ref())
            .map(|doc| {
                doc.design
                    .frames
                    .keys()
                    .filter_map(|id| doc.node(*id).map(|n| (id.to_string(), n.name.clone())))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        div().when(!frames.is_empty(),|d|d.child(self.select("print-design-frame","Design frame","Choose a frame…".into(),frames,
                |s,v,cx|{if let Ok(id)=v.parse(){s.design_frames(Some(vec![id]),cx);}},cx))
                .child(Button::new("print-all-design-frames").label("Use all Design frames").small().outline().disabled(self.busy||self.source_pending)
                    .on_click(cx.listener(|s,_,_,cx|s.design_frames(None,cx)))))
            .flex().flex_col().gap_2().child(self.field(17,"Storyboard timestamps (ms) · comma separated"))
            .child(Button::new("print-animation-frames").label("Use page animation frames").small().outline().disabled(self.busy||self.source_pending||self.original_sources.is_none())
                .on_click(cx.listener(|s,_,_,cx|s.animation_frames(cx))))
            .child(Button::new("print-video-frames").label("Choose local video frames…").small().outline().disabled(self.busy||self.source_pending)
                .on_click(cx.listener(|s,_,window,cx|s.video_frames(window,cx))))
            .child(Button::new("print-restore-sources").label("Restore original pages / photos").small().ghost().disabled(self.busy||self.source_pending||self.original_sources.is_none())
                .on_click(cx.listener(|s,_,_,cx|{s.sources=s.original_sources.clone();s.active=s.original_active;s.sheet=0;s.source_error=None;s.changed(cx);})))
            .when(self.source_pending,|d|d.child("Preparing print sources…"))
            .child(div().text_color(theme::palette(cx).muted).child("Local video uses installed FFmpeg. Animation frames sample page motion; embedded video stays poster artwork."))
            .into_any_element()
    }
}
