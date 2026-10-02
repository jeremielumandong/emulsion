//! Audio clip effects (T12; SBP 24): the gain envelope drawn on each clip
//! on the Timeline with draggable keys (like fades), and the **Effects…**
//! editor: the envelope and three EQ bands (low shelf, mid peak, high
//! shelf) as sliders with key toggles at the playhead, easing, and key
//! stepping. The model is `emulsion_core::timeline::effects`; the mixer
//! (`emulsion_io::audio::mix`) plays it for the player and export alike.
//! Every change is one Undo step through `timeline_audio_edit`.
use super::storyboard_timeline::{TimelineDrag, TimingEdit, volume_fraction, volume_from_fraction};
use super::*;
use emulsion_core::motion::Easing;
use emulsion_core::timeline::{AudioClip, ClipParam};
use gpui_kit::component::{
    Selectable, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenu, PopupMenuItem},
};

/// Each clip's envelope area as last painted, by track and clip.
type Areas = Rc<RefCell<HashMap<(usize, usize), Bounds<Pixels>>>>;

/// The envelope areas, for dragging keys up and down.
#[derive(Default)]
pub(crate) struct AudioFxUi {
    areas: Areas,
    /// Where a key drag began (pointer y) and the envelope area's height:
    /// levels follow the pointer's travel, so the drag holds even if the
    /// Timeline's layout shifts while it runs.
    grab: Option<(Pixels, f32)>,
}

/// `param`'s value as a slider position, 0–1.
fn fraction(param: ClipParam, db: f32) -> f32 {
    if param == ClipParam::Envelope {
        return volume_fraction(db);
    }
    let (lo, hi) = param.range();
    ((db - lo) / (hi - lo)).clamp(0., 1.)
}

/// The value at slider position `f`, in half-dB steps.
fn from_fraction(param: ClipParam, f: f32) -> f32 {
    if param == ClipParam::Envelope {
        return volume_from_fraction(f);
    }
    let (lo, hi) = param.range();
    ((lo + (hi - lo) * f.clamp(0., 1.)) * 2.).round() / 2.
}

/// A clip's key at `frame` moved to `to` frames and `db`, kept between its
/// neighbours and inside the clip. `None` when the key no longer exists.
pub(crate) fn move_key(
    clip: &AudioClip,
    param: ClipParam,
    index: usize,
    to: i64,
    db: f32,
) -> Option<AudioClip> {
    let keys = clip.keys(param);
    keys.get(index)?;
    let low = index.checked_sub(1).map_or(0, |i| keys[i].frame as i64 + 1);
    let high = keys
        .get(index + 1)
        .map_or(clip.frames as i64 - 1, |k| k.frame as i64 - 1);
    let mut next = clip.clone();
    let (lo, hi) = param.range();
    let key = &mut next.keys_mut(param)[index];
    key.frame = to.clamp(low, high.max(low)) as u64;
    key.db = db.clamp(lo, hi);
    Some(next)
}

impl EditorView {
    /// The clip at `at` in the board.
    fn fx_clip(&self, (track, index): (usize, usize)) -> Option<&AudioClip> {
        self.editor
            .storyboard()?
            .timeline
            .tracks
            .get(track)?
            .clips
            .get(index)
    }

    /// The playhead as frames into the clip, kept inside it.
    pub(crate) fn fx_playhead(&self, clip: &AudioClip) -> u64 {
        self.transport
            .frame
            .saturating_sub(clip.start)
            .min(clip.frames.saturating_sub(1))
    }

    /// Change a clip's effects as one Undo step.
    pub(crate) fn clip_fx_edit(
        &mut self,
        (track, index): (usize, usize),
        edit: impl FnOnce(&mut AudioClip) -> Result<(), String>,
        cx: &mut Context<Self>,
    ) -> bool {
        self.timeline_audio_edit(
            move |t, _| {
                edit(
                    t.tracks
                        .get_mut(track)
                        .and_then(|t| t.clips.get_mut(index))
                        .ok_or("That clip no longer exists.")?,
                )
            },
            cx,
        )
    }

    /// Set `param` at the playhead (a key when it is keyed).
    pub(crate) fn clip_fx_set(
        &mut self,
        at: (usize, usize),
        param: ClipParam,
        db: f32,
        cx: &mut Context<Self>,
    ) {
        let Some(frame) = self.fx_clip(at).map(|c| self.fx_playhead(c)) else {
            return;
        };
        self.clip_fx_edit(
            at,
            |c| {
                c.set_param(param, frame, db, false);
                Ok(())
            },
            cx,
        );
    }

