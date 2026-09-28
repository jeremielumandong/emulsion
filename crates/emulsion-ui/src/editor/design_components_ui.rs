//! Local component libraries and explicit linked-instance publishing.
use super::*;
use emulsion_core::design_components as components;
use gpui_kit::component::{
    Disableable, Sizable, WindowExt,
    button::{Button, ButtonVariants},
};

impl EditorView {
    pub(super) fn design_component_controls(&self, p: &Palette, cx: &Context<Self>) -> AnyElement {
        let linked = self
            .selected
            .and_then(|id| self.editor.doc.design.component_links.get(&id));
        let variants = linked
            .and_then(|link| self.editor.doc.design.components.get(&link.component))
            .map(|d| d.variants.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        div().id("design-component-controls").test_support().flex().flex_col().gap(px(5.))
            .child(div().text_color(p.muted).child("Reusable components"))
            .child(Button::new("design-component-create").label("Create from selection…").small().outline().disabled(self.selected_layer_roots().is_empty()).on_click(cx.listener(|this,_,window,cx|this.design_component_name(false,window,cx))))
            .child(Button::new("design-component-library").label("Browse project components…").small().outline().on_click(cx.listener(|this,_,window,cx|this.design_component_library(window,cx))))
            .when_some(linked,|d,link|d.child(div().text_size(px(11.)).child(format!("{} · {} · linked on this page",link.component,link.variant)))
                .child(Button::new("design-component-update").label("Update linked instances").small().outline().on_click(cx.listener(|this,_,_,cx|this.design_component_action("update",None,cx))))
                .child(Button::new("design-component-reset").label("Reset selected instance").small().outline().on_click(cx.listener(|this,_,_,cx|this.design_component_action("reset",None,cx))))
                .child(Button::new("design-component-variant").label("Save as new variant…").small().outline().on_click(cx.listener(|this,_,window,cx|this.design_component_name(true,window,cx))))
                .children(variants.into_iter().enumerate().map(|(index,variant)|Button::new(("design-component-switch",index)).label(format!("Switch to {variant}")).small().ghost().on_click(cx.listener(move|this,_,_,cx|this.design_component_action("reset",Some(&variant),cx)))))
                .child(Button::new("design-component-detach").label("Detach instance").small().ghost().on_click(cx.listener(|this,_,_,cx|this.design_component_action("detach",None,cx)))))
            .child(div().text_size(px(10.)).text_color(p.muted).child("Update and Reset replace child edits. Other pages keep independent copies. Detach before nesting components."))
            .into_any_element()
    }
    fn design_component_action(
        &mut self,
        action: &str,
        variant: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let Some(id) = self.selected else {
            return;
        };
        let result = match action {
            "update" => components::update(&mut self.editor, id, None),
            "detach" => components::detach(&mut self.editor, id),
            _ => components::reset(&mut self.editor, id, variant),
        };
        match result {
            Ok(()) => {
                self.after_change(cx);
                self.set_status(
                    "Component updated. Undo restores the previous objects.",
                    false,
                    cx,
                );
            }
            Err(e) => self.set_status(e, true, cx),
        }
    }
    pub(crate) fn design_component_name(
        &mut self,
        variant: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let ids = self.selected_layer_roots();
        let selected = self.selected;
        let ticket = self.edit_ticket();
        let owner = cx.weak_entity();
        let input = cx.new(|cx| {
            InputState::new(window, cx).default_value(if variant { "Variant" } else { "Component" })
        });
        window.open_dialog(cx, move |dialog, _, _| {
            let input = input.clone();
            let owner = owner.clone();
            let ids = ids.clone();
            dialog
                .title(if variant {
                    "Save component variant"
                } else {
                    "Create component"
                })
                .width(px(400.))
                .child(Input::new(&input).id("design-component-name"))
                .footer(crate::widgets::form_dialog_footer("Save"))
                .on_ok(move |_, _, cx| {
                    let name = input.read(cx).value().to_string();
                    owner
                        .update(cx, |this, cx| {
                            if this.edit_ticket() != ticket {
                                this.set_status(
                                    "The page changed. Open the component dialog again.",
                                    true,
                                    cx,
                                );
                                return false;
                            }
                            let result = if variant {
                                selected
                                    .ok_or("Select a linked component".to_string())
                                    .and_then(|id| {
                                        components::update(&mut this.editor, id, Some(&name))
                                            .map(|()| id)
                                    })
                            } else {
                                components::create(&mut this.editor, &ids, &name)
                            };
                            match result {
                                Ok(id) => {
                                    this.set_layer_selection(vec![id], Some(id));
                                    this.after_change(cx);
                                    this.set_tool(Tool::Move, cx);
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
    pub(crate) fn design_component_library(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let active = self.editor.active_page();
        let ticket = self.edit_ticket();
        let owner = cx.weak_entity();
        let pages = self
            .editor
            .snapshot()
            .map(|project| project.pages)
            .unwrap_or_default();
        let entries: Vec<_> = pages
            .iter()
            .flat_map(|page| {
                page.doc
                    .design
                    .components
                    .iter()
                    .flat_map(|(name, definition)| {
                        definition.variants.keys().map(|variant| {
                            (
                                page.meta.id,
                                page.meta.name.clone(),
                                name.clone(),
                                variant.clone(),
                            )
                        })
                    })
            })
            .collect();
        let pages = std::sync::Arc::new(pages);
        window.open_dialog(cx,move|dialog,_,_|{
            dialog.title("Project components").width(px(500.)).child(div().id("design-component-library-list").test_support().max_h(px(460.)).overflow_y_scroll().flex().flex_col().gap_2()
                .child("Insert a native editable instance. Importing from another page creates an independent library on this page.")
                .when(entries.is_empty(),|d|d.child("No components yet. Select artwork and choose Create from selection."))
                .children(entries.iter().enumerate().map(|(index,(page,page_name,name,variant))|{
                    let owner=owner.clone();let pages=pages.clone();let page=*page;let name=name.clone();let variant=variant.clone();
                    Button::new(("design-component-insert",index)).label(format!("{page_name} / {name} / {variant}")).small().outline().on_click(move|_,window,cx|{
                        let success=owner.update(cx,|this,cx|{
                            if this.edit_ticket()!=ticket{this.set_status("The page changed. Reopen the component library.",true,cx);return false;}
                            let result=if page==active{components::insert(&mut this.editor,&name,&variant,(24.,24.))}else{pages.iter().find(|p|p.meta.id==page).ok_or("Source page unavailable".into()).and_then(|p|components::import_and_insert(&mut this.editor,&p.doc,&name,&variant,(24.,24.)))};
                            match result{Ok(id)=>{this.set_layer_selection(vec![id],Some(id));this.after_change(cx);this.set_tool(Tool::Move,cx);true},Err(e)=>{this.after_change(cx);this.set_status(e,true,cx);false}}
                        }).unwrap_or(false);if success{window.close_dialog(cx);}
                    })
                })))
        });
    }
}
