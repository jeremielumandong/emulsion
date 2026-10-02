//! Drawing the Timeline dock: toolbar, ruler, the panel track, audio tracks
//! with clips and markers, the playhead, play range, scroll bar and the
//! duration overlay. Pointer gestures start here and run through
//! `timeline_begin` / `timeline_move` / `timeline_end`.
use super::super::storyboard_audio_library::{DraggedSound, waveform};
use super::*;
use emulsion_core::storyboard::KeyframeSync;
use emulsion_core::timeline::{Edge, TransitionKind};
use gpui_kit::component::{
    Disableable, Selectable, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenu, PopupMenuItem},
};

pub(super) const HEADER_W: f32 = 176.;
const RULER_H: f32 = 30.;
const PANEL_H: f32 = 76.;
const TRACK_H: f32 = 54.;
const EDGE_W: f32 = 7.;

/// A scene's tint on the panel track; neighbouring scenes differ.
fn scene_tint(index: usize) -> Hsla {
    let hue = (index as f32 * 0.161).fract();
    hsla(hue, 0.55, 0.55, 0.22)
}

/// Tick spacing in frames for the ruler at `zoom`: labels stay at least
/// 72 px apart, on whole frames, seconds or minutes.
fn ruler_step(zoom: f32, rate: FrameRate) -> u64 {
    let s = rate.timebase().max(1);
    [
        1,
        2,
        5,
        10,
        s / 2,
        s,
        2 * s,
        5 * s,
        10 * s,
        30 * s,
        60 * s,
        300 * s,
        600 * s,
        1800 * s,
        3600 * s,
    ]
    .into_iter()
    .filter(|step| *step > 0)
    .find(|step| *step as f32 * zoom >= 72.)
    .unwrap_or(3600 * s)
}

fn menu_item(
    owner: &WeakEntity<EditorView>,
    label: impl Into<SharedString>,
    run: impl Fn(&mut EditorView, &mut Window, &mut Context<EditorView>) + 'static,
) -> PopupMenuItem {
    let owner = owner.clone();
    PopupMenuItem::new(label).on_click(move |_, window, cx| {
        owner.update(cx, |e, cx| run(e, window, cx)).ok();
    })
}

const EDGES: [(Edge, &str); 4] = [
    (Edge::Left, "left"),
    (Edge::Right, "right"),
    (Edge::Top, "top"),
    (Edge::Bottom, "bottom"),
];

