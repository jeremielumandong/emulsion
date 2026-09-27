//! Editable YouTube objects and click-to-play presentation surfaces.
use super::*;
use emulsion_core::design::media;
use gpui_kit::component::{
    Sizable, WindowExt,
    button::{Button, ButtonVariants},
};

#[derive(Default)]
pub(super) struct VideoUi {
    active: Option<ActiveVideo>,
    task: Option<Task<()>>,
    focus: Option<FocusHandle>,
}
struct ActiveVideo {
    player: crate::web_player::Player,
    _server: emulsion_io::design_media::PlayerServer,
    id: NodeId,
    image: Option<Arc<RenderImage>>,
    window: AnyWindowHandle,
}
impl EditorView {
    pub(super) fn design_video_playing(&self) -> bool {
        self.video.active.is_some()
    }
    pub(super) fn stop_design_video(&mut self, cx: &mut Context<Self>) {
        self.video.task = None;
        if let Some(active) = self.video.active.take() {
            if let Some(image) = active.image.clone() {
                let handle = active.window;
                cx.defer(move |cx| {
                    cx.update_window(handle, |_, window, _| {
                        let _ = window.drop_image(image);
                    })
                    .ok();
                });
            }
            self.notify_canvas(cx);
            cx.notify();
        }
    }
    pub(super) fn design_video_controls(&self, cx: &Context<Self>) -> AnyElement {
        let editing = self
            .selected
            .is_some_and(|id| self.editor.doc.design.media.contains_key(&id));
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                Button::new("design-youtube-add")
                    .label("Embed YouTube video…")
                    .small()
                    .outline()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.design_video_dialog(false, window, cx)
                    })),
            )
            .when(editing, |d| {
                d.child(
                    Button::new("design-youtube-edit")
                        .label("Edit video link…")
                        .small()
                        .outline()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.design_video_dialog(true, window, cx)
                        })),
                )
                .child(
                    Button::new("design-youtube-detach")
                        .label("Keep poster only")
                        .small()
                        .ghost()
                        .on_click(cx.listener(|this, _, _, cx| {
                            if !this.prepare_page_action(cx) {
                                return;
                            }
                            if let Some(id) = this.selected {
                                match media::detach_youtube(&mut this.editor, id) {
                                    Ok(()) => this.after_change(cx),
                                    Err(error) => this.set_status(error, true, cx),
                                }
                            }
                        })),
                )
            })
            .into_any_element()
    }
    fn design_video_dialog(&mut self, editing: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let id = editing.then_some(self.selected).flatten();
        let initial = id
            .and_then(|id| self.editor.doc.design.media.get(&id))
            .map(|video| video.url())
            .unwrap_or_default();
        let input = cx.new(|cx| InputState::new(window, cx).default_value(initial));
        let owner = cx.weak_entity();
        let ticket = self.edit_ticket();
        window.open_dialog(cx,move|dialog,_,_|{
            let input=input.clone();let owner=owner.clone();
            dialog.title("YouTube video").width(px(480.))
                .child(div().flex().flex_col().gap_2().child("YouTube link")
                    .child(Input::new(&input).id("design-youtube-url"))
                    .child("The video plays inside Emulsion during presentation. Playback needs internet access. Images and PDF exports keep the poster."))
                .footer(crate::widgets::form_dialog_footer(if editing {"Update video"} else {"Insert video"}))
                .on_ok(move|_,_,cx|{
                    let url=input.read(cx).value().to_string();
                    owner.update(cx,|this,cx|{
                        if this.edit_ticket()!=ticket {this.set_status("The page changed. Open the video dialog again.",true,cx);return false;}
                        let result=if let Some(id)=id {media::update_youtube(&mut this.editor,id,&url).map(|_|id)} else {
                            let w=f64::from(this.editor.doc.width).min(640.);
                            let h=w*9./16.;
                            let origin=((f64::from(this.editor.doc.width)-w)/2.,(f64::from(this.editor.doc.height)-h)/2.);
                            media::insert_youtube(&mut this.editor,&url,origin,(w,h))
                        };
                        match result {
                            Ok(id)=>{this.set_layer_selection(vec![id],Some(id));this.after_change(cx);this.set_tool(Tool::Move,cx);true}
                            Err(error)=>{this.set_status(error,true,cx);false}
                        }
                    }).unwrap_or(false)
                })
        });
    }

    fn video_screen_bounds(&self, id: NodeId) -> Option<Bounds<Pixels>> {
        if self.view.rotation.rem_euclid(360.) != 0. {
            return None;
        }
        let doc = self.motion.preview.as_ref().unwrap_or(&self.editor.doc);
        let (x, y, w, h) = media::bounds(doc, id)?;
        let canvas = self.canvas_bounds()?;
        let p = self.view.doc_to_screen((x, y), &canvas);
        let q = self.view.doc_to_screen((x + w, y + h), &canvas);
        Some(Bounds::new(
            point(px(p.0.min(q.0) as f32), px(p.1.min(q.1) as f32)),
            size(px((q.0 - p.0).abs() as f32), px((q.1 - p.1).abs() as f32)),
        ))
    }
    fn video_fits_canvas(&self, bounds: Bounds<Pixels>) -> bool {
        self.canvas_bounds().is_some_and(|canvas| {
            bounds.origin.x >= canvas.origin.x
                && bounds.origin.y >= canvas.origin.y
                && bounds.origin.x + bounds.size.width <= canvas.origin.x + canvas.size.width
                && bounds.origin.y + bounds.size.height <= canvas.origin.y + canvas.size.height
        })
    }
    fn play_design_video(&mut self, id: NodeId, window: &mut Window, cx: &mut Context<Self>) {
        self.stop_design_video(cx);
        let Some(video) = self.editor.doc.design.media.get(&id) else {
            return;
        };
        let Some(bounds) = self.video_screen_bounds(id) else {
            return;
        };
        if bounds.size.width < px(200.) || bounds.size.height < px(200.) {
            self.set_status(
                "Enlarge the video or presentation window to play it (minimum 200 × 200 px).",
                true,
                cx,
            );
            return;
        }
        if !self.video_fits_canvas(bounds) {
            self.set_status(
                "Fit the whole video inside the presentation canvas to play it.",
                true,
                cx,
            );
            return;
        }
        let result = (|| {
            let server = emulsion_io::design_media::PlayerServer::start(video)?;
            let player = crate::web_player::Player::new(server.url(), bounds, window, cx)?;
            Ok::<_, anyhow::Error>(ActiveVideo {
                player,
                _server: server,
                id,
                image: None,
                window: window.window_handle(),
            })
        })();
        match result {
            Ok(active) => {
                self.status = None;
                self.video.active = Some(active);
                let focus = self
                    .video
                    .focus
                    .get_or_insert_with(|| cx.focus_handle())
                    .clone();
                self.video.task = Some(cx.spawn(async move |this, cx| {
                    loop {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(33))
                            .await;
                        let more = this
                            .update(cx, |this, cx| {
                                if !this.motion.presenting
                                    || !this.visible
                                    || !this.design_video_playing()
                                {
                                    return false;
                                }
                                this.notify_canvas(cx);
                                true
                            })
                            .unwrap_or(false);
                        if !more {
                            break;
                        }
                    }
                }));
                window.focus(&focus, cx);
                self.notify_canvas(cx);
                cx.notify();
            }
            Err(error) => self.set_status(format!("Video player unavailable: {error}"), true, cx),
        }
    }

    pub(super) fn design_video_overlays(
        &mut self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.motion.presenting {
            return None;
        }
        let canvas = self.canvas_bounds()?;
        if let Some(error) = self
            .video
            .active
            .as_ref()
            .and_then(|active| active.player.error())
        {
            self.stop_design_video(cx);
            self.set_status(format!("Video player unavailable: {error}"), true, cx);
        }
        let videos: Vec<_> = self
            .editor
            .doc
            .design
            .media
            .keys()
            .filter_map(|id| self.video_screen_bounds(*id).map(|b| (*id, b)))
            .collect();
        if self
            .video
            .active
            .as_ref()
            .is_some_and(|active| !videos.iter().any(|(id, _)| *id == active.id))
        {
            self.stop_design_video(cx);
        }
        if self.video.active.as_ref().is_some_and(|active| {
            videos
                .iter()
                .any(|(id, bounds)| *id == active.id && !self.video_fits_canvas(*bounds))
        }) {
            self.stop_design_video(cx);
            self.set_status(
                "Fit the whole video inside the presentation canvas to play it.",
                true,
                cx,
            );
        }
        if self.video.active.as_ref().is_some_and(|active| {
            videos.iter().any(|(id, bounds)| {
                *id == active.id && (bounds.size.width < px(200.) || bounds.size.height < px(200.))
            })
        }) {
            self.stop_design_video(cx);
            self.set_status(
                "Enlarge the video or presentation window to play it (minimum 200 × 200 px).",
                true,
                cx,
            );
        }
        let mut overlay = div().absolute().size_full();
        for (id, bounds) in videos {
            let origin = bounds.origin;
            let mut player = div()
                .id(("design-video", id as usize))
                .test_support()
                .absolute()
                .left(bounds.origin.x - canvas.origin.x)
                .top(bounds.origin.y - canvas.origin.y)
                .w(bounds.size.width)
                .h(bounds.size.height)
                .overflow_hidden();
            if let Some(active) = self.video.active.as_mut().filter(|active| active.id == id) {
                active.player.update_bounds(bounds);
                if let Some(image) = active.player.latest_frame()
                    && active
                        .image
                        .as_ref()
                        .is_none_or(|old| !Arc::ptr_eq(old, &image))
                    && let Some(old) = active.image.replace(image)
                {
                    let _ = window.drop_image(old);
                }
                player = player
                    .bg(rgb(0x111111))
                    .occlude()
                    .when_some(self.video.focus.as_ref(), |d, focus| {
                        d.track_focus(focus).key_context("EmbeddedVideo")
                    })
                    .when_some(active.image.clone(), |d, image| {
                        d.child(img(image).size_full())
                    })
                    .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, _| {
                        if let Some(active) = &mut this.video.active {
                            active.player.pointer_move(
                                f32::from(event.position.x - origin.x),
                                f32::from(event.position.y - origin.y),
                            );
                        }
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            if let Some(active) = &mut this.video.active {
                                active.player.pointer_button(
                                    1,
                                    true,
                                    f32::from(event.position.x - origin.x),
                                    f32::from(event.position.y - origin.y),
                                );
                            }
                            cx.stop_propagation();
                        }),
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseUpEvent, _, cx| {
                            if let Some(active) = &mut this.video.active {
                                active.player.pointer_button(
                                    1,
                                    false,
                                    f32::from(event.position.x - origin.x),
                                    f32::from(event.position.y - origin.y),
                                );
                            }
                            cx.stop_propagation();
                        }),
                    )
                    .on_scroll_wheel(cx.listener(move |this, event: &ScrollWheelEvent, _, cx| {
                        let delta = event.delta.pixel_delta(px(20.));
                        if let Some(active) = &this.video.active {
                            active.player.scroll(
                                f32::from(delta.x),
                                f32::from(delta.y),
                                f32::from(event.position.x - origin.x),
                                f32::from(event.position.y - origin.y),
                            );
                        }
                        cx.stop_propagation();
                    }))
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                        if event.keystroke.key == "escape" {
                            this.stop_design_video(cx);
                            window.focus(&this.canvas_focus, cx);
                        } else if let Some(active) = &this.video.active {
                            let m = event.keystroke.modifiers;
                            let modifiers = u32::from(m.shift)
                                | (u32::from(m.control) * 4)
                                | (u32::from(m.alt) * 8)
                                | (u32::from(m.platform) * 64);
                            active.player.key(&event.keystroke.key, true, modifiers);
                        }
                        cx.stop_propagation();
                    }))
                    .on_key_up(cx.listener(|this, event: &KeyUpEvent, _, cx| {
                        if let Some(active) = &this.video.active {
                            active.player.key(&event.keystroke.key, false, 0);
                        }
                        cx.stop_propagation();
                    }));
            } else {
                player = player.flex().items_center().justify_center().child(
                    Button::new(("design-video-play", id as usize))
                        .label("Play YouTube video")
                        .bg(p.panel)
                        .text_color(p.ink)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.play_design_video(id, window, cx)
                        })),
                );
            }
            overlay = overlay.child(player);
        }
        Some(overlay.into_any_element())
    }
}
