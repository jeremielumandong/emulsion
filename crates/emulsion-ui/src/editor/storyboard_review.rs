//! Storyboard review in the editor: the Review section of the Panel
//! inspector (status and notes), review status badges and the status filter
//! on the Board, and review layers (New Review Layer, and marking a layer
//! as one). Review layers draw on the Stage; exports leave them out through
//! `emulsion_core::storyboard_review::printable`.
use super::*;
use emulsion_core::project::PageId;
use emulsion_core::storyboard::{REVIEW_STATUSES, ReviewStatus, Storyboard};
use emulsion_core::storyboard_review::{self as review, REVIEW_RGB};
use gpui_kit::component::{
    Sizable,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
};

/// Which panels the Board shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum BoardFilter {
    #[default]
    All,
    Status(ReviewStatus),
    /// Panels with unresolved notes.
    OpenNotes,
}

impl BoardFilter {
    fn label(self) -> String {
        match self {
            Self::All => "All panels".into(),
            Self::Status(status) => status.label().into(),
            Self::OpenNotes => "Open notes".into(),
        }
    }
    pub(crate) fn allows(self, board: &Storyboard, panel: PageId) -> bool {
        let Some(review) = board.panels.get(&panel).map(|p| &p.review) else {
            return false;
        };
        match self {
            Self::All => true,
            Self::Status(status) => review.status == status,
            Self::OpenNotes => review.open_notes().next().is_some(),
        }
    }
}

/// Review and change-tracking state of one editor.
#[derive(Default)]
pub(crate) struct ReviewUi {
    pub(crate) filter: BoardFilter,
    /// The note being written in the inspector.
    note: Option<Entity<InputState>>,
    _note_sub: Option<Subscription>,
    pub(crate) changes: super::storyboard_changes::ChangeMarks,
}

pub(crate) fn status_color(status: ReviewStatus) -> Hsla {
    rgb(status.rgb()).into()
}

impl EditorView {
    pub(crate) fn set_review_status(
        &mut self,
        panels: Vec<PageId>,
        status: ReviewStatus,
        cx: &mut Context<Self>,
    ) {
        // Locked panels can still be reviewed.
        self.edit_board(
            |b| {
                for panel in panels {
                    b.set_review_status(panel, status)?;
                }
                Ok(())
            },
            cx,
        );
    }

    /// Add a note to `panel` as one Undo step, signed with the author name
    /// in Settings. Returns whether it was added.
    pub(crate) fn add_review_note_text(
        &mut self,
        panel: PageId,
        text: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let author = crate::app_state::settings(cx)
            .storyboard
            .review_author
            .trim()
            .to_string();
        self.edit_board(
            |b| {
                b.add_review_note(panel, &author, text, review::now())
                    .map(|_| ())
            },
            cx,
        )
    }

    /// Add the inspector's note to `panel` and clear the box.
    fn add_review_note(&mut self, panel: PageId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(input) = self.review_ui.note.clone() else {
            return;
        };
        let text = input.read(cx).value().trim().to_string();
        if !text.is_empty() && self.add_review_note_text(panel, &text, cx) {
            input.update(cx, |input, cx| input.set_value("", window, cx));
        }
    }

    /// Layer › New Review Layer: an empty layer that draws on the Stage but
    /// never prints, above the selection.
    pub(crate) fn new_review_layer(&mut self, cx: &mut Context<Self>) -> Option<NodeId> {
        if self.editor.storyboard().is_none() {
            self.set_status("Review layers belong to storyboard panels.", false, cx);
            return None;
        }
        if self.refuse_locked_panel(cx) {
            return None;
        }
        let count = self.editor.doc.nodes.iter().filter(|n| n.review).count();
        let name = if count == 0 {
            "Review".to_string()
        } else {
            format!("Review {}", count + 1)
        };
        let node = review::review_layer(&self.editor.doc, &name);
        let slot = self.insertion_slot();
        let id = self.execute(
            Command::AddNode {
                node: Box::new(node),
                slot,
            },
            cx,
        )?;
        self.set_layer_selection(vec![id], Some(id));
        self.set_status(
            "New review layer: it shows on the Stage and never prints or exports.",
            false,
            cx,
        );
        Some(id)
    }

    /// Mark the selected layers as review layers, or as printing ones.
    fn mark_review_layers(&mut self, review: bool, cx: &mut Context<Self>) {
        let commands = self
            .selected_layer_ids()
            .into_iter()
            .map(|id| Command::SetReview { id, review })
            .collect::<Vec<_>>();
        if !commands.is_empty() {
            let name = if review {
                "Make review layer"
            } else {
                "Make printing layer"
            };
            self.execute_layer_commands(name, commands, cx);
        }
    }

    /// The Board's review status filter.
    pub(crate) fn board_filter_allows(&self, board: &Storyboard, panel: PageId) -> bool {
        self.review_ui.filter.allows(board, panel)
    }

    /// Board toolbar: the review filter.
    pub(crate) fn review_board_tools(&self, cx: &Context<Self>) -> AnyElement {
        let owner = cx.weak_entity();
        let current = self.review_ui.filter;
        Button::new("board-review-filter")
            .label(format!("Show: {} ▾", current.label()))
            .tooltip("Show only panels with a review status or open notes")
            .small()
            .ghost()
            .dropdown_menu(move |mut menu, _, _| {
                let choices = std::iter::once(BoardFilter::All)
                    .chain(
                        REVIEW_STATUSES
                            .iter()
                            .map(|(status, ..)| BoardFilter::Status(*status)),
                    )
                    .chain([BoardFilter::OpenNotes]);
                for choice in choices {
                    let owner = owner.clone();
                    menu = menu.item(
                        PopupMenuItem::new(choice.label())
                            .checked(choice == current)
                            .on_click(move |_, _, cx| {
                                owner
                                    .update(cx, |e, cx| {
                                        e.review_ui.filter = choice;
                                        cx.notify();
                                    })
                                    .ok();
                            }),
                    );
                }
                menu
            })
            .into_any_element()
    }