    /// Add a key at the playhead (with the value there), or remove the key
    /// there.
    pub(crate) fn clip_fx_toggle_key(
        &mut self,
        at: (usize, usize),
        param: ClipParam,
        cx: &mut Context<Self>,
    ) {
        let Some(frame) = self.fx_clip(at).map(|c| self.fx_playhead(c)) else {
            return;
        };
        self.clip_fx_edit(
            at,
            |c| {
                let keys = c.keys(param);
                match keys.iter().position(|k| k.frame == frame) {
                    Some(i) => {
                        let db = c.param_at(param, frame as f64);
                        let keys = c.keys_mut(param);
                        keys.remove(i);
                        // A band's last key leaves its value as the gain.
                        if keys.is_empty() {
                            c.set_param(param, frame, db, false);
                        }
                    }
                    None => {
                        let db = c.param_at(param, frame as f64);
                        c.set_param(param, frame, db, true);
                    }
                }
                Ok(())
            },
            cx,
        );
    }

    pub(crate) fn clip_fx_delete_key(
        &mut self,
        at: (usize, usize),
        param: ClipParam,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        self.clip_fx_edit(
            at,
            |c| {
                let keys = c.keys_mut(param);
                if index >= keys.len() {
                    return Err("That key no longer exists.".into());
                }
                keys.remove(index);
                Ok(())
            },
            cx,
        );
    }

    pub(crate) fn clip_fx_easing(
        &mut self,
        at: (usize, usize),
        param: ClipParam,
        index: usize,
        easing: Easing,
        cx: &mut Context<Self>,
    ) {
        self.clip_fx_edit(
            at,
            |c| {
                let key = c
                    .keys_mut(param)
                    .get_mut(index)
                    .ok_or("That key no longer exists.")?;
                key.easing = easing;
                key.curve = None;
                Ok(())
            },
            cx,
        );
    }

    /// Remove `param`'s keys (a band keeps its value at the playhead).
    pub(crate) fn clip_fx_clear(
        &mut self,
        at: (usize, usize),
        param: ClipParam,
        cx: &mut Context<Self>,
    ) {
        let Some(frame) = self.fx_clip(at).map(|c| self.fx_playhead(c)) else {
            return;
        };
        self.clip_fx_edit(
            at,
            |c| {
                let db = c.param_at(param, frame as f64);
                c.keys_mut(param).clear();
                c.set_param(param, frame, db, false);
                Ok(())
            },
            cx,
        );
    }

    /// Move the playhead to the previous or next key of `param`.
    fn clip_fx_step(
        &mut self,
        at: (usize, usize),
        param: ClipParam,
        next: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(clip) = self.fx_clip(at) else {
            return;
        };
        let here = self.transport.frame;
        let frames = clip.keys(param).iter().map(|k| clip.start + k.frame);
        let target = if next {
            frames.filter(|f| *f > here).min()
        } else {
            frames.filter(|f| *f < here).max()
        };
        if let Some(frame) = target {
            self.timeline_seek(frame, cx);
        }
    }

    // ── The envelope on the Timeline ──

    /// Where dragging envelope key `key` of the clip at `at` puts it, with
    /// the overlay text.
    pub(crate) fn envelope_key_move(
        &self,
        (track, index): (usize, usize),
        key: usize,
        travel: f64,
        y: Pixels,
    ) -> Option<(TimingEdit, String)> {
        let board = self.editor.storyboard()?;
        let clip = board.timeline.tracks.get(track)?.clips.get(index)?;
        let original = clip.envelope.get(key)?;
        let (y0, h) = self.audio_fx.grab?;
        let f = volume_fraction(original.db) - f32::from(y - y0) / h.max(1.);
        let db = volume_from_fraction(f);
        let to = (original.frame as f64 + travel).round() as i64;
        let moved = move_key(clip, ClipParam::Envelope, key, to, db)?;
        let k = moved.envelope[key];
        let rate = board.settings.frame_rate;
        let mut next = board.timeline.clone();
        next.tracks[track].clips[index] = moved;
        Some((
            TimingEdit::Audio(next),
            format!("{:+.1} dB · {}", k.db, rate.timecode(clip.start + k.frame)),
        ))
    }

