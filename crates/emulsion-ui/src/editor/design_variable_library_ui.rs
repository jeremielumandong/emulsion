//! Native project variable management using grouped page history.
use super::*;
use emulsion_core::design_variable_project as library;
use gpui_kit::component::{
    Sizable, WindowExt,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
};
impl EditorView {
    pub(super) fn design_variable_library_buttons(
        &self,
        name: &str,
        index: usize,
        cx: &Context<Self>,
    ) -> AnyElement {
        let linked = self.editor.doc.design.variable_libraries.contains_key(name);
        div()
            .flex()
            .flex_wrap()
            .gap_1()
            .children(
                (if linked {
                    vec![
                        ("publish", "Publish value"),
                        ("rename", "Rename across project…"),
                        ("detach", "Make local"),
                        ("remove", "Remove across project"),
                    ]
                } else {
                    vec![("share", "Share across project")]
                })
                .into_iter()
                .enumerate()
                .map(|(i, (action, label))| {
                    let name = name.to_owned();
                    Button::new((ElementId::from("project-variable"), format!("{index}-{i}")))
                        .label(label)
                        .small()
                        .ghost()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if action == "rename" {
                                this.project_variable_rename_dialog(name.clone(), window, cx);
                            } else {
                                this.project_variable_action(action, &name, cx);
                            }
                        }))
                }),
            )
            .into_any_element()
    }
    fn project_variable_action(&mut self, action: &str, name: &str, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let result = match action {
            "share" => library::share(&mut self.editor, name).map(|_| ()),
            "publish" => library::publish(&mut self.editor, name).map(|_| ()),
            "remove" => library::remove(&mut self.editor, name),
            _ => library::detach(&mut self.editor, name),
        };
        match result {
            Ok(()) => {
                self.after_change(cx);
                self.set_status(
                    "Project variable updated. Undo restores every affected page.",
                    false,
                    cx,
                );
            }
            Err(e) => self.set_status(e, true, cx),
        }
    }
    fn project_variable_rename_dialog(
        &mut self,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let input = cx.new(|cx| InputState::new(window, cx).default_value(&name));
        let error = cx.new(|_| String::new());
        let ticket = self.edit_ticket();
        let owner = cx.weak_entity();
        window.open_dialog(cx,move|dialog,_,cx|{let input=input.clone();let owner=owner.clone();let name=name.clone();let error_apply=error.clone();
   dialog.title("Rename project variable").width(px(420.)).child("Rename this shared variable on all linked pages. Existing object bindings follow the new name.").child(Input::new(&input).id("project-variable-name")).when(!error.read(cx).is_empty(),|d|d.child(error.read(cx).clone())).footer(crate::widgets::form_dialog_footer("Rename across project")).on_ok(move|_,window,cx|{
    let to=input.read(cx).value().to_string();let result=owner.update(cx,|this,cx|{if this.edit_ticket()!=ticket{return Err("The page changed. Reopen variable settings.".into());}library::rename(&mut this.editor,&name,&to)?;this.after_change(cx);Ok::<_,String>(())}).unwrap_or_else(|_|Err("The editor closed.".into()));
    match result{Ok(())=>true,Err(e)=>{error_apply.update(cx,|v,cx|{*v=e;cx.notify();});window.refresh();false}}
   })
  });
    }
    pub(super) fn project_variable_import_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let active = self.editor.active_page();
        let choices: Vec<_> = self
            .editor
            .page_list()
            .iter()
            .filter(|p| p.id != active)
            .flat_map(|p| {
                self.editor
                    .page(p.id)
                    .unwrap()
                    .doc
                    .design
                    .variables
                    .keys()
                    .map(|name| (p.id, name.clone(), format!("{} · {name}", p.name)))
                    .collect::<Vec<_>>()
            })
            .collect();
        if choices.is_empty() {
            self.set_status("Create a variable on another project page first.", true, cx);
            return;
        }
        let selected = cx.new(|_| 0usize);
        let input = cx.new(|cx| InputState::new(window, cx).default_value(&choices[0].1));
        let error = cx.new(|_| String::new());
        let ticket = self.edit_ticket();
        let owner = cx.weak_entity();
        window.open_dialog(cx,move|dialog,_,cx|{
   let choice=*selected.read(cx);let picker=selected.clone();let menu_choices=choices.clone();let selected_apply=selected.clone();let choices_apply=choices.clone();let input=input.clone();let owner=owner.clone();let error_apply=error.clone();
   dialog.title("Import project variable").width(px(460.)).child("Link a variable from another page. Local edits publish only when you choose Publish value.")
    .child(Button::new("project-variable-source").label(choices[choice].2.clone()).small().outline().dropdown_menu(move|mut menu,_,_|{for (i,(_,_,label)) in menu_choices.iter().enumerate(){let picker=picker.clone();menu=menu.item(PopupMenuItem::new(label.clone()).on_click(move|_,window,cx|{picker.update(cx,|v,cx|{*v=i;cx.notify();});window.refresh();}));}menu}))
    .child(Input::new(&input).id("project-variable-import-name")).when(!error.read(cx).is_empty(),|d|d.child(error.read(cx).clone())).footer(crate::widgets::form_dialog_footer("Import variable"))
    .on_ok(move|_,window,cx|{let target=input.read(cx).value().to_string();let (page,name,_)=&choices_apply[*selected_apply.read(cx)];let result=owner.update(cx,|this,cx|{if this.edit_ticket()!=ticket{return Err("The page changed. Reopen import.".into());}library::import(&mut this.editor,*page,name,&target)?;this.after_change(cx);Ok::<_,String>(())}).unwrap_or_else(|_|Err("The editor closed.".into()));match result{Ok(())=>true,Err(e)=>{error_apply.update(cx,|v,cx|{*v=e;cx.notify();});window.refresh();false}}})
  });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    use gpui_kit::test::TestWindowExt;
    #[gpui_kit::test]
    fn project_variable_native_import_dialog_validates_and_undo_restores_both_pages(
        cx: &mut TestAppContext,
    ) {
        use emulsion_core::{
            design_variables as variables,
            project::{ProjectEditor, ProjectKind},
        };
        let mut project =
            ProjectEditor::new_project(ProjectKind::Design, Document::new(400, 300)).unwrap();
        let source = project.active_page();
        variables::set(&mut project, "Gap", variables::Value::Number(16.)).unwrap();
        project
            .add_page(Document::new(400, 300), "Destination".into(), 0.)
            .unwrap();
        let (ws, cx) = crate::tests::open(cx, Document::new(400, 300));
        cx.simulate_resize(size(px(1200.), px(900.)));
        let view = cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.install_project(project, "Variables".into(), window, cx)
            });
            let view = ws.read(cx).editor.clone().unwrap();
            view.update(cx, |v, cx| v.project_variable_import_dialog(window, cx));
            view
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.click("project-variable-import-name", cx));
        cx.simulate_keystrokes("ctrl-a");
        cx.simulate_input(" ");
        cx.update(|window, cx| window.click("ok", cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(view.read(cx).editor.doc.design.variables.is_empty());
            assert!(window.find("ok").visible());
            window.click("project-variable-import-name", cx);
        });
        cx.simulate_keystrokes("ctrl-a");
        cx.simulate_input("Local gap");
        cx.update(|window, cx| window.click("ok", cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            view.update(cx, |v, _| {
                assert_eq!(
                    v.editor.doc.design.variables["Local gap"],
                    variables::Value::Number(16.)
                );
                assert_eq!(
                    v.editor.doc.design.variable_libraries["Local gap"],
                    v.editor.page(source).unwrap().doc.design.variable_libraries["Gap"]
                );
                assert!(v.editor.undo());
                assert!(v.editor.doc.design.variables.is_empty());
                assert!(
                    v.editor
                        .page(source)
                        .unwrap()
                        .doc
                        .design
                        .variable_libraries
                        .is_empty()
                );
            })
        });
    }
}
