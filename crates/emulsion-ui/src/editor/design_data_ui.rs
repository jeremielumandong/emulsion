//! Local CSV binding controls for selected editable text and images.
use super::*;
use emulsion_core::design_data::{self, Binding, Fit};
use gpui_kit::component::{Selectable, Sizable, WindowExt, button::Button};
impl EditorView {
    pub(crate) fn design_data_binding_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let Some(id) = self
            .selected
            .map(|id| design_data::target(&self.editor.doc, id))
        else {
            self.set_status(
                "Select a text object or image to bind a CSV column.",
                true,
                cx,
            );
            return;
        };
        let image = match self.editor.doc.node(id).map(|n| &n.kind) {
            Some(NodeKind::Text { .. }) => false,
            Some(NodeKind::Raster { .. }) => true,
            _ => {
                self.set_status(
                    "Select editable text, a raster image, or an image frame.",
                    true,
                    cx,
                );
                return;
            }
        };
        let existing = self.editor.doc.design.data_bindings.get(&id);
        let column = cx.new(|cx| {
            InputState::new(window, cx).default_value(existing.map_or("", Binding::column))
        });
        let (initial_fit, initial_focus) = match existing {
            Some(Binding::Image { fit, focus, .. }) => (*fit, *focus),
            _ => (Fit::Cover, [0.5; 2]),
        };
        let fit = cx.new(|_| initial_fit);
        let focus = initial_focus.map(|v| {
            cx.new(|cx| InputState::new(window, cx).default_value((v * 100.).to_string()))
        });
        let error = cx.new(|_| String::new());
        let remove = cx.new(|_| false);
        let ticket = self.edit_ticket();
        let owner = cx.weak_entity();
        window.open_dialog(cx,move|dialog,_,cx| {
            let owner=owner.clone();let column=column.clone();let focus=focus.clone();let chosen=fit.clone();let removal=remove.clone();let error_apply=error.clone();
            dialog.title("Bind CSV data").width(px(460.)).child(
                div().flex().flex_col().gap_2()
                .child(if image {"Replace this image from a local file path in each CSV record."} else {"Replace this text from each CSV record, keeping its starting text style."})
                .child("CSV column").child(Input::new(&column).id("design-data-column"))
                .when(image,|d|d.child(div().flex().gap_1().children([(Fit::Cover,"Cover"),(Fit::Contain,"Contain"),(Fit::Stretch,"Stretch")].into_iter().enumerate().map(|(i,(value,label))| {
                    let change=fit.clone();Button::new(("design-data-fit",i)).label(label).small().outline().selected(*fit.read(cx)==value).on_click(move|_,window,cx|{change.update(cx,|v,cx|{*v=value;cx.notify();});window.refresh();})
                }))).child("Focal point X / Y (%)").child(Input::new(&focus[0]).id("design-data-focus-x")).child(Input::new(&focus[1]).id("design-data-focus-y")))
                .child({let change=remove.clone();Button::new("design-data-remove").label("Remove this binding").small().outline().selected(*remove.read(cx)).on_click(move|_,window,cx|{change.update(cx,|v,cx|{*v= !*v;cx.notify();});window.refresh();})})
            ).footer(div().flex().flex_col().gap_2().when(!error.read(cx).is_empty(),|d|d.child(div().id("design-data-error").test_support().child(error.read(cx).clone()))).child(crate::widgets::form_dialog_footer("Save binding")))
            .on_ok(move|_,_,cx| {
                let result=(||->Result<(),String>{
                    let binding=if *removal.read(cx) {None} else {
                        let column=column.read(cx).value().trim().to_string();
                        Some(if image {
                            let mut position=[0.;2];for i in 0..2 {position[i]=focus[i].read(cx).value().trim().parse::<f64>().map_err(|_|"Enter a focal point between 0 and 100%.")?/100.;}
                            Binding::Image{column,fit:*chosen.read(cx),focus:position}
                        } else {Binding::Text{column}})
                    };
                    owner.update(cx,|this,cx| {
                        if this.edit_ticket()!=ticket {return Err("The page changed. Reopen this dialog.".into());}
                        design_data::set(&mut this.editor,id,binding)?;this.after_change(cx);Ok(())
                    }).unwrap_or_else(|_|Err("The editor closed.".into()))
                })();
                match result {Ok(())=>true,Err(message)=>{error_apply.update(cx,|v,cx|{*v=message;cx.notify();});false}}
            })
        });
    }
}
