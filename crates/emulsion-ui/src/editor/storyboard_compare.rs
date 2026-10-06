//! Compare Versions: two states of the board (versions, the last save or
//! export, or the board now) panel by panel, with both pictures, caption
//! word diffs and timing, and a selected pair at full size side by side,
//! as a wipe or as an onion skin. Old versions are read from history into
//! separate read-only states; the open project is never touched.
use super::storyboard_changes::{baselines, kind_color, spawn_thumb};
use super::*;
use crate::widgets::TrackBounds;
use emulsion_core::project::PageId;
use emulsion_core::storyboard_changes::{CompareRow, DiffOp, compare};
use emulsion_core::storyboard_versions::{Baseline, BoardState};
use gpui_kit::component::{
    Sizable, WindowExt,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
};
use std::collections::HashSet;

const ROW_THUMB: u32 = 120;
const PAIR_THUMB: u32 = 640;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PairView {
    SideBySide,
    Wipe,
    Onion,
}

/// One side of the comparison: a baseline, or the board now (`None`).
type Side = Option<Baseline>;

pub(crate) struct CompareView {
    editor: WeakEntity<EditorView>,
    from: Side,
    to: Side,
    states: Option<(Arc<BoardState>, Arc<BoardState>)>,
    rows: Option<Arc<Vec<CompareRow>>>,
    error: Option<String>,
    unchanged: bool,
    selected: Option<usize>,
    view: PairView,
    /// Where the wipe divides the pair, 0–1 from the left.
    wipe: f32,
    track: TrackBounds,
    /// Pictures by (newer side, panel, size); cleared when a side changes.
    thumbs: HashMap<(bool, PageId, u32), Arc<RenderImage>>,
    loading: HashSet<(bool, PageId, u32)>,
    generation: u64,
}

impl CompareView {
    fn new(editor: &Entity<EditorView>, from: Side, cx: &mut Context<Self>) -> Self {
        let mut view = Self {
            editor: editor.downgrade(),
            from,
            to: None,
            states: None,
            rows: None,
            error: None,
            unchanged: false,
            selected: None,
            view: PairView::SideBySide,
            wipe: 0.5,
            track: Default::default(),
            thumbs: HashMap::new(),
            loading: HashSet::new(),
            generation: 0,
        };
        view.load(cx);
        view
    }