impl EditorView {
    /// A dialog with one text field; `commit` returns whether to close.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn timeline_text_dialog(
        &mut self,
        title: &'static str,
        label: &'static str,
        value: String,
        confirm: &'static str,
        commit: impl Fn(&mut EditorView, String, &mut Context<EditorView>) -> bool + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| InputState::new(window, cx).default_value(value));
        let owner = cx.weak_entity();
        let commit = Rc::new(commit);
        window.open_dialog(cx, move |dialog, _, _| {
            let (input, owner, commit) = (input.clone(), owner.clone(), commit.clone());
            dialog
                .title(title)
                .width(px(380.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(label)
                        .child(Input::new(&input)),
                )
                .footer(crate::widgets::form_dialog_footer(confirm))
                .on_ok(move |_, _, cx| {
                    let text = input.read(cx).value().to_string();
                    owner
                        .update(cx, |this, cx| commit(this, text, cx))
                        .unwrap_or(false)
                })
        });
    }

    pub(crate) fn timeline_duration_dialog(
        &mut self,
        panel: PageId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(frames) = self
            .editor
            .storyboard()
            .and_then(|b| b.panels.get(&panel))
            .map(|p| p.frames)
        else {
            return;
        };
        let value = self.timeline_rate().timecode(u64::from(frames));
        self.timeline_text_dialog(
            "Set duration",
            "Duration: frames (36), seconds (1.5s) or timecode (00:00:01:12)",
            value,
            "Set",
            move |this, text, cx| this.timeline_set_duration(panel, &text, cx),
            window,
            cx,
        );
    }

    pub(crate) fn timeline_fit_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(board) = self.editor.storyboard() else {
            return;
        };
        let first = self.board_selection()[0];
        let panels = self.timeline_scale_set(first);
        let total: u64 = panels
            .iter()
            .filter_map(|id| board.panels.get(id))
            .map(|p| u64::from(p.frames))
            .sum();
        let value = board.settings.frame_rate.timecode(total);
        self.timeline_text_dialog(
            "Fit selection to duration",
            "Total duration for the selected panels, kept in proportion",
            value,
            "Fit",
            |this, text, cx| this.timeline_fit_selection(&text, cx),
            window,
            cx,
        );
    }

    fn timeline_transition_length_dialog(
        &mut self,
        panel: PageId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(transition) = self
            .editor
            .storyboard()
            .and_then(|b| b.panels.get(&panel))
            .map(|p| p.transition)
        else {
            return;
        };
        self.timeline_text_dialog(
            "Transition length",
            "Length: frames, seconds (0.5s) or timecode",
            transition.frames.to_string(),
            "Set",
            move |this, text, cx| {
                let rate = this.timeline_rate();
                match parse_duration(&text, rate).and_then(|f| u32::try_from(f).ok()) {
                    Some(frames) => this.timeline_set_transition(
                        panel,
                        Transition {
                            frames,
                            ..transition
                        },
                        cx,
                    ),
                    None => {
                        this.set_status("Type a length in frames, seconds or timecode.", true, cx);
                        false
                    }
                }
            },
            window,
            cx,
        );
    }

    fn timeline_fade_colour_dialog(
        &mut self,
        panel: PageId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = match self
            .editor
            .storyboard()
            .and_then(|b| b.panels.get(&panel))
            .map(|p| p.transition.kind)
        {
            Some(TransitionKind::FadeToColor { color }) => color,
            _ => [0, 0, 0],
        };
        self.timeline_text_dialog(
            "Fade to colour",
            "Colour as hex, such as #000000",
            format!("#{:02X}{:02X}{:02X}", current[0], current[1], current[2]),
            "Set",
            move |this, text, cx| {
                let hex = text.trim().trim_start_matches('#');
                let color = (hex.len() == 6)
                    .then(|| u32::from_str_radix(hex, 16).ok())
                    .flatten()
                    .map(|v| [(v >> 16) as u8, (v >> 8) as u8, v as u8]);
                match color {
                    Some(color) => {
                        this.timeline_transition_kind(
                            panel,
                            TransitionKind::FadeToColor { color },
                            cx,
                        );
                        true
                    }
                    None => {
                        this.set_status(
                            "Type a colour as six hex digits, such as #1A1A1A.",
                            true,
                            cx,
                        );
                        false
                    }
                }
            },
            window,
            cx,
        );
    }

    /// The transitions menu at the cut into `panel`.
    fn timeline_transition_menu(
        mut menu: PopupMenu,
        owner: WeakEntity<Self>,
        panel: PageId,
        current: Transition,
        window: &mut Window,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let kind = current.kind;
        let pick = |label: &str, k: TransitionKind| {
            menu_item(&owner, label.to_string(), move |e, _, cx| {
                e.timeline_transition_kind(panel, k, cx)
            })
            .checked(kind == k && (k == TransitionKind::Cut || !current.is_cut()))
        };
        menu = menu
            .item(pick("Cut", TransitionKind::Cut))
            .item(pick("Dissolve", TransitionKind::Dissolve));
        for (label, slide) in [("Wipe", false), ("Slide", true)] {
            let owner = owner.clone();
            menu = menu.submenu(label, window, cx, move |mut sub, _, _| {
                for (edge, name) in EDGES {
                    let k = if slide {
                        TransitionKind::Slide { from: edge }
                    } else {
                        TransitionKind::Wipe { from: edge }
                    };
                    sub = sub.item(
                        menu_item(&owner, format!("From the {name}"), move |e, _, cx| {
                            e.timeline_transition_kind(panel, k, cx)
                        })
                        .checked(kind == k),
                    );
                }
                sub
            });
        }
        menu = menu
            .item(pick("Clock wipe", TransitionKind::Clock))
            .item(pick("Iris", TransitionKind::Iris))
            .item(pick(
                "Fade to black",
                TransitionKind::FadeToColor { color: [0, 0, 0] },
            ))
            .item(pick(
                "Fade to white",
                TransitionKind::FadeToColor {
                    color: [255, 255, 255],
                },
            ))
            .item(menu_item(
                &owner,
                "Fade to colour…",
                move |e, window, cx| e.timeline_fade_colour_dialog(panel, window, cx),
            ))
            .separator()
            .item(
                menu_item(
                    &owner,
                    format!("Length… ({} f)", current.frames),
                    move |e, window, cx| e.timeline_transition_length_dialog(panel, window, cx),
                )
                .disabled(kind == TransitionKind::Cut),
            );
        menu
    }

    fn timeline_panel_menu(menu: PopupMenu, owner: WeakEntity<Self>, panel: PageId) -> PopupMenu {
        menu.item(menu_item(
            &owner,
            "Set duration…",
            move |e, window, cx| e.timeline_duration_dialog(panel, window, cx),
        ))
        .item(menu_item(
            &owner,
            "Fit selection to duration…",
            |e, window, cx| e.timeline_fit_dialog(window, cx),
        ))
        .item(menu_item(&owner, "Snap cuts to markers", |e, _, cx| {
            e.timeline_snap_cuts(cx)
        }))
        .separator()
        .item(menu_item(
            &owner,
            "Open on the Stage",
            move |e, window, cx| e.board_click(panel, Modifiers::default(), 2, window, cx),
        ))
    }

    fn timeline_track_menu(
        menu: PopupMenu,
        owner: WeakEntity<Self>,
        track: usize,
        name: String,
    ) -> PopupMenu {
        menu.item(menu_item(
            &owner,
            "Rename track…",
            move |e, window, cx| {
                e.timeline_text_dialog(
                    "Rename track",
                    "Name",
                    name.clone(),
                    "Rename",
                    move |this, text, cx| this.timeline_rename_track(track, &text, cx),
                    window,
                    cx,
                )
            },
        ))
        .item(menu_item(
            &owner,
            "Add marker at playhead",
            move |e, _, cx| e.timeline_add_marker(Some(track), cx),
        ))
        .separator()
        .item(menu_item(&owner, "Delete track", move |e, _, cx| {
            e.timeline_delete_track(track, cx)
        }))
    }

    fn timeline_clip_menu(
        menu: PopupMenu,
        owner: WeakEntity<Self>,
        at: (usize, usize),
        clip: AudioClip,
    ) -> PopupMenu {
        let name = clip.name.clone();
        menu.item(menu_item(&owner, "Rename clip…", move |e, window, cx| {
            e.timeline_text_dialog(
                "Rename clip",
                "Name",
                name.clone(),
                "Rename",
                move |this, text, cx| this.timeline_rename_clip(at, &text, cx),
                window,
                cx,
            )
        }))
        .item(menu_item(
            &owner,
            format!("Gain… ({:+.1} dB)", clip.gain_db),
            move |e, window, cx| {
                e.timeline_text_dialog(
                    "Clip gain",
                    "Gain in dB (−60 to +24)",
                    format!("{:.1}", clip.gain_db),
                    "Set",
                    move |this, text, cx| this.timeline_clip_gain(at, &text, cx),
                    window,
                    cx,
                )
            },
        ))
        .item(menu_item(&owner, "Show in library", move |e, _, cx| {
            e.timeline_ui.library.open = true;
            e.library_select_sound(clip.asset, cx);
        }))
        .separator()
        .item(menu_item(&owner, "Delete clip", move |e, _, cx| {
            e.timeline_delete_clip(at, cx)
        }))
    }

    fn timeline_marker_menu(
        menu: PopupMenu,
        owner: WeakEntity<Self>,
        at: (usize, usize),
        name: String,
    ) -> PopupMenu {
        menu.item(menu_item(
            &owner,
            "Rename marker…",
            move |e, window, cx| {
                e.timeline_text_dialog(
                    "Rename marker",
                    "Name",
                    name.clone(),
                    "Rename",
                    move |this, text, cx| this.timeline_rename_marker(at, &text, cx),
                    window,
                    cx,
                )
            },
        ))
        .item(menu_item(&owner, "Delete marker", move |e, _, cx| {
            e.timeline_delete_marker(at, cx)
        }))
    }

    /// Clicking a panel selects it, as on the Board; the canvas keeps focus
    /// on the Stage.
    fn timeline_panel_click(
        &mut self,
        id: PageId,
        modifiers: Modifiers,
        clicks: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.board_click(id, modifiers, clicks, window, cx);
        let focus = if self.board_open() {
            self.board_focus(cx)
        } else {
            self.timeline_focus(cx)
        };
        window.focus(&focus, cx);
    }

    /// The strip's Timeline switch.
    pub(in crate::editor) fn storyboard_timeline_toggle(&self, cx: &Context<Self>) -> AnyElement {
        Button::new("storyboard-timeline-toggle")
            .label("Timeline")
            .tooltip("Show the timeline under the Stage or Board (Ctrl+Alt+T)")
            .small()
            .outline()
            .selected(self.timeline_open())
            .on_click(cx.listener(|this, _, _, cx| this.toggle_storyboard_timeline(cx)))
            .into_any_element()
    }

    /// While playing, page the view along so the playhead stays in sight.
    fn timeline_follow_playhead(&mut self) {
        if !self.transport.playing {
            return;
        }
        let width = self.timeline_lane_width();
        let x = self.transport.frame as f32 * self.timeline_ui.zoom - self.timeline_ui.scroll;
        if x < 0. || x > width - 16. {
            self.timeline_ui.scroll =
                (self.transport.frame as f32 * self.timeline_ui.zoom - 16.).max(0.);
        }
    }

    /// The Timeline dock, when open on a storyboard.
    pub(in crate::editor) fn storyboard_timeline(
        &mut self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.timeline_open() {
            return None;
        }
        let focus = self.timeline_focus(cx);
        self.timeline_follow_playhead();
        self.clamp_timeline_scroll();
        let toolbar = self.timeline_toolbar(p, cx);
        let ruler = self.timeline_ruler(p, cx);
        let panels = self.timeline_panel_row(p, cx);
        let keys = self.timeline_key_rows(p, cx);
        let camera = self.timeline_camera_row(p, HEADER_W, TRACK_H * 0.6, cx);
        let tracks = self.timeline_track_rows(p, cx);
        let scrollbar = self.timeline_scrollbar(p, cx);
        let library = self
            .timeline_ui
            .library
            .open
            .then(|| self.timeline_library(p, window, cx));
        let height = self.timeline_ui.height;
        Some(
            div()
                .id("storyboard-timeline")
                .test_support()
                .track_focus(&focus)
                // The Board's keys: the transport shortcuts, Delete and M.
                .key_context("NodePanel")
                .map(|d| Self::playback_actions(d, cx))
                .on_action(cx.listener(|this, _: &crate::actions::DeleteNode, _, cx| {
                    this.timeline_delete_selected(cx)
                }))
                .on_action(
                    cx.listener(|this, _: &crate::actions::AddTimelineMarker, _, cx| {
                        this.timeline_add_marker(None, cx)
                    }),
                )
                .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                    if this.timeline_key(&e.keystroke.key, e.keystroke.modifiers, cx) {
                        cx.stop_propagation();
                    }
                }))
                .flex()
                .flex_col()
                .flex_none()
                .h(px(height))
                .bg(p.panel)
                .border_t_1()
                .border_color(p.line)
                .text_color(p.ink)
                .child(
                    // The top edge resizes the dock.
                    div()
                        .id("timeline-resize")
                        .h(px(4.))
                        .w_full()
                        .cursor(CursorStyle::ResizeUpDown)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                                let drag = TimelineDrag::Resize {
                                    y: f32::from(e.position.y),
                                    height: this.timeline_ui.height,
                                };
                                this.timeline_begin(drag, e.position, e.modifiers, window, cx);
                                cx.stop_propagation();
                            }),
                        ),
                )
                .child(toolbar)
                .child(
                    div()
                        .flex()
                        .flex_1()
                        .min_h_0()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w_0()
                                .min_h_0()
                                .child(ruler)
                                .child(
                                    div()
                                        .id("timeline-rows")
                                        .flex()
                                        .flex_col()
                                        .flex_1()
                                        .min_h_0()
                                        .overflow_y_scroll()
                                        .child(panels)
                                        .children(camera)
                                        .children(keys)
                                        .children(tracks),
                                )
                                .child(scrollbar),
                        )
                        .children(library),
                )
                .into_any_element(),
        )
    }

    fn timeline_toolbar(&mut self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        // The player's transport (play, steps, timecode, loop, play range).
        let transport = self.transport_bar(p, cx);
        let ui = &self.timeline_ui;
        let total = self.timeline_length();
        let owner = cx.weak_entity();
        let button = |id: &'static str, label: &str, tooltip: &'static str| {
            Button::new(id)
                .label(label.to_string())
                .tooltip(tooltip)
                .xsmall()
                .ghost()
        };
        let readout = format!("frame {} / {}", self.transport.frame, total);
        let sync = self
            .editor
            .storyboard()
            .map_or(KeyframeSync::Scale, |b| b.keyframe_sync);
        let tracks = self
            .editor
            .storyboard()
            .map_or(0, |b| b.timeline.tracks.len());
        div()
            .id("timeline-toolbar")
            .test_support()
            .flex()
            .flex_none()
            .flex_wrap()
            .items_center()
            .gap_1()
            .px_2()
            .pb_1()
            .border_b_1()
            .border_color(p.line)
            .child(
                div()
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Timeline"),
            )
            .child(transport)
            // The transport counts timecode; in frames mode, frames too.
            .when(!ui.timecode, |bar| {
                bar.child(
                    div()
                        .id("timeline-readout")
                        .test_support()
                        .px_1()
                        .font_family(MONO_FONT)
                        .text_size(px(11.))
                        .text_color(p.muted)
                        .child(readout),
                )
            })
            .child(
                button(
                    "timeline-units",
                    if ui.timecode { "Timecode" } else { "Frames" },
                    "Count the ruler in timecode or frames",
                )
                .outline()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.timeline_ui.timecode = !this.timeline_ui.timecode;
                    cx.notify();
                })),
            )
            .child(
                button("timeline-zoom-out", "−", "Zoom out (Ctrl+scroll)").on_click(
                    cx.listener(|this, _, _, cx| this.timeline_zoom_by(1. / 1.5, None, cx)),
                ),
            )
            .child(
                button("timeline-zoom-in", "+", "Zoom in (Ctrl+scroll)")
                    .on_click(cx.listener(|this, _, _, cx| this.timeline_zoom_by(1.5, None, cx))),
            )
            .child(
                button("timeline-zoom-fit", "Fit", "Show the whole timeline")
                    .on_click(cx.listener(|this, _, _, cx| this.timeline_zoom_fit(cx))),
            )
            .child(
                button(
                    "timeline-snap",
                    "Snap",
                    "Snap drags to cuts, markers and the playhead (hold Ctrl to drag freely)",
                )
                .selected(ui.snap)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.timeline_ui.snap = !this.timeline_ui.snap;
                    cx.notify();
                })),
            )
            .child(div().w(px(8.)))
            .child(
                Button::new("timeline-timing")
                    .label("Timing ▾")
                    .xsmall()
                    .ghost()
                    .dropdown_menu(move |menu, _, _| {
                        menu.item(menu_item(
                            &owner,
                            "Set duration of the active panel…",
                            |e, window, cx| {
                                let id = e.editor.active_page();
                                e.timeline_duration_dialog(id, window, cx)
                            },
                        ))
                        .item(menu_item(
                            &owner,
                            "Fit selection to duration…",
                            |e, window, cx| e.timeline_fit_dialog(window, cx),
                        ))
                        .item(menu_item(&owner, "Snap cuts to markers", |e, _, cx| {
                            e.timeline_snap_cuts(cx)
                        }))
                        .separator()
                        .item(
                            menu_item(&owner, "Layer keys stretch with the panel", |e, _, cx| {
                                e.set_keyframe_sync(KeyframeSync::Scale, cx);
                            })
                            .checked(sync == KeyframeSync::Scale),
                        )
                        .item(
                            menu_item(&owner, "Layer keys keep their frames", |e, _, cx| {
                                e.set_keyframe_sync(KeyframeSync::Keep, cx);
                            })
                            .checked(sync == KeyframeSync::Keep),
                        )
                        .separator()
                        .item(menu_item(
                            &owner,
                            "Add marker at playhead (M)",
                            |e, _, cx| e.timeline_add_marker(None, cx),
                        ))
                    }),
            )
            .child(
                button(
                    "timeline-add-track",
                    "Add track",
                    "Add an audio track (up to 16)",
                )
                .disabled(tracks >= MAX_TRACKS)
                .on_click(cx.listener(|this, _, _, cx| this.timeline_add_track(cx))),
            )
            .child(
                button(
                    "timeline-library-toggle",
                    "Sounds",
                    "Show the sound library",
                )
                .selected(ui.library.open)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.timeline_ui.library.open = !this.timeline_ui.library.open;
                    cx.notify();
                })),
            )
            .children(ui.overlay.clone().map(|text| {
                div()
                    .id("timeline-overlay")
                    .test_support()
                    .ml_auto()
                    .px_2()
                    .rounded(px(3.))
                    .bg(p.accent)
                    .text_color(p.accent_fg)
                    .font_family(MONO_FONT)
                    .text_size(px(11.))
                    .child(text)
            }))
            .into_any_element()
    }

    /// The left header cell of a row.
    pub(super) fn timeline_header(p: &Palette, height: f32) -> Div {
        div()
            .flex_none()
            .w(px(HEADER_W))
            .h(px(height))
            .px_2()
            .border_r_1()
            .border_color(p.line)
            .bg(p.panel)
            .overflow_hidden()
    }

    /// A lane: the area right of the headers where time runs.
    pub(super) fn timeline_lane(id: impl Into<ElementId>, height: f32) -> Stateful<Div> {
        div()
            .id(id)
            .relative()
            .flex_1()
            .min_w_0()
            .h(px(height))
            .overflow_hidden()
    }

    /// The playhead line in a lane.
    pub(super) fn timeline_playhead_line(&self, p: &Palette) -> Option<Div> {
        let x = self.transport.frame as f32 * self.timeline_ui.zoom - self.timeline_ui.scroll;
        (x >= -2. && x <= self.timeline_lane_width() + 2.).then(|| {
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(x))
                .w(px(1.))
                .bg(p.accent)
        })
    }

    fn timeline_ruler(&mut self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let rate = self.timeline_rate();
        let (zoom, scroll) = (self.timeline_ui.zoom, self.timeline_ui.scroll);
        let width = self.timeline_lane_width();
        let step = ruler_step(zoom, rate);
        let minor = match step {
            1 => 1,
            s if s.is_multiple_of(5) && s >= 5 => s / 5,
            s => (s / 2).max(1),
        };
        let first = ((scroll / zoom) as u64 / minor) * minor;
        let last = ((scroll + width) / zoom) as u64 + 1;
        let mut lane = Self::timeline_lane("timeline-ruler", RULER_H)
            .test_support()
            .bg(p.soft_bg)
            .border_b_1()
            .border_color(p.line)
            .cursor(CursorStyle::PointingHand)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, window, cx| {
                    this.timeline_begin(TimelineDrag::Scrub, e.position, e.modifiers, window, cx);
                    cx.stop_propagation();
                }),
            )
            .on_scroll_wheel(
                cx.listener(|this, e: &ScrollWheelEvent, _, cx| this.timeline_wheel(e, cx)),
            );
        // Measure the lane, and follow drags anywhere in the window.
        let lanes = self.timeline_ui.lanes.clone();
        let owner = cx.weak_entity();
        lane = lane.child(
            canvas(
                move |bounds, _, _| lanes.set(Some(bounds)),
                move |_, _, window, _| {
                    let (moved, released) = (owner.clone(), owner.clone());
                    window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                        if phase == DispatchPhase::Bubble {
                            moved
                                .update(cx, |this, cx| {
                                    if this.timeline_ui.drag.is_some() {
                                        this.timeline_move(e.position, e.modifiers, cx);
                                    }
                                })
                                .ok();
                        }
                    });
                    window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
                        if phase == DispatchPhase::Capture && e.button == MouseButton::Left {
                            released.update(cx, |this, cx| this.timeline_end(cx)).ok();
                        }
                    });
                },
            )
            .absolute()
            .size_full(),
        );
        let mut frame = first;
        while frame <= last {
            let x = frame as f32 * zoom - scroll;
            let major = frame.is_multiple_of(step);
            lane = lane.child(
                div()
                    .absolute()
                    .left(px(x))
                    .bottom_0()
                    .w(px(1.))
                    .h(px(if major { 12. } else { 5. }))
                    .bg(p.muted.opacity(if major { 0.8 } else { 0.4 })),
            );
            if major {
                lane = lane.child(
                    div()
                        .absolute()
                        .left(px(x + 3.))
                        .top(px(2.))
                        .font_family(MONO_FONT)
                        .text_size(px(10.))
                        .text_color(p.muted)
                        .child(frame_label(rate, frame, self.timeline_ui.timecode)),
                );
            }
            frame += minor;
        }
        // The play range: shading outside it and a handle at each end.
        if let Some((a, b)) = self.transport.range {
            let (xa, xb) = (a as f32 * zoom - scroll, b as f32 * zoom - scroll);
            lane = lane
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(px(xa))
                        .w(px((xb - xa).max(1.)))
                        .bg(p.accent.opacity(0.14)),
                )
                .child(Self::range_handle("timeline-range-in", xa, false, p, cx))
                .child(Self::range_handle("timeline-range-out", xb, true, p, cx));
        }
        let x = self.transport.frame as f32 * zoom - scroll;
        lane = lane.child(
            div()
                .id("timeline-playhead")
                .test_support()
                .absolute()
                .top(px(RULER_H - 12.))
                .left(px(x - 5.))
                .w(px(11.))
                .h(px(12.))
                .rounded(px(2.))
                .bg(p.accent),
        );
        div()
            .flex()
            .flex_none()
            .child(
                Self::timeline_header(p, RULER_H)
                    .flex()
                    .items_center()
                    .text_size(px(10.))
                    .text_color(p.muted)
                    .child(format!(
                        "{} · {}",
                        rate.label(),
                        if self.timeline_ui.timecode {
                            "timecode"
                        } else {
                            "frames"
                        }
                    )),
            )
            .child(lane)
            .into_any_element()
    }

    fn range_handle(
        id: &'static str,
        x: f32,
        out: bool,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        div()
            .id(id)
            .test_support()
            .absolute()
            .top_0()
            .h(px(RULER_H - 12.))
            .left(px(if out { x - 8. } else { x }))
            .w(px(8.))
            .bg(p.accent.opacity(0.8))
            .cursor(CursorStyle::ResizeLeftRight)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                    let drag = if out {
                        TimelineDrag::RangeOut
                    } else {
                        TimelineDrag::RangeIn
                    };
                    this.timeline_begin(drag, e.position, e.modifiers, window, cx);
                    cx.stop_propagation();
                }),
            )
    }

    /// Ctrl/Cmd+wheel zooms around the pointer; horizontal or Shift+wheel
    /// scrolls through time.
    pub(super) fn timeline_wheel(&mut self, e: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let delta = e.delta.pixel_delta(px(16.));
        let (dx, dy) = (f32::from(delta.x), f32::from(delta.y));
        if e.modifiers.secondary() {
            self.timeline_zoom_by(if dy > 0. { 1.25 } else { 0.8 }, Some(e.position.x), cx);
            cx.stop_propagation();
        } else if e.modifiers.shift || dx.abs() > dy.abs() {
            let d = if dx.abs() > dy.abs() { dx } else { dy };
            self.timeline_ui.scroll -= d;
            self.clamp_timeline_scroll();
            cx.notify();
            cx.stop_propagation();
        }
    }

    fn timeline_panel_row(&mut self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let (zoom, scroll) = (self.timeline_ui.zoom, self.timeline_ui.scroll);
        let width = self.timeline_lane_width();
        let layout = self.timeline_layout();
        let names: HashMap<PageId, String> = self
            .editor
            .page_list()
            .iter()
            .map(|m| (m.id, m.name.clone()))
            .collect();
        let selection: std::collections::HashSet<PageId> =
            self.board_selection().into_iter().collect();
        let active = self.editor.active_page();
        let dragging = matches!(self.timeline_ui.drag, Some(TimelineDrag::Edge { .. }));
        struct Item {
            id: PageId,
            start: u64,
            frames: u32,
            scene: usize,
            scene_name: Option<String>,
            transition: Transition,
            locked: bool,
        }
        let (items, count, rate) = {
            let Some(board) = self.timeline_board() else {
                return div().into_any_element();
            };
            let mut scene_index = 0;
            let mut previous = None;
            let mut items = Vec::new();
            for (id, start) in board.panel_starts(&layout) {
                let panel = &board.panels[&id];
                let first = previous != Some(panel.scene);
                if first && previous.is_some() {
                    scene_index += 1;
                }
                previous = Some(panel.scene);
                items.push(Item {
                    id,
                    start,
                    frames: panel.frames,
                    scene: scene_index,
                    scene_name: first.then(|| board.scenes[&panel.scene].name.clone()),
                    transition: panel.transition,
                    locked: board.is_locked(id),
                });
            }
            let count = items.len();
            (items, count, board.settings.frame_rate)
        };
        let (doc_w, doc_h) = (
            self.editor.doc.width.max(1) as f32,
            self.editor.doc.height.max(1) as f32,
        );
        let thumb_h = PANEL_H - 26.;
        let thumb_w = (thumb_h * doc_w / doc_h).round();
        let mut lane = Self::timeline_lane("timeline-panels", PANEL_H)
            .test_support()
            .border_b_1()
            .border_color(p.line)
            .on_scroll_wheel(
                cx.listener(|this, e: &ScrollWheelEvent, _, cx| this.timeline_wheel(e, cx)),
            );
        let owner = cx.weak_entity();
        for item in items {
            let x = item.start as f32 * zoom - scroll;
            let w = item.frames as f32 * zoom;
            if x + w < -40. || x > width + 40. {
                continue;
            }
            let id = item.id;
            let chosen = selection.contains(&id);
            let image = (w >= 28.)
                .then(|| self.page_thumbnail(id, 96, cx))
                .flatten();
            let menu_owner = owner.clone();
            let mut block = div()
                .id(("timeline-panel", id))
                .test_support()
                .absolute()
                .left(px(x))
                .top(px(4.))
                .w(px(w.max(2.)))
                .h(px(PANEL_H - 9.))
                .rounded(px(3.))
                .overflow_hidden()
                .bg(scene_tint(item.scene))
                .border_1()
                .border_color(if chosen {
                    p.accent
                } else if id == active {
                    p.accent.opacity(0.4)
                } else {
                    p.line
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                        this.timeline_panel_click(id, e.modifiers, e.click_count, window, cx);
                        cx.stop_propagation();
                    }),
                )
                .context_menu(move |menu, _, cx| {
                    if !chosen {
                        menu_owner
                            .update(cx, |e, cx| {
                                e.set_board_selection(vec![id]);
                                cx.notify();
                            })
                            .ok();
                    }
                    Self::timeline_panel_menu(menu, menu_owner.clone(), id)
                })
                .child(
                    div()
                        .flex()
                        .gap_1()
                        .p(px(3.))
                        .child(
                            div()
                                .flex_none()
                                .w(px(thumb_w))
                                .h(px(thumb_h))
                                .bg(gpui_kit::white())
                                .overflow_hidden()
                                .children(image.map(|image| {
                                    img(image).size_full().object_fit(ObjectFit::Contain)
                                })),
                        )
                        .when(w > thumb_w + 40., |row| {
                            row.child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .min_w_0()
                                    .text_size(px(10.))
                                    .child(
                                        div()
                                            .whitespace_nowrap()
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .child(names.get(&id).cloned().unwrap_or_default()),
                                    )
                                    .when(item.locked, |c| {
                                        c.child(div().text_color(p.muted).child("Locked"))
                                    }),
                            )
                        }),
                )
                .when(w > 30., |b| {
                    b.child(
                        div()
                            .absolute()
                            .bottom(px(2.))
                            .left(px(4.))
                            .font_family(MONO_FONT)
                            .text_size(px(9.5))
                            .text_color(p.muted)
                            .child(frame_label(
                                rate,
                                u64::from(item.frames),
                                self.timeline_ui.timecode,
                            )),
                    )
                });
            // The transition plays over the panel's first frames.
            if !item.transition.is_cut() {
                let tw = item.transition.frames as f32 * zoom;
                block = block.child(
                    div()
                        .id(("timeline-transition", id))
                        .test_support()
                        .absolute()
                        .left_0()
                        .top_0()
                        .h(px(12.))
                        .w(px(tw.max(3.)))
                        .bg(p.ink.opacity(0.55))
                        .text_color(p.panel)
                        .text_size(px(8.5))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .pl(px(2.))
                        .child(item.transition.label())
                        .child(
                            div()
                                .id(("timeline-transition-edge", id))
                                .test_support()
                                .absolute()
                                .right_0()
                                .top_0()
                                .bottom_0()
                                .w(px(5.))
                                .bg(p.accent)
                                .cursor(CursorStyle::ResizeLeftRight)
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                                        this.timeline_begin(
                                            TimelineDrag::TransitionLength { panel: id },
                                            e.position,
                                            e.modifiers,
                                            window,
                                            cx,
                                        );
                                        cx.stop_propagation();
                                    }),
                                ),
                        ),
                );
            }
            block = block.child(
                div()
                    .id(("timeline-panel-edge", id))
                    .test_support()
                    .absolute()
                    .right_0()
                    .top_0()
                    .bottom_0()
                    .w(px(EDGE_W))
                    .cursor(CursorStyle::ResizeLeftRight)
                    .when(!dragging, |e| e.hover(|s| s.bg(p.accent.opacity(0.5))))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                            let mode = EdgeMode::from_modifiers(e.modifiers);
                            this.timeline_begin(
                                TimelineDrag::Edge { panel: id, mode },
                                e.position,
                                e.modifiers,
                                window,
                                cx,
                            );
                            cx.stop_propagation();
                        }),
                    ),
            );
            lane = lane.child(block);
            if let Some(name) = item.scene_name {
                // Scene boundary and name.
                lane = lane.child(
                    div()
                        .absolute()
                        .left(px(x - 1.))
                        .top_0()
                        .bottom_0()
                        .w(px(2.))
                        .bg(p.ink.opacity(0.6)),
                );
                if item.start > 0 || w > 60. {
                    lane = lane.child(
                        div()
                            .absolute()
                            .left(px(x + 3.))
                            .bottom(px(14.))
                            .px_1()
                            .rounded(px(2.))
                            .bg(p.ink.opacity(0.7))
                            .text_color(p.panel)
                            .text_size(px(9.))
                            .child(format!("Sc {name}")),
                    );
                }
            }
            // The transitions menu at the cut into this panel.
            if item.start > 0 {
                let current = item.transition;
                let menu_owner = owner.clone();
                lane = lane.child(
                    div().absolute().left(px(x - 7.)).bottom(px(1.)).child(
                        Button::new(("timeline-cut", id))
                            .label(if current.is_cut() { "|" } else { "⧓" })
                            .tooltip(format!("Transition into this panel: {}", current.label()))
                            .xsmall()
                            .outline()
                            .dropdown_menu(move |menu, window, cx| {
                                Self::timeline_transition_menu(
                                    menu,
                                    menu_owner.clone(),
                                    id,
                                    current,
                                    window,
                                    cx,
                                )
                            }),
                    ),
                );
            }
        }
        lane = lane.children(self.timeline_playhead_line(p));
        div()
            .flex()
            .flex_none()
            .child(
                Self::timeline_header(p, PANEL_H)
                    .flex()
                    .flex_col()
                    .justify_center()
                    .text_size(px(11.))
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Panels"))
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(p.muted)
                            .child(format!("{count} playing")),
                    ),
            )
            .child(lane)
            .into_any_element()
    }

    fn timeline_track_rows(&mut self, p: &Palette, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(timeline) = self.timeline_board().map(|b| b.timeline.clone()) else {
            return Vec::new();
        };
        let rate = self.timeline_rate();
        let (zoom, scroll) = (self.timeline_ui.zoom, self.timeline_ui.scroll);
        let width = self.timeline_lane_width();
        let owner = cx.weak_entity();
        self.timeline_ui
            .rows
            .borrow_mut()
            .resize(timeline.tracks.len(), None);
        self.timeline_ui
            .rows
            .borrow_mut()
            .truncate(timeline.tracks.len());
        while self.timeline_ui.volume_tracks.len() < timeline.tracks.len() {
            self.timeline_ui
                .volume_tracks
                .push(Rc::new(Cell::new(None)));
        }
        let mut rows = Vec::new();
        for (t, track) in timeline.tracks.iter().enumerate() {
            let audible = timeline.audible(t);
            let selected = self.timeline_ui.track == Some(t);
            let rows_cell = self.timeline_ui.rows.clone();
            let mut lane = Self::timeline_lane(("timeline-track", t), TRACK_H)
                .test_support()
                .border_b_1()
                .border_color(p.line)
                .when(selected, |l| l.bg(p.soft_bg))
                .on_scroll_wheel(
                    cx.listener(|this, e: &ScrollWheelEvent, _, cx| this.timeline_wheel(e, cx)),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                        this.timeline_ui.track = Some(t);
                        this.timeline_ui.clip = None;
                        this.timeline_ui.marker = None;
                        this.timeline_begin(
                            TimelineDrag::Scrub,
                            e.position,
                            e.modifiers,
                            window,
                            cx,
                        );
                        cx.stop_propagation();
                    }),
                )
                .drag_over::<DraggedSound>(|s, _, _, _| s.bg(gpui_kit::black().opacity(0.06)))
                .on_drop(cx.listener(move |this, d: &DraggedSound, window, cx| {
                    let x = window.mouse_position().x;
                    this.timeline_drop_sound(d, t, x, cx);
                }))
                .child(
                    canvas(
                        move |bounds, _, _| {
                            if let Some(slot) = rows_cell.borrow_mut().get_mut(t) {
                                *slot = Some(bounds);
                            }
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .size_full(),
                );
            for (c, clip) in track.clips.iter().enumerate() {
                let x = clip.start as f32 * zoom - scroll;
                let w = clip.frames as f32 * zoom;
                if x + w < 0. || x > width {
                    continue;
                }
                lane = lane.child(self.timeline_clip(t, c, clip, x, w, audible, &timeline, p, cx));
            }
            for (m, marker) in track.markers.iter().enumerate() {
                let x = marker.frame as f32 * zoom - scroll;
                if x < -40. || x > width + 4. {
                    continue;
                }
                let chosen = self.timeline_ui.marker == Some((t, m));
                let menu_owner = owner.clone();
                let name = marker.name.clone();
                lane = lane.child(
                    div()
                        .id(SharedString::from(format!("timeline-marker-{t}-{m}")))
                        .test_support()
                        .absolute()
                        .left(px(x - 4.))
                        .top_0()
                        .bottom_0()
                        .w(px(9.))
                        .cursor(CursorStyle::ResizeLeftRight)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                                this.timeline_ui.track = Some(t);
                                this.timeline_ui.marker = Some((t, m));
                                this.timeline_ui.clip = None;
                                this.timeline_begin(
                                    TimelineDrag::Marker { track: t, index: m },
                                    e.position,
                                    e.modifiers,
                                    window,
                                    cx,
                                );
                                cx.stop_propagation();
                            }),
                        )
                        .context_menu(move |menu, _, _| {
                            Self::timeline_marker_menu(
                                menu,
                                menu_owner.clone(),
                                (t, m),
                                name.clone(),
                            )
                        })
                        .child(
                            div()
                                .absolute()
                                .left(px(4.))
                                .top_0()
                                .bottom_0()
                                .w(px(1.))
                                .bg(rgb(0xE5A50A)),
                        )
                        .child(
                            div()
                                .absolute()
                                .left_0()
                                .top_0()
                                .size(px(9.))
                                .rounded(px(1.))
                                .bg(if chosen {
                                    p.accent
                                } else {
                                    rgb(0xE5A50A).into()
                                }),
                        ),
                );
                lane = lane.child(
                    div()
                        .absolute()
                        .left(px(x + 6.))
                        .top(px(-1.))
                        .text_size(px(9.))
                        .text_color(p.muted)
                        .whitespace_nowrap()
                        .child(marker.name.clone()),
                );
            }
            lane = lane.children(self.timeline_playhead_line(p));
            let volume = self.timeline_ui.volume_tracks[t].clone();
            let name = track.name.clone();
            let menu_owner = owner.clone();
            let header = Self::timeline_header(p, TRACK_H)
                .id(("timeline-track-header", t))
                .flex()
                .flex_col()
                .justify_center()
                .gap(px(2.))
                .when(selected, |h| h.bg(p.soft_bg))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        this.timeline_ui.track = Some(t);
                        cx.notify();
                    }),
                )
                .context_menu(move |menu, _, _| {
                    Self::timeline_track_menu(menu, menu_owner.clone(), t, name.clone())
                })
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(11.))
                                .whitespace_nowrap()
                                .overflow_hidden()
                                .text_ellipsis()
                                .text_color(if audible { p.ink } else { p.muted })
                                .child(track.name.clone()),
                        )
                        .child(
                            Button::new(("timeline-mute", t))
                                .label("M")
                                .tooltip("Mute")
                                .xsmall()
                                .ghost()
                                .selected(track.muted)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.timeline_toggle_track(t, false, cx)
                                })),
                        )
                        .child(
                            Button::new(("timeline-solo", t))
                                .label("S")
                                .tooltip("Solo")
                                .xsmall()
                                .ghost()
                                .selected(track.solo)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.timeline_toggle_track(t, true, cx)
                                })),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(
                            div().flex_1().child(
                                crate::widgets::slider(
                                    ("timeline-volume", t),
                                    volume_fraction(track.volume_db),
                                    volume,
                                    p,
                                    cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                                        this.timeline_begin(
                                            TimelineDrag::Volume { track: t },
                                            e.position,
                                            e.modifiers,
                                            window,
                                            cx,
                                        );
                                        cx.stop_propagation();
                                    }),
                                )
                                .h(px(16.)),
                            ),
                        )
                        .child(
                            div()
                                .w(px(44.))
                                .font_family(MONO_FONT)
                                .text_size(px(9.5))
                                .text_color(p.muted)
                                .child(format!("{:+.1} dB", track.volume_db)),
                        ),
                );
            rows.push(
                div()
                    .flex()
                    .flex_none()
                    .child(header)
                    .child(lane)
                    .into_any_element(),
            );
        }
        let _ = rate;
        rows
    }

    #[allow(clippy::too_many_arguments)]
    fn timeline_clip(
        &mut self,
        t: usize,
        c: usize,
        clip: &AudioClip,
        x: f32,
        w: f32,
        audible: bool,
        timeline: &Timeline,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let zoom = self.timeline_ui.zoom;
        let rate = self.timeline_rate();
        let chosen = self.timeline_ui.clip == Some((t, c));
        // The part of the sound the clip plays.
        let from_ms = clip.offset_ms as f64;
        let span = (
            from_ms,
            from_ms + rate.frames_to_seconds(clip.frames) * 1000.,
        );
        let peaks = timeline
            .assets
            .contains_key(&clip.asset)
            .then(|| self.sound_peaks(clip.asset, span, w.round() as usize, cx))
            .flatten();
        let fill: Hsla = if audible {
            rgb(0x3E7CB1).into()
        } else {
            p.muted
        };
        let owner = cx.weak_entity();
        let menu_clip = clip.clone();
        let handle = |part: ClipPart, id: String| {
            div()
                .id(SharedString::from(id))
                .test_support()
                .cursor(CursorStyle::ResizeLeftRight)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                        this.timeline_ui.clip = Some((t, c));
                        this.timeline_ui.track = Some(t);
                        this.timeline_begin(
                            TimelineDrag::Clip {
                                track: t,
                                index: c,
                                part,
                            },
                            e.position,
                            e.modifiers,
                            window,
                            cx,
                        );
                        cx.stop_propagation();
                    }),
                )
        };
        let (fade_in, fade_out) = (clip.fade_in as f32 * zoom, clip.fade_out as f32 * zoom);
        div()
            .id(SharedString::from(format!("timeline-clip-{t}-{c}")))
            .test_support()
            .absolute()
            .left(px(x))
            .top(px(4.))
            .w(px(w.max(2.)))
            .h(px(TRACK_H - 9.))
            .rounded(px(3.))
            .overflow_hidden()
            .bg(fill.opacity(0.28))
            .border_1()
            .border_color(if chosen { p.accent } else { fill.opacity(0.7) })
            .cursor(CursorStyle::OpenHand)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                    this.timeline_ui.clip = Some((t, c));
                    this.timeline_ui.track = Some(t);
                    this.timeline_ui.marker = None;
                    this.timeline_begin(
                        TimelineDrag::Clip {
                            track: t,
                            index: c,
                            part: ClipPart::Body,
                        },
                        e.position,
                        e.modifiers,
                        window,
                        cx,
                    );
                    cx.stop_propagation();
                }),
            )
            .context_menu(move |menu, _, _| {
                Self::timeline_clip_menu(menu, owner.clone(), (t, c), menu_clip.clone())
            })
            .child(waveform(peaks, fill).absolute().size_full())
            .child(
                div()
                    .absolute()
                    .left(px(4.))
                    .top(px(1.))
                    .text_size(px(9.5))
                    .text_color(p.ink)
                    .whitespace_nowrap()
                    .child(if clip.gain_db.abs() >= 0.05 {
                        format!("{} · {:+.1} dB", clip.name, clip.gain_db)
                    } else {
                        clip.name.clone()
                    }),
            )
            // Fades: a shaded ramp and a handle at its inner end.
            .when(fade_in > 0., |d| {
                d.child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .bottom_0()
                        .w(px(fade_in))
                        .bg(gpui_kit::black().opacity(0.15)),
                )
            })
            .when(fade_out > 0., |d| {
                d.child(
                    div()
                        .absolute()
                        .right_0()
                        .top_0()
                        .bottom_0()
                        .w(px(fade_out))
                        .bg(gpui_kit::black().opacity(0.15)),
                )
            })
            .child(
                handle(ClipPart::FadeIn, format!("timeline-fade-in-{t}-{c}"))
                    .absolute()
                    .top_0()
                    .left(px((fade_in - 4.).max(0.)))
                    .size(px(8.))
                    .rounded_full()
                    .bg(p.panel)
                    .border_1()
                    .border_color(p.ink),
            )
            .child(
                handle(ClipPart::FadeOut, format!("timeline-fade-out-{t}-{c}"))
                    .absolute()
                    .top_0()
                    .right(px((fade_out - 4.).max(0.)))
                    .size(px(8.))
                    .rounded_full()
                    .bg(p.panel)
                    .border_1()
                    .border_color(p.ink),
            )
            .child(
                handle(ClipPart::Start, format!("timeline-clip-start-{t}-{c}"))
                    .absolute()
                    .left_0()
                    .top(px(10.))
                    .bottom_0()
                    .w(px(EDGE_W)),
            )
            .child(
                handle(ClipPart::End, format!("timeline-clip-end-{t}-{c}"))
                    .absolute()
                    .right_0()
                    .top(px(10.))
                    .bottom_0()
                    .w(px(EDGE_W)),
            )
            .into_any_element()
    }

    fn timeline_scrollbar(&mut self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let content = (self.timeline_length() as f32 + 48.) * self.timeline_ui.zoom;
        let width = self.timeline_lane_width();
        let fraction = (width / content.max(width)).clamp(0.02, 1.);
        let left = (self.timeline_ui.scroll / content.max(width)).clamp(0., 1. - fraction);
        let bar = self.timeline_ui.scrollbar.clone();
        div()
            .flex()
            .flex_none()
            .h(px(10.))
            .child(div().flex_none().w(px(HEADER_W)))
            .child(
                div()
                    .id("timeline-scrollbar")
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .bg(p.soft_bg)
                    .child(
                        canvas(move |b, _, _| bar.set(Some(b)), |_, _, _, _| {})
                            .absolute()
                            .size_full(),
                    )
                    .child(
                        div()
                            .id("timeline-scroll-thumb")
                            .absolute()
                            .top(px(2.))
                            .bottom(px(2.))
                            .left(relative(left))
                            .w(relative(fraction))
                            .rounded(px(3.))
                            .bg(p.muted.opacity(0.5))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                                    let Some(b) = this.timeline_ui.scrollbar.get() else {
                                        return;
                                    };
                                    let thumb =
                                        f32::from(b.origin.x) + left * f32::from(b.size.width);
                                    let grab = f32::from(e.position.x) - thumb;
                                    this.timeline_begin(
                                        TimelineDrag::ScrollBar { grab },
                                        e.position,
                                        e.modifiers,
                                        window,
                                        cx,
                                    );
                                    cx.stop_propagation();
                                }),
                            ),
                    ),
            )
            .into_any_element()
    }
}