    /// The envelope drawn over a clip `w` pixels wide, with key handles;
    /// `None` when it has no envelope keys.
    pub(super) fn timeline_clip_envelope(
        &mut self,
        (t, c): (usize, usize),
        clip: &AudioClip,
        w: f32,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if clip.envelope.is_empty() {
            return None;
        }
        let zoom = self.timeline_ui.zoom;
        let areas = self.audio_fx.areas.clone();
        let line = clip.clone();
        let color: Hsla = rgb(0xF2C14E).into();
        let mut layer = div()
            .absolute()
            .top(px(10.))
            .bottom(px(2.))
            .left_0()
            .w(px(w))
            .child(
                canvas(
                    move |bounds, _, _| {
                        areas.borrow_mut().insert((t, c), bounds);
                    },
                    move |bounds, _, window, _| {
                        let h = f32::from(bounds.size.height);
                        let y = |db: f32| bounds.origin.y + px(h * (1. - volume_fraction(db)));
                        let mut path = PathBuilder::stroke(px(1.5));
                        let steps = (w / 2.).ceil().max(1.) as usize;
                        for i in 0..=steps {
                            let x = (i as f32 * 2.).min(w);
                            let db = line.param_at(ClipParam::Envelope, f64::from(x / zoom));
                            let at = point(bounds.origin.x + px(x), y(db));
                            if i == 0 {
                                path.move_to(at);
                            } else {
                                path.line_to(at);
                            }
                        }
                        if let Ok(path) = path.build() {
                            window.paint_path(path, color);
                        }
                    },
                )
                .absolute()
                .size_full(),
            );
        let owner = cx.weak_entity();
        for (k, key) in clip.envelope.iter().enumerate() {
            let x = key.frame as f32 * zoom;
            if x > w + 4. {
                break;
            }
            let fraction = volume_fraction(key.db);
            let owner = owner.clone();
            let easing = key.easing;
            layer = layer.child(
                div()
                    .id(SharedString::from(format!(
                        "timeline-envelope-key-{t}-{c}-{k}"
                    )))
                    .test_support()
                    .absolute()
                    .left(px(x - 4.))
                    .top(relative(1. - fraction))
                    .mt(px(-4.))
                    .size(px(8.))
                    .rounded(px(1.))
                    .bg(color)
                    .border_1()
                    .border_color(p.ink)
                    .cursor(CursorStyle::PointingHand)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                            this.timeline_ui.clip = Some((t, c));
                            this.timeline_ui.track = Some(t);
                            let h = this
                                .audio_fx
                                .areas
                                .borrow()
                                .get(&(t, c))
                                .map_or(1., |a| f32::from(a.size.height));
                            this.audio_fx.grab = Some((e.position.y, h));
                            this.timeline_begin(
                                TimelineDrag::EnvelopeKey {
                                    track: t,
                                    clip: c,
                                    key: k,
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
                        Self::envelope_key_menu(menu, owner.clone(), (t, c), k, easing)
                    }),
            );
        }
        Some(layer.into_any_element())
    }

    fn envelope_key_menu(
        menu: PopupMenu,
        owner: WeakEntity<Self>,
        at: (usize, usize),
        key: usize,
        easing: Easing,
    ) -> PopupMenu {
        let mut menu = menu;
        for choice in Easing::ALL {
            let owner = owner.clone();
            menu = menu.item(
                PopupMenuItem::new(choice.label())
                    .checked(choice == easing)
                    .on_click(move |_, _, cx| {
                        owner
                            .update(cx, |e, cx| {
                                e.clip_fx_easing(at, ClipParam::Envelope, key, choice, cx)
                            })
                            .ok();
                    }),
            );
        }
        menu.separator()
            .item(PopupMenuItem::new("Delete key").on_click(move |_, _, cx| {
                owner
                    .update(cx, |e, cx| {
                        e.clip_fx_delete_key(at, ClipParam::Envelope, key, cx)
                    })
                    .ok();
            }))
    }

    /// The clip menu's **Effects…** item.
    pub(super) fn timeline_clip_effects_item(
        owner: &WeakEntity<Self>,
        at: (usize, usize),
    ) -> PopupMenuItem {
        let owner = owner.clone();
        PopupMenuItem::new("Effects…").on_click(move |_, window, cx| {
            owner
                .update(cx, |e, cx| e.open_clip_effects(at, window, cx))
                .ok();
        })
    }

    /// The clip menu's item adding a gain envelope key at the playhead.
    pub(super) fn timeline_clip_gain_key_item(
        owner: &WeakEntity<Self>,
        at: (usize, usize),
    ) -> PopupMenuItem {
        let owner = owner.clone();
        PopupMenuItem::new("Add gain key at playhead").on_click(move |_, _, cx| {
            owner
                .update(cx, |e, cx| {
                    let Some(clip) = e.fx_clip(at) else {
                        return;
                    };
                    let frame = e.fx_playhead(clip);
                    if !clip.envelope.iter().any(|k| k.frame == frame) {
                        e.clip_fx_toggle_key(at, ClipParam::Envelope, cx)
                    }
                })
                .ok();
        })
    }

