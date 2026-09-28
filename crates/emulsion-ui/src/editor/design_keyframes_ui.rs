//! Property keyframes share the native motion inspector and Undo history.
use super::*;
use emulsion_core::design_keyframes::{self as keyframes, Easing, Keyframe, Property};
impl EditorView {
    pub(super) fn property_keyframes_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let Some(id) = self.selected else { return };
        let saved = self
            .editor
            .doc
            .design
            .keyframes
            .get(&id)
            .cloned()
            .unwrap_or_default();
        let property = Rc::new(std::cell::Cell::new(Property::TranslationX));
        let easing = Rc::new(std::cell::Cell::new(Easing::Linear));
        let remove = Rc::new(std::cell::Cell::new(false));
        let time = cx.new(|cx| InputState::new(window, cx).default_value("0"));
        let value = cx.new(|cx| InputState::new(window, cx).default_value("0"));
        let error = Rc::new(RefCell::new(String::new()));
        let owner = cx.weak_entity();
        let ticket = self.edit_ticket();
        let duration = self.editor.doc.design.duration_ms;
        window.open_dialog(cx,move|dialog,_,_|{
            let mut body=div().id("design-keyframes-body").flex().flex_col().gap_2().max_h(px(430.)).overflow_y_scroll();
            let pick=property.clone();let field=value.clone();
            body=body.child(Button::new("design-keyframe-property").label(format!("{} ▾",property.get().label())).outline().dropdown_menu(move|mut menu,_,_|{
                for p in Property::ALL{let pick=pick.clone();let field=field.clone();menu=menu.item(PopupMenuItem::new(p.label()).on_click(move|_,window,cx|{pick.set(p);field.update(cx,|state,cx|state.set_value(p.initial().to_string(),window,cx));cx.refresh_windows();}));}menu
            }));
            let pick=easing.clone();body=body.child(Button::new("design-keyframe-easing").label(format!("Easing: {} ▾",easing.get().label())).outline().dropdown_menu(move|mut menu,_,_|{
                for e in Easing::ALL{let pick=pick.clone();menu=menu.item(PopupMenuItem::new(e.label()).on_click(move|_,_,cx|{pick.set(e);cx.refresh_windows();}));}menu
            }));
            body=body.child(format!("Time · milliseconds (0–{duration})")).child(Input::new(&time).id("design-keyframe-time"))
                .child("Value · relative to authored appearance").child(Input::new(&value).id("design-keyframe-value"));
            let deleting=remove.clone();body=body.child(Button::new("design-keyframe-action").label(if remove.get(){"Action: delete keyframe"}else{"Action: save keyframe"}).outline().on_click(move|_,_,cx|{deleting.set(!deleting.get());cx.refresh_windows();}));
            for track in &saved {for frame in &track.frames{
                let p=track.property;let f=*frame;let property=property.clone();let easing=easing.clone();let time=time.clone();let value=value.clone();
                body=body.child(Button::new(SharedString::from(format!("design-saved-keyframe-{p:?}-{}",f.time_ms))).label(format!("{} · {} ms → {}",p.label(),f.time_ms,f.value)).small().ghost().on_click(move|_,window,cx|{property.set(p);easing.set(f.easing);time.update(cx,|s,cx|s.set_value(f.time_ms.to_string(),window,cx));value.update(cx,|s,cx|s.set_value(f.value.to_string(),window,cx));cx.refresh_windows();}));
            }}
            body=body.child("Add at least two keyframes for interpolation. Easing applies from this keyframe to the next. Scale and opacity use multipliers; offsets and rotation start at zero. Preview animation evaluates a copy of the page.").child(error.borrow().clone());
            let time=time.clone();let value=value.clone();let owner=owner.clone();let property=property.clone();let easing=easing.clone();let remove=remove.clone();let error=error.clone();
            dialog.title("Property keyframes").width(px(500.)).child(body).footer(crate::widgets::form_dialog_footer("Apply keyframe")).on_ok(move|_,_,cx|{
                let result=(||->Result<_,String>{let time=time.read(cx).value().parse::<u32>().map_err(|_|"Enter a whole-number time.")?;let value=value.read(cx).value().parse::<f64>().map_err(|_|"Enter a numeric value.")?;Ok((time,value))})();
                let result=result.and_then(|(time,value)|owner.update(cx,|this,cx|{
                    if this.edit_ticket()!=ticket{return Err("The page changed. Open keyframes again.".into())}
                    if remove.get(){keyframes::remove_keyframe(&mut this.editor,id,property.get(),time)?}else{keyframes::set_keyframe(&mut this.editor,id,property.get(),Keyframe{time_ms:time,value,easing:easing.get()})?}
                    this.after_change(cx);Ok(())
                }).unwrap_or_else(|_|Err("The editor closed.".into())));
                match result{Ok(())=>true,Err(e)=>{*error.borrow_mut()=e;cx.refresh_windows();false}}
            })
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    use gpui_kit::test::TestWindowExt;
    use gpui::TestAppContext;
    #[gpui_kit::test]
    fn design_property_keyframe_authoring_cancel_save_and_undo(cx: &mut TestAppContext) {
        let mut editor = emulsion_core::Editor::new(Document::new(640, 480), None);
        let id = emulsion_core::design::media::insert_youtube(
            &mut editor,
            "https://youtu.be/M7lc1UVf-VE",
            (10., 20.),
            (400., 225.),
        )
        .unwrap();
        let original = editor.doc.clone();
        let (workspace, cx) = crate::tests::open(cx, editor.doc);
        let view = cx.update(|_, cx| workspace.read(cx).editor.clone().unwrap());
        cx.update(|window, cx| {
            view.update(cx, |this, cx| {
                this.set_layer_selection(vec![id], Some(id));
                this.property_keyframes_dialog(window, cx)
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.click("close", cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert_eq!(view.read(cx).editor.doc, original);
            view.update(cx, |this, cx| this.property_keyframes_dialog(window, cx));
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.click("design-keyframe-value", cx));
        cx.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-a"
        } else {
            "ctrl-a"
        });
        cx.simulate_input("42.5");
        cx.run_until_parked();
        cx.update(|window, cx| window.click("ok", cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            view.update(cx, |this, cx| {
                assert_eq!(
                    this.editor.doc.design.keyframes[&id][0].frames[0].value,
                    42.5
                );
                assert_eq!(this.editor.doc.nodes, original.nodes);
                let preview = emulsion_core::design_metadata::at_time(&this.editor.doc, 0).unwrap();
                assert_eq!(
                    emulsion_core::design::media::bounds(&preview, id)
                        .unwrap()
                        .0,
                    52.5
                );
                this.undo(cx);
                assert_eq!(this.editor.doc, original);
            })
        });
    }
}
