//! Timing › Estimate durations from captions…: panel durations from the
//! words in their Dialogue and Action captions at word rates the person
//! sets (remembered in Settings), over the selected panels, the active
//! panel's scene or the whole board. The dialog previews old → new per panel
//! and per scene; Apply is one Undo step through the timeline's timing edit,
//! so layer keys follow the keyframe sync mode, transitions shorten to fit
//! and locked panels keep their length.
use super::storyboard_timeline::TimingEdit;
use super::*;
use crate::app_state;
use emulsion_core::project::PageId;
use emulsion_core::storyboard::FrameRate;
use emulsion_core::storyboard_estimate::{self as estimate, Estimate, Kept, WordRates};
use gpui_kit::component::{
    Disableable, Sizable, WindowExt,
    button::{Button, ButtonVariants},
};

/// Panels listed in the preview; the totals still cover every panel.
const MAX_ROWS: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EstimateScope {
    Selection,
    Scene,
    Board,
}

/// The number inputs, in order: dialogue wpm, action wpm, pause per line,
/// pause per parenthetical, minimum seconds.
const INPUTS: [(&str, &str); 5] = [
    ("estimate-dialogue-wpm", "Dialogue words/min"),
    ("estimate-action-wpm", "Action words/min"),
    ("estimate-line-pause", "Pause per line (s)"),
    (
        "estimate-parenthetical-pause",
        "Pause per parenthetical (s)",
    ),
    ("estimate-minimum", "Minimum panel (s)"),
];

fn values(rates: &WordRates) -> [f64; 5] {
    [
        rates.dialogue_wpm,
        rates.action_wpm,
        rates.line_pause,
        rates.parenthetical_pause,
        rates.minimum_seconds,
    ]
}

pub(crate) struct DurationEstimate {
    editor: WeakEntity<EditorView>,
    inputs: Vec<Entity<InputState>>,
    pub(crate) rates: WordRates,
    pub(crate) scope: EstimateScope,
    /// Why the typed rates cannot be used.
    pub(crate) error: Option<String>,
    _subs: Vec<Subscription>,
}

