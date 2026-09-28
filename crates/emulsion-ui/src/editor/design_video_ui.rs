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
    pub(super) fn design_media_runtime_state(&self) -> serde_json::Value {
        self.video.active.as_ref().map(|active|serde_json::json!({"node":active.id,"playback":active._server.playback_state(),"runtime_error":active.player.error()})).unwrap_or(serde_json::Value::Null)
    }
    pub(super) fn design_media_host_play(
        &mut self,
        id: NodeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if !self.motion.presenting || !self.presentation_media_visible(id) {
            return Err(
                "Start presentation and choose visible media outside blocked overlays.".into(),
            );
        }
        if let Some(active) = self.video.active.as_ref().filter(|a| a.id == id) {
            return active
                ._server
                .command(emulsion_io::design_media::PlaybackCommand::Play);
        }
        self.play_design_video(id, window, cx);
        let active=self.video.active.as_ref().filter(|a|a.id==id).ok_or("Could not start media. Check the playback runtime and ensure its whole frame is at least 200 × 200 screen pixels.")?;
        active
            ._server
            .command(emulsion_io::design_media::PlaybackCommand::Play)
    }
    pub(super) fn design_media_host_command(
        &mut self,
        command: emulsion_io::design_media::PlaybackCommand,
    ) -> Result<(), String> {
        if !self.motion.presenting {
            return Err("Start a presentation before controlling media.".into());
        }
        let active = self
            .video
            .active
            .as_ref()
            .ok_or("No active media player. Play an object first.")?;
        if let emulsion_io::design_media::PlaybackCommand::Seek { position_ms } = command {
            let doc = self.motion.preview.as_ref().unwrap_or(&self.editor.doc);
            if let Some(media) = doc.design.local_media.get(&active.id)
                && (position_ms < media.trim_start_ms
                    || media.trim_end_ms.is_some_and(|end| position_ms > end))
            {
                return Err(
                    "Seek position must remain inside the local media trim interval.".into(),
                );
            }
        }
        active._server.command(command)
    }

    pub(super) fn design_video_playing(&self) -> bool {
        self.video.active.is_some()
    }
    pub(super) fn stop_design_video(&mut self, cx: &mut Context<Self>) {
        self.video.task = None;
        if let Some(active) = self.video.active.take() {
            let image = active.image.clone();
            let handle = active.window;
            let player_focus = self.video.focus.clone();
            let canvas_focus = self.canvas_focus.clone();
            cx.defer(move |cx| {
                cx.update_window(handle, |_, window, cx| {
                    if let Some(image) = image {
                        let _ = window.drop_image(image);
                    }
                    if player_focus.is_some_and(|focus| focus.is_focused(window)) {
                        window.focus(&canvas_focus, cx);
                    }
                })
                .ok();
            });
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
            .child(
                Button::new("design-local-media-add")
                    .label("Import video or audio…")
                    .small()
                    .outline()
                    .on_click(cx.listener(|this, _, _, cx| this.import_design_media(cx))),
            )
            .when(
                self.selected
                    .is_some_and(|id| self.editor.doc.design.local_media.contains_key(&id)),
                |d| {
                    d.child(
                        Button::new("design-local-media-edit")
                            .label("Trim and playback…")
                            .small()
                            .outline()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.local_media_dialog(window, cx)
                            })),
                    )
                    .child(
                        Button::new("design-local-media-detach")
                            .label("Keep poster only")
                            .small()
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| {
                                if !this.prepare_page_action(cx) {
                                    return;
                                }
                                if let Some(id) = this.selected {
                                    match media::detach_local(&mut this.editor, id) {
                                        Ok(()) => this.after_change(cx),
                                        Err(e) => this.set_status(e, true, cx),
                                    }
                                }
                            })),
                    )
                },
            )
            .child(
                Button::new("design-playback-setup")
                    .label("Video playback setup…")
                    .small()
                    .ghost()
                    .on_click(
                        cx.listener(|this, _, window, cx| this.playback_setup_dialog(window, cx)),
                    ),
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
        if !self.presentation_media_visible(id) {
            return;
        }
        let doc = self.motion.preview.as_ref().unwrap_or(&self.editor.doc);
        if !doc.design.media.contains_key(&id) && !doc.design.local_media.contains_key(&id) {
            return;
        }
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
            let server = if let Some(video) = doc.design.media.get(&id) {
                emulsion_io::design_media::PlayerServer::start(video)?
            } else {
                emulsion_io::design_media::PlayerServer::start_local(&doc.design.local_media[&id])?
            };
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
                            .update(cx, |this, cx| this.poll_design_video(cx))
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

    // Lifecycle changes notify CanvasView. Run them from the polling task,
    // never while CanvasView is borrowed to render the player overlay.
    fn poll_design_video(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.motion.presenting || !self.visible {
            self.stop_design_video(cx);
            return false;
        }
        let Some(active) = self.video.active.as_ref() else {
            return false;
        };
        if active._server.finished() {
            self.stop_design_video(cx);
            self.resume_presentation_advance(cx);
            return false;
        }
        if !self.presentation_media_visible(active.id) {
            self.stop_design_video(cx);
            return false;
        }
        if let Some(error) = active.player.error() {
            self.stop_design_video(cx);
            self.set_status(format!("Video player unavailable: {error}"), true, cx);
            return false;
        }
        let Some(bounds) = self.video_screen_bounds(active.id) else {
            self.stop_design_video(cx);
            return false;
        };
        if !self.video_fits_canvas(bounds) {
            self.stop_design_video(cx);
            self.set_status(
                "Fit the whole video inside the presentation canvas to play it.",
                true,
                cx,
            );
            return false;
        }
        if bounds.size.width < px(200.) || bounds.size.height < px(200.) {
            self.stop_design_video(cx);
            self.set_status(
                "Enlarge the video or presentation window to play it (minimum 200 × 200 px).",
                true,
                cx,
            );
            return false;
        }
        self.notify_canvas(cx);
        true
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
        let doc = self.motion.preview.as_ref().unwrap_or(&self.editor.doc);
        let videos: Vec<_> = doc
            .design
            .media
            .keys()
            .chain(doc.design.local_media.keys())
            .filter(|id| self.presentation_media_visible(**id))
            .filter_map(|id| self.video_screen_bounds(*id).map(|b| (*id, b)))
            .collect();
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
                        .label(
                            if self
                                .editor
                                .doc
                                .design
                                .local_media
                                .get(&id)
                                .is_some_and(|m| m.kind == media::LocalMediaKind::Audio)
                            {
                                "Play audio"
                            } else if self.editor.doc.design.local_media.contains_key(&id) {
                                "Play video"
                            } else {
                                "Play YouTube video"
                            },
                        )
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

#[path = "design_local_media_ui.rs"]
mod local;

#[cfg(all(test, target_os = "linux"))]
mod host_control_tests {
    use super::*;
    use ::core::prelude::v1::test;
    use emulsion_mcp::design_motion_tools::HostAction;
    use gpui::TestAppContext;
    #[gpui_kit::test]
    fn presentation_host_media_commands_are_bounded_and_nonmutating(cx: &mut TestAppContext) {
        use std::io::{Read, Write};
        let _helper =
            crate::web_player::test_helper(b"#!/bin/sh\nwhile read command; do :; done\n");
        let mut editor = emulsion_core::Editor::new(Document::new(800, 600), None);
        let media =
            media::LocalMedia::from_bytes("tone.wav".into(), b"RIFF\0\0\0\0WAVEdata".to_vec())
                .unwrap();
        let id = media::insert_local(&mut editor, media, (80., 80.), (640., 360.)).unwrap();
        media::update_local(&mut editor, id, 100, Some(1000), 1., false).unwrap();
        let original = editor.doc.clone();
        let (workspace, cx) = crate::tests::open(cx, editor.doc);
        cx.simulate_resize(size(px(1440.), px(1000.)));
        let view = cx.update(|window, cx| {
            let view = workspace.read(cx).editor.clone().unwrap();
            view.update(cx, |this, cx| {
                this.presentation_host_action(
                    HostAction::Start {
                        fullscreen: true,
                        presenter: false,
                        auto_advance: false,
                    },
                    window,
                    cx,
                )
                .unwrap()
            });
            view
        });
        cx.run_until_parked();
        cx.update(|window,cx|view.update(cx,|this,cx|{
            let state=this.presentation_host_action(HostAction::MediaPlay(id),window,cx).unwrap();assert_eq!(state["media"]["node"],id);
            this.presentation_host_action(HostAction::MediaPause,window,cx).unwrap();
            assert!(this.presentation_host_action(HostAction::MediaSeek(500),window,cx).is_err());
            let url=this.video.active.as_ref().unwrap()._server.url();let (authority,path)=url.strip_prefix("http://").unwrap().split_once('/').unwrap();
            let mut stream=std::net::TcpStream::connect(authority).unwrap();stream.set_read_timeout(Some(std::time::Duration::from_secs(2))).unwrap();
            write!(stream,"GET /{path}/control?ready=1&paused=1&position_ms=100&duration_ms=2000&error=0 HTTP/1.1\r\nHost: {authority}\r\n\r\n").unwrap();let mut response=String::new();stream.read_to_string(&mut response).unwrap();assert!(response.contains("pause"));
            let state=this.presentation_host_action(HostAction::MediaSeek(500),window,cx).unwrap();assert_eq!(state["media"]["playback"]["ready"],true);assert_eq!(state["media"]["playback"]["pending_commands"],1);
            assert!(this.presentation_host_action(HostAction::MediaSeek(50),window,cx).is_err());assert!(this.presentation_host_action(HostAction::MediaSeek(1001),window,cx).is_err());
            this.presentation_host_action(HostAction::MediaStop,window,cx).unwrap();assert!(!this.design_video_playing());
            this.presentation_host_action(HostAction::End,window,cx).unwrap();assert_eq!(this.editor.doc,original);
        }));
    }
}