    /// The review badge on a Board card: status and open notes.
    pub(crate) fn review_badge(&self, panel: PageId, p: &Palette) -> Option<AnyElement> {
        let review = &self.editor.storyboard()?.panels.get(&panel)?.review;
        let open = review.open_notes().count();
        if review.status == ReviewStatus::None && open == 0 {
            return None;
        }
        let mut text = String::new();
        if review.status != ReviewStatus::None {
            text.push_str(review.status.label());
        }
        if open > 0 {
            if !text.is_empty() {
                text.push_str(" · ");
            }
            text.push_str(&format!("{open} note{}", if open == 1 { "" } else { "s" }));
        }
        let color = if review.status == ReviewStatus::None {
            rgb(REVIEW_RGB).into()
        } else {
            status_color(review.status)
        };
        Some(
            div()
                .id(("board-review-badge", panel))
                .test_support()
                .absolute()
                .bottom_1()
                .right_1()
                .px_1()
                .rounded(px(3.))
                .bg(color)
                .text_color(p.accent_fg)
                .text_size(px(10.))
                .child(text)
                .into_any_element(),
        )
    }

    /// The Review section of the Panel inspector.
    pub(crate) fn review_section(
        &mut self,
        panel: PageId,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.review_ui.note.is_none() {
            let input = cx.new(|cx| {
                InputState::new(window, cx).placeholder("Add a review note and press Enter")
            });
            let sub = cx.subscribe_in(&input, window, |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    let panel = this.editor.active_page();
                    this.add_review_note(panel, window, cx);
                }
            });
            self.review_ui.note = Some(input);
            self.review_ui._note_sub = Some(sub);
        }
        let Some(data) = self
            .editor
            .storyboard()
            .and_then(|b| b.panels.get(&panel))
            .map(|p| p.review.clone())
        else {
            return div().into_any_element();
        };
        let owner = cx.weak_entity();
        let current = data.status;
        let status_menu = Button::new("storyboard-review-status")
            .label(format!("{} ▾", current.label()))
            .small()
            .outline()
            .dropdown_menu(move |mut menu, _, _| {
                for (status, label, _) in REVIEW_STATUSES {
                    let owner = owner.clone();
                    menu = menu.item(
                        PopupMenuItem::new(label)
                            .checked(status == current)
                            .on_click(move |_, _, cx| {
                                owner
                                    .update(cx, |e, cx| {
                                        e.set_review_status(vec![panel], status, cx)
                                    })
                                    .ok();
                            }),
                    );
                }
                menu
            });
        let mut notes = div().flex().flex_col().gap(px(6.));
        for note in &data.notes {
            let id = note.id;
            let resolved = note.resolved;
            let who = if note.author.is_empty() {
                "Unsigned".to_string()
            } else {
                note.author.clone()
            };
            notes = notes.child(
                div()
                    .id(("storyboard-review-note", id))
                    .test_support()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .p(px(6.))
                    .rounded(px(4.))
                    .bg(p.soft_bg)
                    .when(resolved, |d| d.opacity(0.6))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(mono(
                                format!("{who} · {}", emulsion_io::recent::ago(note.time)),
                                10.,
                                p.muted,
                            ))
                            .child(div().flex_1())
                            .child(
                                Button::new(("storyboard-review-resolve", id))
                                    .label(if resolved { "Reopen" } else { "Resolve" })
                                    .xsmall()
                                    .ghost()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.edit_board(
                                            |b| b.resolve_review_note(panel, id, !resolved),
                                            cx,
                                        );
                                    })),
                            )
                            .child(
                                Button::new(("storyboard-review-delete", id))
                                    .label("Delete")
                                    .xsmall()
                                    .ghost()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.edit_board(|b| b.remove_review_note(panel, id), cx);
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .text_color(p.ink)
                            .when(resolved, |d| d.line_through())
                            .child(note.text.clone()),
                    ),
            );
        }
        let selected: Vec<NodeId> = self.selected_layer_ids();
        let marked = !selected.is_empty()
            && selected
                .iter()
                .all(|id| self.editor.doc.node(*id).is_some_and(|n| n.review));
        let input = self.review_ui.note.clone().expect("created above");
        div()
            .id("storyboard-review")
            .test_support()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(label("Review", p))
                    .child(div().flex_1())
                    .child(status_menu),
            )
            .child(notes)
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .child(
                        div()
                            .id("storyboard-review-input")
                            .test_support()
                            .flex_1()
                            .child(Input::new(&input).small()),
                    )
                    .child(
                        Button::new("storyboard-review-add")
                            .label("Add note")
                            .small()
                            .outline()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.add_review_note(panel, window, cx)
                            })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(6.))
                    .child(
                        Button::new("storyboard-review-layer")
                            .label("New review layer")
                            .tooltip("A layer for review marks: it shows on the Stage and never prints or exports")
                            .xsmall()
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.new_review_layer(cx);
                            })),
                    )
                    .when(!selected.is_empty(), |row| {
                        row.child(
                            Button::new("storyboard-review-mark")
                                .label(if marked {
                                    "Make selected layer print"
                                } else {
                                    "Make selected layer review-only"
                                })
                                .xsmall()
                                .ghost()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.mark_review_layers(!marked, cx)
                                })),
                        )
                    }),
            )
            .into_any_element()
    }
}