impl DurationEstimate {
    fn new(
        editor: Entity<EditorView>,
        scope: EstimateScope,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let rates = app_state::settings(cx).storyboard.duration_rates.clone();
        let inputs: Vec<_> = values(&rates)
            .into_iter()
            .map(|v| cx.new(|cx| InputState::new(window, cx).default_value(format!("{v}"))))
            .collect();
        let mut subs: Vec<_> = inputs
            .iter()
            .map(|input| {
                cx.subscribe(input, |this, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.read_inputs(cx);
                    }
                })
            })
            .collect();
        // Undo and edits elsewhere refresh the preview.
        subs.push(cx.observe(&editor, |_, _, cx| cx.notify()));
        Self {
            editor: editor.downgrade(),
            inputs,
            rates,
            scope,
            error: None,
            _subs: subs,
        }
    }

    fn read_inputs(&mut self, cx: &mut Context<Self>) {
        let typed: Vec<Option<f64>> = self
            .inputs
            .iter()
            .map(|i| i.read(cx).value().trim().parse::<f64>().ok())
            .collect();
        let mut next = self.rates.clone();
        let result = match typed[..] {
            [Some(d), Some(a), Some(l), Some(p), Some(m)] => {
                next.dialogue_wpm = d;
                next.action_wpm = a;
                next.line_pause = l;
                next.parenthetical_pause = p;
                next.minimum_seconds = m;
                next.validate()
            }
            _ => Err("Type numbers for every rate.".into()),
        };
        match result {
            Ok(()) => {
                self.rates = next;
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
        cx.notify();
    }

    /// Apply the estimate; true when the dialog can close.
    pub(crate) fn apply(&mut self, cx: &mut Context<Self>) -> bool {
        if self.error.is_some() {
            return false;
        }
        let (rates, scope) = (self.rates.clone(), self.scope);
        self.editor
            .update(cx, |e, cx| e.apply_duration_estimate(&rates, scope, cx))
            .unwrap_or(false)
    }
}

impl Render for DurationEstimate {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let preview = self
            .editor
            .upgrade()
            .map(|e| e.read(cx).duration_estimate(&self.rates, self.scope));
        let fields = INPUTS.iter().zip(&self.inputs).map(|((id, label), input)| {
            div()
                .id(*id)
                .flex()
                .flex_col()
                .gap(px(2.))
                .w(px(96.))
                .child(mono(*label, 9.5, p.muted))
                .child(Input::new(input).small())
        });
        let scope = |id: &'static str, text: &'static str, value: EstimateScope| {
            chip(id, text, self.scope == value, &p)
                .test_support()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.scope = value;
                    cx.notify();
                }))
        };
        let rows = preview.as_ref().map(|(estimate, names, rate)| {
            let secs = |f: u64| format!("{:.1}s", rate.frames_to_seconds(f));
            let mut list = div().flex().flex_col().gap(px(1.));
            for scene in &estimate.scenes {
                list = list.child(
                    div()
                        .flex()
                        .justify_between()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(scene.name.clone())
                        .child(format!(
                            "{} → {}",
                            secs(scene.old_frames),
                            secs(scene.new_frames)
                        )),
                );
                for panel in estimate
                    .panels
                    .iter()
                    .filter(|x| x.scene == scene.scene)
                    .take(MAX_ROWS)
                {
                    let note = match panel.kept {
                        Some(Kept::Locked) => " (locked)",
                        Some(Kept::NoText) => " (no text)",
                        None => "",
                    };
                    list = list.child(
                        div()
                            .flex()
                            .justify_between()
                            .pl(px(10.))
                            .text_color(if panel.kept.is_some() { p.muted } else { p.ink })
                            .child(format!(
                                "{}{note}",
                                names.get(&panel.panel).cloned().unwrap_or_default()
                            ))
                            .child(format!(
                                "{} → {}",
                                secs(u64::from(panel.old_frames)),
                                secs(u64::from(panel.new_frames))
                            )),
                    );
                }
            }
            list.child(
                div()
                    .id("estimate-total")
                    .test_support()
                    .flex()
                    .justify_between()
                    .border_t_1()
                    .border_color(p.line)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Total")
                    .child(format!(
                        "{} → {}",
                        secs(estimate.old_frames()),
                        secs(estimate.new_frames())
                    )),
            )
        });
        let changes = preview.as_ref().map_or(0, |(e, _, _)| e.changes().0.len());
        let fields_note = format!(
            "Counts the {} captions as dialogue and {} as action. Panels without that text keep their duration.",
            self.rates.dialogue_fields.join(", "),
            self.rates.action_fields.join(", ")
        );
        div()
            .id("duration-estimate")
            .test_support()
            .flex()
            .flex_col()
            .gap(px(10.))
            .text_size(px(12.))
            .child(div().flex().flex_wrap().gap(px(8.)).children(fields))
            .child(mono(fields_note, 10., p.muted))
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .child(scope(
                        "estimate-scope-selection",
                        "Selected panels",
                        EstimateScope::Selection,
                    ))
                    .child(scope("estimate-scope-scene", "Scene", EstimateScope::Scene))
                    .child(scope(
                        "estimate-scope-board",
                        "Whole board",
                        EstimateScope::Board,
                    )),
            )
            .child(
                div()
                    .id("estimate-preview")
                    .max_h(px(280.))
                    .overflow_y_scroll()
                    .p(px(8.))
                    .rounded(px(4.))
                    .bg(p.soft_bg)
                    .children(rows),
            )
            .children(self.error.clone().map(|message| {
                div()
                    .id("estimate-error")
                    .test_support()
                    .aria_label(message.clone())
                    .text_color(p.accent)
                    .child(message)
            }))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        Button::new("estimate-cancel")
                            .label("Cancel")
                            .small()
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("estimate-apply")
                            .label(if changes == 1 {
                                "Apply to 1 panel".to_string()
                            } else {
                                format!("Apply to {changes} panels")
                            })
                            .small()
                            .primary()
                            .disabled(self.error.is_some() || changes == 0)
                            .on_click(cx.listener(|this, _, window, cx| {
                                if this.apply(cx) {
                                    window.close_dialog(cx);
                                }
                            })),
                    ),
            )
    }
}