    /// Open the Effects editor for the clip at `at`.
    pub(crate) fn open_clip_effects(
        &mut self,
        at: (usize, usize),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.fx_clip(at).is_none() {
            return;
        }
        self.timeline_ui.clip = Some(at);
        self.timeline_ui.track = Some(at.0);
        let owner = cx.weak_entity();
        let editor = cx.entity();
        let panel = cx.new(|cx| ClipEffects {
            owner,
            // Redraw when the board or the playhead changes.
            _observe: cx.observe(&editor, |_, _, cx| cx.notify()),
            at,
            drag: None,
            tracks: ClipParam::ALL
                .iter()
                .map(|p| (*p, Rc::new(std::cell::Cell::new(None))))
                .collect(),
            focus: cx.focus_handle(),
        });
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title("Clip effects")
                .width(px(560.))
                .child(panel.clone())
        });
    }
}

/// The Effects editor for one clip. It reads the clip from the board on
/// every draw, so Undo and timeline edits show at once.
pub(crate) struct ClipEffects {
    owner: WeakEntity<EditorView>,
    _observe: Subscription,
    at: (usize, usize),
    /// A slider being dragged and its value, committed on release.
    drag: Option<(ClipParam, f32)>,
    tracks: HashMap<ClipParam, TrackBounds>,
    focus: FocusHandle,
}

impl ClipEffects {
    fn slider_value(&self, param: ClipParam, x: Pixels) -> Option<f32> {
        let bounds = self.tracks.get(&param)?;
        crate::widgets::track_fraction(bounds, x).map(|f| from_fraction(param, f))
    }

    fn commit(&mut self, cx: &mut Context<Self>) {
        let Some((param, db)) = self.drag.take() else {
            return;
        };
        let at = self.at;
        self.owner
            .update(cx, |e, cx| e.clip_fx_set(at, param, db, cx))
            .ok();
        cx.notify();
    }

    fn act(
        &self,
        cx: &mut Context<Self>,
        run: impl FnOnce(&mut EditorView, &mut Context<EditorView>),
    ) {
        self.owner.update(cx, run).ok();
        cx.notify();
    }
}