    /// Read both sides from history and compare them off the UI thread.
    fn load(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        self.thumbs.clear();
        self.loading.clear();
        self.rows = None;
        self.selected = None;
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        let read = |side: Side, cx: &App| -> Result<BoardState, String> {
            let e = &editor.read(cx).editor;
            match side {
                Some(baseline) => e.board_state(baseline),
                None => e
                    .current_board_state()
                    .ok_or_else(|| "Open a storyboard.".into()),
            }
        };
        let states = read(self.from, cx).and_then(|a| Ok((a, read(self.to, cx)?)));
        let (old, new) = match states {
            Ok((a, b)) => (Arc::new(a), Arc::new(b)),
            Err(error) => {
                self.error = Some(error);
                self.states = None;
                cx.notify();
                return;
            }
        };
        self.error = None;
        self.states = Some((old.clone(), new.clone()));
        let generation = self.generation;
        cx.spawn(async move |this, cx| {
            let rows = cx
                .background_spawn(async move { compare(&old, &new) })
                .await;
            this.update(cx, |this, cx| {
                if this.generation == generation {
                    this.rows = Some(Arc::new(rows));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn thumb(
        &mut self,
        newer: bool,
        panel: Option<PageId>,
        max: u32,
        cx: &mut Context<Self>,
    ) -> Option<Arc<RenderImage>> {
        let panel = panel?;
        let key = (newer, panel, max);
        if let Some(image) = self.thumbs.get(&key) {
            return Some(image.clone());
        }
        let (old, new) = self.states.as_ref()?;
        let state = if newer { new } else { old };
        if self.loading.insert(key) {
            let doc = state.doc(panel)?.clone();
            let generation = self.generation;
            spawn_thumb(doc, max, cx, move |this: &mut Self, image, cx| {
                if this.generation == generation {
                    match image {
                        Ok(image) => {
                            this.thumbs.insert(key, image);
                        }
                        Err(error) => this.error = Some(error),
                    }
                    cx.notify();
                }
            });
        }
        None
    }

    fn side_label(editor: &EditorView, side: Side) -> String {
        match side {
            None => "The board now".into(),
            Some(b) => baselines(&editor.editor)
                .into_iter()
                .find(|(c, _)| *c == b)
                .map_or("Deleted version".into(), |(_, l)| l),
        }
    }

    fn side_picker(
        &self,
        id: &'static str,
        title: &'static str,
        newer: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let Some(editor) = self.editor.upgrade() else {
            return div().into_any_element();
        };
        let e = editor.read(cx);
        let current = if newer { self.to } else { self.from };
        let choices: Vec<(Side, String)> = std::iter::once((None, "The board now".to_string()))
            .chain(baselines(&e.editor).into_iter().map(|(b, l)| (Some(b), l)))
            .collect();
        let owner = cx.weak_entity();
        Button::new(id)
            .label(format!("{title}: {} ▾", Self::side_label(e, current)))
            .small()
            .outline()
            .dropdown_menu(move |mut menu, _, _| {
                for (side, label) in &choices {
                    let (side, owner) = (*side, owner.clone());
                    menu = menu.item(
                        PopupMenuItem::new(label.clone())
                            .checked(side == current)
                            .on_click(move |_, _, cx| {
                                owner
                                    .update(cx, |this, cx| {
                                        if newer {
                                            this.to = side;
                                        } else {
                                            this.from = side;
                                        }
                                        this.load(cx);
                                    })
                                    .ok();
                            }),
                    );
                }
                menu
            })
            .into_any_element()
    }

    /// A caption's word diff: added words highlighted, removed ones struck.
    fn diff_text(words: &[(DiffOp, String)], p: &Palette) -> StyledText {
        let mut text = String::new();
        let mut highlights = Vec::new();
        for (op, part) in words {
            let start = text.len();
            text.push_str(part);
            let style = match op {
                DiffOp::Same => continue,
                DiffOp::Added => HighlightStyle {
                    background_color: Some(
                        kind_color(emulsion_core::storyboard_changes::ChangeKind::New).opacity(0.3),
                    ),
                    font_weight: Some(FontWeight::SEMIBOLD),
                    ..Default::default()
                },
                DiffOp::Removed => HighlightStyle {
                    background_color: Some(
                        kind_color(emulsion_core::storyboard_changes::ChangeKind::Deleted)
                            .opacity(0.2),
                    ),
                    strikethrough: Some(StrikethroughStyle {
                        thickness: px(1.),
                        color: Some(p.ink),
                    }),
                    ..Default::default()
                },
            };
            highlights.push((start..text.len(), style));
        }
        StyledText::new(text).with_highlights(highlights)
    }

    fn picture(image: Option<Arc<RenderImage>>, w: f32, h: f32) -> Div {
        div()
            .flex_none()
            .w(px(w))
            .h(px(h))
            .bg(gpui_kit::white())
            .overflow_hidden()
            .children(image.map(|i| img(i).size_full().object_fit(ObjectFit::Contain)))
    }

    /// The selected pair at full size.
    fn pair(
        &mut self,
        row: &CompareRow,
        aspect: f32,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let old = self.thumb(false, row.change.old, PAIR_THUMB, cx);
        let new = self.thumb(true, row.change.new, PAIR_THUMB, cx);
        let modes = [
            (PairView::SideBySide, "Side by side", "compare-side"),
            (PairView::Wipe, "Wipe", "compare-wipe"),
            (PairView::Onion, "Onion skin", "compare-onion"),
        ];
        let mut bar = div().flex().items_center().gap(px(6.));
        for (mode, text, id) in modes {
            bar = bar.child(
                chip(id, text, self.view == mode, p)
                    .test_support()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.view = mode;
                        cx.notify();
                    })),
            );
        }
        let w = 640.;
        let h = (w * aspect).round();
        let body = match self.view {
            PairView::SideBySide => div()
                .flex()
                .gap(px(8.))
                .child(Self::picture(old, w / 2. - 4., h / 2.))
                .child(Self::picture(new, w / 2. - 4., h / 2.))
                .into_any_element(),
            PairView::Onion => div()
                .relative()
                .w(px(w))
                .h(px(h))
                .child(Self::picture(old, w, h).absolute())
                .child(Self::picture(new, w, h).absolute().opacity(0.5))
                .into_any_element(),
            PairView::Wipe => {
                let track = self.track.clone();
                let split = self.wipe * w;
                div()
                    .id("compare-wipe-view")
                    .test_support()
                    .relative()
                    .w(px(w))
                    .h(px(h))
                    .cursor(CursorStyle::ResizeLeftRight)
                    .child(
                        canvas(move |b, _, _| track.set(Some(b)), |_, _, _, _| {})
                            .absolute()
                            .size_full(),
                    )
                    .child(Self::picture(new, w, h).absolute())
                    // The older picture on the left of the divide.
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .h(px(h))
                            .w(px(split))
                            .overflow_hidden()
                            .child(Self::picture(old, w, h)),
                    )
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .left(px(split - 1.))
                            .w(px(2.))
                            .h(px(h))
                            .bg(p.accent),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, e: &MouseDownEvent, _, cx| {
                            if let Some(f) =
                                crate::widgets::track_fraction(&this.track, e.position.x)
                            {
                                this.wipe = f;
                                cx.notify();
                            }
                        }),
                    )
                    .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
                        if e.pressed_button == Some(MouseButton::Left)
                            && let Some(f) =
                                crate::widgets::track_fraction(&this.track, e.position.x)
                        {
                            this.wipe = f;
                            cx.notify();
                        }
                    }))
                    .into_any_element()
            }
        };
        div()
            .id("compare-pair")
            .test_support()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(bar.child(mono(
                match self.view {
                    PairView::Wipe => {
                        "Drag across the picture: older on the left, newer on the right."
                    }
                    PairView::Onion => "The newer picture at half strength over the older one.",
                    PairView::SideBySide => "Older on the left, newer on the right.",
                },
                10.,
                p.muted,
            )))
            .child(body)
            .into_any_element()
    }
}