impl EditorView {
    /// Timing › Estimate durations from captions…
    pub(crate) fn open_duration_estimate(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<DurationEstimate>> {
        self.editor.storyboard()?;
        let scope = if self.board_selection().len() > 1 {
            EstimateScope::Selection
        } else {
            EstimateScope::Scene
        };
        let editor = cx.entity();
        let dialog = cx.new(|cx| DurationEstimate::new(editor, scope, window, cx));
        let shown = dialog.clone();
        window.open_dialog(cx, move |d, _, _| {
            d.title("Estimate durations from captions")
                .width(px(560.))
                .child(shown.clone())
        });
        Some(dialog)
    }

    fn page_order(&self) -> Vec<PageId> {
        self.editor.page_list().iter().map(|m| m.id).collect()
    }

    /// The panels `scope` covers, in any order.
    fn estimate_panels(&self, scope: EstimateScope) -> Vec<PageId> {
        let Some(board) = self.editor.storyboard() else {
            return Vec::new();
        };
        let layout = self.page_order();
        match scope {
            EstimateScope::Selection => self.board_selection(),
            EstimateScope::Scene => {
                let scene = board
                    .panels
                    .get(&self.editor.active_page())
                    .map(|p| p.scene);
                layout
                    .into_iter()
                    .filter(|id| board.panels.get(id).map(|p| p.scene) == scene)
                    .collect()
            }
            EstimateScope::Board => layout,
        }
    }

    /// The estimate for `scope`, with panel names and the frame rate.
    pub(crate) fn duration_estimate(
        &self,
        rates: &WordRates,
        scope: EstimateScope,
    ) -> (Estimate, HashMap<PageId, String>, FrameRate) {
        let names = self
            .editor
            .page_list()
            .iter()
            .map(|m| (m.id, m.name.clone()))
            .collect();
        let Some(board) = self.editor.storyboard() else {
            return (Estimate::default(), names, FrameRate::whole(24));
        };
        let panels = self.estimate_panels(scope);
        let found = estimate::estimate(board, &self.page_order(), &panels, rates);
        (found, names, board.settings.frame_rate)
    }

    /// Apply the estimate for `scope` as one Undo step and remember the
    /// rates. True when it applied (or nothing needed changing).
    pub(crate) fn apply_duration_estimate(
        &mut self,
        rates: &WordRates,
        scope: EstimateScope,
        cx: &mut Context<Self>,
    ) -> bool {
        let (found, _, _) = self.duration_estimate(rates, scope);
        let (panels, frames) = found.changes();
        if app_state::settings(cx).storyboard.duration_rates != *rates {
            let rates = rates.clone();
            app_state::update_settings(cx, |s| s.storyboard.duration_rates = rates);
        }
        if panels.is_empty() {
            self.set_status("The durations already match the captions.", false, cx);
            return true;
        }
        let count = panels.len();
        if !self.timeline_commit(TimingEdit::Frames { panels, frames }, cx) {
            return false;
        }
        self.set_status(
            format!(
                "Estimated {count} panel duration{} from the captions.",
                if count == 1 { "" } else { "s" }
            ),
            false,
            cx,
        );
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::open;
    use core::prelude::v1::test;
    use emulsion_core::project::{ProjectEditor, ProjectKind};

    #[gpui_kit::test]
    fn estimated_durations_preview_then_apply_as_one_undo_step(cx: &mut TestAppContext) {
        let (ws, cx) = open(cx, Document::new(64, 36));
        let mut project =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(80, 40)).unwrap();
        let blank = project.storyboard().unwrap().blank_panel().unwrap();
        let first = project.active_page();
        let ids = project
            .insert_panels(
                Some(first),
                &blank,
                vec![
                    ("A".into(), emulsion_core::storyboard::Panel::new(0, 48)),
                    ("B".into(), emulsion_core::storyboard::Panel::new(0, 48)),
                ],
                None,
            )
            .unwrap();
        project
            .edit_storyboard(|b| {
                let action = b.caption("Action").unwrap();
                let dialogue = b.caption("Dialogue").unwrap();
                let a = b.panels.get_mut(&ids[0]).unwrap();
                a.captions
                    .insert(action, "Mia runs to the window and looks out.".into());
                a.captions.insert(dialogue, "MIA: Who is out there?".into());
                let locked = b.panels.get_mut(&ids[1]).unwrap();
                locked.captions.insert(
                    dialogue,
                    "TOM: A long line that would change the length.".into(),
                );
                locked.locked = true;
                Ok(())
            })
            .unwrap();
        let e = cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.install_project(project, "Board".into(), window, cx)
            });
            ws.read(cx).editor.clone().unwrap()
        });
        cx.run_until_parked();
        let dialog = cx
            .update(|window, cx| e.update(cx, |e, cx| e.open_duration_estimate(window, cx)))
            .unwrap();
        cx.run_until_parked();
        cx.update(|_, cx| {
            dialog.update(cx, |d, cx| {
                d.scope = EstimateScope::Board;
                cx.notify();
            });
            let (found, _, _) = e
                .read(cx)
                .duration_estimate(&dialog.read(cx).rates, EstimateScope::Board);
            assert_eq!(found.panels.len(), 3);
            // 4 s of action, 1.6 s of speech and a 0.5 s pause at 24 fps.
            assert_eq!(found.panels[1].new_frames, 146);
            assert_eq!(found.panels[2].kept, Some(Kept::Locked));
            assert_eq!(found.changes().0, vec![ids[0]]);
        });
        // A bad rate is refused until fixed.
        cx.update(|window, cx| {
            let input = dialog.read(cx).inputs[0].clone();
            input.update(cx, |i, cx| i.set_value("0", window, cx));
            dialog.update(cx, |d, cx| d.read_inputs(cx));
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert!(dialog.read(cx).error.is_some());
            assert!(!dialog.update(cx, |d, cx| d.apply(cx)));
        });
        cx.update(|window, cx| {
            let input = dialog.read(cx).inputs[0].clone();
            input.update(cx, |i, cx| i.set_value("150", window, cx));
            dialog.update(cx, |d, cx| d.read_inputs(cx));
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert!(dialog.read(cx).error.is_none());
            assert!(dialog.update(cx, |d, cx| d.apply(cx)));
            let board = e.read(cx).editor.storyboard().unwrap().clone();
            assert_eq!(board.panels[&ids[0]].frames, 146);
            assert_eq!(
                board.panels[&ids[1]].frames, 48,
                "locked panels keep theirs"
            );
        });
        cx.update(|_, cx| e.update(cx, |e, cx| e.undo(cx)));
        cx.update(|_, cx| {
            let board = e.read(cx).editor.storyboard().unwrap().clone();
            assert_eq!(board.panels[&ids[0]].frames, 48, "one Undo step");
        });
    }
}
