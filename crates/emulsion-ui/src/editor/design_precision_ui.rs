use super::*;
use emulsion_core::design_precision::{self, Settings, Unit};
use gpui_kit::component::{Selectable, Sizable, WindowExt, button::Button};
struct Form {
    unit: Unit,
    origin_x: Entity<InputState>,
    origin_y: Entity<InputState>,
    x: Entity<InputState>,
    y: Entity<InputState>,
    gap: Entity<InputState>,
    vertical: bool,
}
fn number(input: &Entity<InputState>, cx: &App) -> Result<f64, String> {
    input
        .read(cx)
        .value()
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or("Enter finite numeric values.".into())
}
impl Render for Form {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().flex().flex_col().gap_2().child(div().flex().gap_1().children(Unit::ALL.into_iter().enumerate().map(|(i,unit)|Button::new(("precision-unit",i)).small().outline().label(unit.label()).selected(self.unit==unit).on_click(cx.listener(move|this,_,_,cx|{this.unit=unit;cx.notify();})))))
 .child("Ruler origin in document pixels · artwork stays in place")
 .child(div().flex().gap_2().child(Input::new(&self.origin_x).id("precision-origin-x")).child(Input::new(&self.origin_y).id("precision-origin-y")))
 .child("Optional object position · relative to origin, in selected units")
 .child(div().flex().gap_2().child(Input::new(&self.x).id("precision-x")).child(Input::new(&self.y).id("precision-y")))
 .child("Optional exact gap · uses selected objects, in selected units")
 .child(Input::new(&self.gap).id("precision-gap"))
 .child(Button::new("precision-axis").small().label(if self.vertical{"Vertical spacing"}else{"Horizontal spacing"}).on_click(cx.listener(|this,_,_,cx|{this.vertical= !this.vertical;cx.notify();})))
 .child("Physical units use the document print resolution. Position requires one object; spacing requires two or more. Native paths use geometric bounds; other layers use artwork bounds.")
    }
}
impl EditorView {
    pub(crate) fn show_design_precision(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let settings = self.editor.doc.design.precision;
        let form = cx.new(|cx| Form {
            unit: settings.unit,
            origin_x: cx.new(|cx| {
                InputState::new(window, cx).default_value(settings.origin[0].to_string())
            }),
            origin_y: cx.new(|cx| {
                InputState::new(window, cx).default_value(settings.origin[1].to_string())
            }),
            x: cx.new(|cx| InputState::new(window, cx)),
            y: cx.new(|cx| InputState::new(window, cx)),
            gap: cx.new(|cx| InputState::new(window, cx)),
            vertical: false,
        });
        let owner = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let owner = owner.clone();
            let form = form.clone();
            dialog
                .title("Rulers and precise placement")
                .width(px(520.))
                .child(form.clone())
                .footer(crate::widgets::form_dialog_footer("Apply"))
                .on_ok(move |_, _, cx| {
                    owner
                        .update(cx, |this, cx| {
                            let result = (|| {
                                let f = form.read(cx);
                                let mut trial =
                                    emulsion_core::Editor::new(this.editor.doc.clone(), None);
                                design_precision::set(
                                    &mut trial,
                                    Settings {
                                        unit: f.unit,
                                        origin: [
                                            number(&f.origin_x, cx)?,
                                            number(&f.origin_y, cx)?,
                                        ],
                                    },
                                )?;
                                let ids = this.selected_layer_roots();
                                let has_pos = !f.x.read(cx).value().trim().is_empty()
                                    || !f.y.read(cx).value().trim().is_empty();
                                if has_pos {
                                    if ids.len() != 1 {
                                        return Err(
                                            "Position requires exactly one selected object."
                                                .to_string(),
                                        );
                                    }
                                    design_precision::position(
                                        &mut trial,
                                        ids[0],
                                        number(&f.x, cx)?,
                                        number(&f.y, cx)?,
                                    )?;
                                }
                                if !f.gap.read(cx).value().trim().is_empty() {
                                    design_precision::spacing(
                                        &mut trial,
                                        &ids,
                                        f.vertical,
                                        number(&f.gap, cx)?,
                                    )?;
                                }
                                this.editor
                                    .commit_design_document(trial.doc, "Precise placement")?;
                                Ok::<_, String>(())
                            })();
                            match result {
                                Ok(()) => {
                                    this.rulers = true;
                                    this.after_change(cx);
                                    true
                                }
                                Err(e) => {
                                    this.set_status(e, true, cx);
                                    false
                                }
                            }
                        })
                        .unwrap_or(false)
                })
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    use gpui::TestAppContext;
    use gpui_kit::test::TestWindowExt;
    #[gpui_kit::test]
    fn design_precision_form_invalid_dependent_spacing_preserves_settings(cx: &mut TestAppContext) {
        let original = Document::new(400, 300);
        let (ws, cx) = crate::tests::open(cx, original.clone());
        let view = cx.update(|_, cx| ws.read(cx).editor.clone().unwrap());
        cx.update(|window, cx| view.update(cx, |this, cx| this.show_design_precision(window, cx)));
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.click(("precision-unit", 1usize), cx);
            window.click("precision-gap", cx);
        });
        cx.simulate_input("10");
        cx.run_until_parked();
        cx.update(|window, cx| window.click("ok", cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert_eq!(view.read(cx).editor.doc, original);
            assert!(window.find("precision-gap").visible());
        });
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
    }
}