impl Render for CompareView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let toolbar = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
            .child(self.side_picker("compare-from", "Older", false, cx))
            .child("→")
            .child(self.side_picker("compare-to", "Newer", true, cx))
            .child(
                chip(
                    "compare-unchanged",
                    "Unchanged panels too",
                    self.unchanged,
                    &p,
                )
                .test_support()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.unchanged = !this.unchanged;
                    this.selected = None;
                    cx.notify();
                })),
            )
            .child(div().flex_1())
            .child(
                Button::new("compare-refresh")
                    .label("Refresh")
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| this.load(cx))),
            );
        let root = div()
            .id("storyboard-compare")
            .test_support()
            .flex()
            .flex_col()
            .gap(px(8.))
            .text_size(px(12.))
            .child(toolbar);
        if let Some(error) = &self.error {
            return root.child(div().text_color(p.muted).child(error.clone()));
        }
        let (Some(rows), Some((old, _))) = (self.rows.clone(), self.states.clone()) else {
            return root.child(mono("Comparing…", 10., p.muted));
        };
        let aspect = old.board.settings.height as f32 / old.board.settings.width.max(1) as f32;
        let thumb_h = (ROW_THUMB as f32 * aspect).round();
        let shown: Vec<usize> = (0..rows.len())
            .filter(|i| self.unchanged || rows[*i].change.is_change())
            .collect();
        let mut list = div()
            .id("compare-rows")
            .test_support()
            .flex()
            .flex_col()
            .gap(px(4.))
            .max_h(px(320.))
            .overflow_y_scroll();
        if shown.is_empty() {
            list = list.child(mono("The two versions have the same panels.", 10., p.muted));
        }
        for &index in &shown {
            let row = &rows[index];
            let old_image = self.thumb(false, row.change.old, ROW_THUMB, cx);
            let new_image = self.thumb(true, row.change.new, ROW_THUMB, cx);
            let frames = match (row.old_frames, row.new_frames) {
                (Some(a), Some(b)) if a != b => format!("{a} → {b} frames"),
                (Some(a), None) | (None, Some(a)) => format!("{a} frames"),
                (Some(a), Some(_)) => format!("{a} frames"),
                (None, None) => String::new(),
            };
            let mut info = div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .min_w_0()
                .flex_1()
                .child(
                    div()
                        .truncate()
                        .text_color(p.ink)
                        .child(row.change.name.clone()),
                )
                .child(mono(
                    format!("{} · {frames}", row.change.summary()),
                    10.,
                    kind_color(row.change.kind),
                ));
            for caption in &row.captions {
                info = info.child(
                    div()
                        .flex()
                        .gap(px(6.))
                        .child(mono(caption.field.clone(), 10., p.muted).flex_none())
                        .child(div().min_w_0().child(Self::diff_text(&caption.words, &p))),
                );
            }
            let missing = |present: bool| {
                if present { "" } else { "—" }
            };
            list = list.child(
                div()
                    .id(("compare-row", index))
                    .test_support()
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .p(px(4.))
                    .rounded(px(3.))
                    .cursor_pointer()
                    .when(self.selected == Some(index), |d| d.bg(p.soft_bg))
                    .hover(|d| d.bg(p.soft_bg))
                    .child(
                        Self::picture(old_image, ROW_THUMB as f32, thumb_h)
                            .child(missing(row.change.old.is_some())),
                    )
                    .child(
                        Self::picture(new_image, ROW_THUMB as f32, thumb_h)
                            .border_2()
                            .border_color(kind_color(row.change.kind))
                            .child(missing(row.change.new.is_some())),
                    )
                    .child(info)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected = Some(index);
                        cx.notify();
                    })),
            );
        }
        let pair = self
            .selected
            .and_then(|i| rows.get(i).cloned())
            .map(|row| self.pair(&row, aspect, &p, cx));
        root.child(list).children(pair)
    }
}

impl EditorView {
    /// Compare Versions…: from the tracked baseline (or the newest version)
    /// to the board now.
    pub(crate) fn open_compare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editor.storyboard().is_none() {
            self.set_status("Compare works on storyboards.", false, cx);
            return;
        }
        let from = self
            .review_ui
            .changes
            .baseline
            .unwrap_or_else(|| self.default_baseline());
        let editor = cx.entity();
        let view = cx.new(|cx| CompareView::new(&editor, Some(from), cx));
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title("Compare versions")
                .width(px(760.))
                .child(view.clone())
                .footer(
                    div().flex().justify_end().child(
                        Button::new("compare-close")
                            .label("Close")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
                )
        });
    }
}