impl Render for ClipEffects {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let root = div()
            .id("clip-effects")
            .test_support()
            .track_focus(&self.focus)
            .flex()
            .flex_col()
            .gap_2()
            .text_size(px(12.))
            .text_color(p.ink);
        let Some(owner) = self.owner.upgrade() else {
            return root;
        };
        let e = owner.read(cx);
        let Some(board) = e.editor.storyboard() else {
            return root;
        };
        let at = self.at;
        let Some(clip) = board
            .timeline
            .tracks
            .get(at.0)
            .and_then(|t| t.clips.get(at.1))
            .cloned()
        else {
            return root.child("That clip no longer exists.");
        };
        let rate = board.settings.frame_rate;
        let into = e.fx_playhead(&clip);
        let inside = (clip.start..clip.end()).contains(&e.transport.frame);
        let mut root = root
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(format!("“{}” · {:+.1} dB clip gain", clip.name, clip.gain_db)),
            )
            .child(div().text_color(p.muted).child(if inside {
                format!(
                    "Keys go at the playhead, {} into the clip. Move the playhead on the Timeline to key another point.",
                    rate.timecode(into)
                )
            } else {
                format!(
                    "The playhead is outside the clip, so keys go at its nearest end ({}). Move the playhead into the clip to key another point.",
                    rate.timecode(into)
                )
            }));
        for param in ClipParam::ALL {
            let keys = clip.keys(param);
            let here = keys.iter().position(|k| k.frame == into);
            let value = match self.drag {
                Some((p, v)) if p == param => v,
                _ => clip.param_at(param, into as f64),
            };
            let track = self.tracks[&param].clone();
            let id = param.key();
            let (lo, hi) = param.range();
            let slider = crate::widgets::slider(
                SharedString::from(format!("clip-fx-{id}")),
                fraction(param, value),
                track,
                &p,
                cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                    if let Some(v) = this.slider_value(param, ev.position.x) {
                        this.drag = Some((param, v));
                        cx.notify();
                    }
                }),
            );
            let key_label = if here.is_some() { "◆" } else { "◇" };
            let mut row = div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .w(px(150.))
                        .child(format!("{} · {lo:+}…{hi:+} dB", param.label())),
                )
                .child(div().w(px(150.)).child(slider))
                .child(
                    div()
                        .id(SharedString::from(format!("clip-fx-{id}-value")))
                        .test_support()
                        .w(px(64.))
                        .font_family(MONO_FONT)
                        .child(format!("{value:+.1} dB")),
                )
                .child(
                    Button::new(SharedString::from(format!("clip-fx-{id}-prev")))
                        .label("‹")
                        .tooltip("Previous key")
                        .xsmall()
                        .ghost()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.act(cx, |e, cx| e.clip_fx_step(at, param, false, cx))
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("clip-fx-{id}-key")))
                        .label(key_label)
                        .tooltip("Add or remove a key at the playhead")
                        .xsmall()
                        .ghost()
                        .selected(here.is_some())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.act(cx, |e, cx| e.clip_fx_toggle_key(at, param, cx))
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("clip-fx-{id}-next")))
                        .label("›")
                        .tooltip("Next key")
                        .xsmall()
                        .ghost()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.act(cx, |e, cx| e.clip_fx_step(at, param, true, cx))
                        })),
                );
            if let Some(index) = here {
                let easing = keys[index].easing;
                let owner = self.owner.clone();
                row = row.child(
                    Button::new(SharedString::from(format!("clip-fx-{id}-ease")))
                        .label(format!("{} ▾", easing.label()))
                        .xsmall()
                        .ghost()
                        .dropdown_menu(move |mut menu, _, _| {
                            for choice in Easing::ALL {
                                let owner = owner.clone();
                                menu = menu.item(
                                    PopupMenuItem::new(choice.label())
                                        .checked(choice == easing)
                                        .on_click(move |_, _, cx| {
                                            owner
                                                .update(cx, |e, cx| {
                                                    e.clip_fx_easing(at, param, index, choice, cx)
                                                })
                                                .ok();
                                        }),
                                );
                            }
                            menu
                        }),
                );
            }
            if !keys.is_empty() {
                row = row.child(
                    Button::new(SharedString::from(format!("clip-fx-{id}-clear")))
                        .label(format!(
                            "Clear {} key{}",
                            keys.len(),
                            if keys.len() == 1 { "" } else { "s" }
                        ))
                        .xsmall()
                        .ghost()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.act(cx, |e, cx| e.clip_fx_clear(at, param, cx))
                        })),
                );
            }
            root = root.child(row);
        }
        root.child(div().text_color(p.muted).child(
            "EQ: low shelf at 200 Hz, mid peak at 1 kHz, high shelf at 5 kHz. Drag the envelope's keys on the clip to move them; right-click one to ease or delete it. Each change is one Undo step.",
        ))
        .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
            let Some((param, _)) = this.drag else {
                return;
            };
            if ev.pressed_button != Some(MouseButton::Left) {
                this.commit(cx);
                return;
            }
            if let Some(v) = this.slider_value(param, ev.position.x) {
                this.drag = Some((param, v));
                cx.notify();
            }
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, _: &MouseUpEvent, _, cx| this.commit(cx)),
        )
        .on_mouse_up_out(
            MouseButton::Left,
            cx.listener(|this, _: &MouseUpEvent, _, cx| this.commit(cx)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    use emulsion_core::timeline::EffectKey;

    #[test]
    fn sliders_map_both_ways_in_half_db_steps() {
        for param in ClipParam::ALL {
            let (lo, hi) = param.range();
            assert_eq!(from_fraction(param, 0.), lo);
            assert_eq!(from_fraction(param, 1.), hi);
            assert_eq!(from_fraction(param, fraction(param, 6.)), 6.);
        }
        assert_eq!(from_fraction(ClipParam::Low, 0.5), 0.);
    }

    #[test]
    fn dragged_keys_stay_between_their_neighbours() {
        let mut clip = AudioClip {
            frames: 50,
            ..AudioClip::default()
        };
        for (f, db) in [(0, 0.), (10, -6.), (20, 0.)] {
            clip.envelope.push(EffectKey::new(f, db));
        }
        let moved = move_key(&clip, ClipParam::Envelope, 1, 40, -100.).unwrap();
        assert_eq!(moved.envelope[1].frame, 19);
        assert_eq!(moved.envelope[1].db, -60.);
        let moved = move_key(&clip, ClipParam::Envelope, 2, 90, 3.).unwrap();
        assert_eq!(moved.envelope[2].frame, 49, "inside the clip");
        assert!(move_key(&clip, ClipParam::Envelope, 7, 0, 0.).is_none());
    }
}
