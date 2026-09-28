use super::*;
use emulsion_core::design_interactions::{
    self as interactions, Action, OverlayOperation, Runtime, Trigger,
};

impl EditorView {
    pub(crate) fn presentation_media_visible(&self, node: NodeId) -> bool {
        let doc = self.motion.preview.as_ref().unwrap_or(&self.editor.doc);
        if let Some(top) = self
            .motion
            .session
            .as_ref()
            .and_then(|s| s.interactions.open_overlays.last())
            && node != *top
            && !doc.is_ancestor(*top, node)
        {
            return false;
        }
        let mut current = Some(node);
        while let Some(id) = current {
            let Some(item) = doc.node(id) else {
                return false;
            };
            if !item.visible || item.opacity <= 0. {
                return false;
            }
            if doc.design.overlays.contains(&id)
                && !self
                    .motion
                    .session
                    .as_ref()
                    .is_some_and(|s| s.interactions.open_overlays.contains(&id))
            {
                return false;
            }
            current = item.parent;
        }
        true
    }
    pub(crate) fn design_interactions_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let Some(node) = self.selected else {
            return;
        };
        let Some(selected) = self.editor.doc.node(node) else {
            return;
        };
        let can_overlay = selected.is_group() && selected.parent.is_none();
        let actions = cx.new(|_| {
            self.editor
                .doc
                .design
                .interactions
                .get(&node)
                .cloned()
                .unwrap_or_default()
        });
        let overlay = cx.new(|_| self.editor.doc.design.overlays.contains(&node));
        let trigger = cx.new(|_| {
            self.editor
                .doc
                .design
                .interaction_triggers
                .get(&node)
                .copied()
                .unwrap_or_default()
        });
        let overlays = self
            .editor
            .doc
            .nodes
            .iter()
            .filter(|n| n.is_group() && n.parent.is_none())
            .map(|n| (n.id, n.name.clone()))
            .collect::<Vec<_>>();
        let variants = self
            .editor
            .doc
            .design
            .component_links
            .iter()
            .filter_map(|(id, link)| {
                Some((
                    *id,
                    self.editor.doc.node(*id)?.name.clone(),
                    self.editor
                        .doc
                        .design
                        .components
                        .get(&link.component)?
                        .variants
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>(),
                ))
            })
            .collect::<Vec<_>>();
        let pages = self
            .editor
            .page_list()
            .iter()
            .map(|p| (p.id, p.name.clone()))
            .collect::<Vec<_>>();
        let current_page = self.editor.active_page();
        let owner = cx.weak_entity();
        let ticket = self.edit_ticket();
        let error = cx.new(|_| String::new());
        window.open_dialog(cx,move|dialog,window,cx|{
            let link_owner=owner.clone();let add=actions.clone();let apply=actions.clone();let overlay_toggle=overlay.clone();let overlay_apply=overlay.clone();let owner=owner.clone();let error_apply=error.clone();let trigger_pick=trigger.clone();let trigger_apply=trigger.clone();
            dialog.title("Object interactions").width(px(500.))
                .child(div().id("design-interactions-body").test_support().max_h(px((f32::from(window.viewport_size().height)-220.).clamp(100.,660.))).overflow_y_scroll().flex().flex_col().gap_2()
                    .child(Button::new("design-interaction-trigger").label(format!("Trigger: {} ▾",trigger.read(cx).label())).small().outline().dropdown_menu(move|mut menu,_,_|{for value in Trigger::ALL{let trigger=trigger_pick.clone();menu=menu.item(PopupMenuItem::new(value.label()).on_click(move|_,window,cx|{trigger.update(cx,|state,cx|{*state=value;cx.notify();});window.refresh();}));}menu}))
                    .child("Actions run during presentation only. Overlays are hidden until opened; Escape closes the top overlay before exiting.")
                    .child(Button::new("design-interaction-overlay").label(if *overlay.read(cx){"✓ This group is a presentation overlay"}else{"Use this group as a presentation overlay"}).small().outline().disabled(!can_overlay).on_click(move|_,window,cx|{overlay_toggle.update(cx,|v,cx|{*v= !*v;cx.notify();});window.refresh();}))
                    .child(Button::new("design-interaction-web-link").label("Edit saved web link…").small().outline().on_click(move|_,window,cx|{window.close_dialog(cx);link_owner.update(cx,|this,cx|this.show_design_hyperlink(node,window,cx)).ok();}))
                    .child(Button::new("design-interaction-add").label("Add action").small().outline().disabled(actions.read(cx).len()>=8).on_click(move|_,window,cx|{add.update(cx,|v,cx|{v.push(Action::Next);cx.notify();});window.refresh();}))
                    .children(actions.read(cx).iter().cloned().enumerate().map(|(index,action)|{
                        let state=actions.clone();let remove=actions.clone();let overlay_targets=overlays.clone();let component_targets=variants.clone();let slide_targets=pages.clone();
                        let options=[("Next slide",Action::Next),("Previous slide",Action::Previous),("Back",Action::Back),("Specific slide",Action::Slide{page:current_page}),("Show overlay",Action::Overlay{target:overlays.first().map_or(0,|v|v.0),operation:OverlayOperation::Show}),("Toggle overlay",Action::Overlay{target:overlays.first().map_or(0,|v|v.0),operation:OverlayOperation::Toggle}),("Hide overlay",Action::Overlay{target:overlays.first().map_or(0,|v|v.0),operation:OverlayOperation::Hide}),("Close top overlay",Action::CloseOverlay),("Switch component variant",Action::Variant{target:variants.first().map_or(0,|v|v.0),variant:variants.first().and_then(|v|v.2.first()).cloned().unwrap_or_default()})];
                        let title=match &action{Action::Next=>"Next slide",Action::Url{..}=>"Open web link",Action::Previous=>"Previous slide",Action::Back=>"Back",Action::Slide{..}=>"Specific slide",Action::Overlay{operation,..}=>match operation{OverlayOperation::Show=>"Show overlay",OverlayOperation::Hide=>"Hide overlay",OverlayOperation::Toggle=>"Toggle overlay"},Action::CloseOverlay=>"Close top overlay",Action::Variant{..}=>"Switch component variant"};
                        let type_state=state.clone();
                        let mut row=div().flex().flex_col().gap_2().child(div().flex().gap_2()
                            .child(Button::new(("design-interaction-type",index)).label(format!("{}. {title} ▾",index+1)).small().outline().dropdown_menu(move|mut menu,_,_|{
                                for (label,action) in options.clone(){let state=type_state.clone();menu=menu.item(PopupMenuItem::new(label).on_click(move|_,window,cx|{state.update(cx,|v,cx|{v[index]=action.clone();cx.notify();});window.refresh();}));}menu
                            }))
                            .child(Button::new(("design-interaction-remove",index)).label("Remove").small().ghost().on_click(move|_,window,cx|{remove.update(cx,|v,cx|{v.remove(index);cx.notify();});window.refresh();})));
                        match action{
                            Action::Url{url}=>{row=row.child(div().text_sm().child(url));},
                            Action::Slide{page}=>{row=row.child(Button::new(("design-interaction-target",index)).label(format!("Slide: {} ▾",slide_targets.iter().find(|v|v.0==page).map_or("Missing slide",|v|v.1.as_str()))).small().outline().dropdown_menu(move|mut menu,_,_|{for (page,name) in slide_targets.clone(){let state=state.clone();menu=menu.item(PopupMenuItem::new(name).on_click(move|_,window,cx|{state.update(cx,|v,cx|{v[index]=Action::Slide{page};cx.notify();});window.refresh();}));}menu}));},
                            Action::Overlay{target,operation}=>{row=row.child(Button::new(("design-interaction-target",index)).label(format!("Overlay: {} ▾",overlay_targets.iter().find(|v|v.0==target).map_or("Choose a group",|v|v.1.as_str()))).small().outline().dropdown_menu(move|mut menu,_,_|{for (target,name) in overlay_targets.clone(){let state=state.clone();menu=menu.item(PopupMenuItem::new(name).on_click(move|_,window,cx|{state.update(cx,|v,cx|{v[index]=Action::Overlay{target,operation};cx.notify();});window.refresh();}));}menu}));},
                            Action::Variant{target,variant}=>{row=row.child(Button::new(("design-interaction-target",index)).label(format!("Variant: {variant} ▾")).small().outline().dropdown_menu(move|mut menu,_,_|{for (target,name,variants) in component_targets.clone(){for variant in variants{let state=state.clone();menu=menu.item(PopupMenuItem::new(format!("{name} · {variant}")).on_click(move|_,window,cx|{state.update(cx,|v,cx|{v[index]=Action::Variant{target,variant:variant.clone()};cx.notify();});window.refresh();}));}}menu}));let _=target;},_=>(),
                        }row
                    }))
                    .child(div().text_size(px(11.)).child("Choosing an overlay target marks that top-level group as an overlay. Removing overlay status removes incoming overlay links. Slide navigation must be last.")))
                .footer(div().id("design-interactions-footer").test_support().flex().flex_col().gap_2().when(!error.read(cx).is_empty(),|d|d.child(div().id("design-interactions-error").test_support().child(error.read(cx).clone()))).child(crate::widgets::form_dialog_footer("Save actions")))
                .on_ok(move|_,window,cx|{
                    let list=apply.read(cx).clone();let is_overlay=*overlay_apply.read(cx);
                    let accepted=owner.update(cx,|this,cx|{
                        if this.edit_ticket()!=ticket{this.set_status("The page changed. Open click actions again.",true,cx);return false;}
                        // Include newly selected overlay targets atomically with the binding.
                        let mut probe=emulsion_core::Editor::new(this.editor.doc.clone(),None);
                        for action in &list{if let Action::Overlay{target,..}=action{probe.doc.design.overlays.insert(*target);}}
                        match interactions::author_with_trigger(&mut probe,node,list,can_overlay.then_some(is_overlay),Some(*trigger_apply.read(cx))){
                            Ok(())=>match this.editor.execute(Command::SetDesign{design:Box::new(probe.doc.design)}){Ok(_)=>{this.after_change(cx);true},Err(e)=>{this.set_status(e.to_string(),true,cx);false}},Err(e)=>{this.set_status(e,true,cx);false}
                        }
                    }).unwrap_or(false);
                    if !accepted{let message=owner.read_with(cx,|this,_|this.status.as_ref().map(|(v,_)|v.to_string())).ok().flatten().unwrap_or_else(||"Document is no longer available.".into());error_apply.update(cx,|v,cx|{*v=message;cx.notify();});window.refresh();}accepted
                })
        });
    }
    pub(crate) fn presentation_interaction_generation(&self) -> u64 {
        self.motion
            .session
            .as_ref()
            .map_or(0, |s| s.interactions.generation)
    }
    pub(super) fn presentation_source_document(&self) -> Result<Document, String> {
        self.motion.session.as_ref().map_or_else(
            || Ok(self.editor.doc.clone()),
            |s| s.interactions.source(&self.editor.doc),
        )
    }
    pub(crate) fn apply_presentation_interactions(
        &self,
        preview: Document,
    ) -> Result<Document, String> {
        let Some(session) = self.motion.session.as_ref() else {
            return Ok(preview);
        };
        let preview = if session.interactions.variants.is_empty() {
            preview
        } else {
            let mut cached = session.interaction_source.borrow_mut();
            if cached
                .as_ref()
                .is_none_or(|(generation, _)| *generation != session.interactions.generation)
            {
                *cached = Some((
                    session.interactions.generation,
                    self.presentation_source_document()?,
                ));
            }
            emulsion_core::design_metadata::at_time(
                &cached.as_ref().unwrap().1,
                self.presentation_time_ms(),
            )?
        };
        Ok(session.interactions.apply(preview))
    }
    fn refresh_interaction_preview(&mut self, cx: &mut Context<Self>) {
        let preview = self
            .motion
            .preview
            .clone()
            .unwrap_or_else(|| self.editor.doc.clone());
        match self.apply_presentation_interactions(preview) {
            Ok(doc) => {
                self.motion.preview = Some(doc);
                self.notify_canvas(cx);
                cx.notify();
            }
            Err(error) => self.set_status(error, true, cx),
        }
    }
    pub(crate) fn record_presentation_navigation(&mut self, to: PageId) {
        let from = self.editor.active_page();
        if from == to {
            return;
        }
        if let Some(session) = self.motion.session.as_mut() {
            if !session.navigating_back {
                session.back_stack.push(from);
                if session.back_stack.len() > 256 {
                    session.back_stack.remove(0);
                }
            }
            session.navigating_back = false;
            session.interactions = Runtime::default();
            *session.interaction_source.borrow_mut() = None;
        }
    }
    fn presentation_goto(&mut self, page: PageId, cx: &mut Context<Self>) -> Result<(), String> {
        let pages = self.editor.page_list();
        let current = pages
            .iter()
            .position(|p| p.id == self.editor.active_page())
            .ok_or("Current slide is missing")?;
        let next = pages
            .iter()
            .position(|p| p.id == page)
            .ok_or("The linked slide was removed from this presentation")?;
        self.presentation_step(next as isize - current as isize, cx);
        Ok(())
    }
    pub(super) fn close_presentation_overlay(&mut self, cx: &mut Context<Self>) -> bool {
        let closed = self
            .motion
            .session
            .as_mut()
            .is_some_and(|s| s.interactions.close_overlay());
        if closed {
            self.stop_design_video(cx);
            self.refresh_interaction_preview(cx);
        }
        closed
    }
    pub(crate) fn presentation_interaction_click(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.button != MouseButton::Left {
            return;
        }
        window.focus(&self.canvas_focus, cx);
        let Some(point) = self.doc_point(event.position) else {
            return;
        };
        let Some(session) = self.motion.session.as_ref() else {
            return;
        };
        let doc = self.motion.preview.as_ref().unwrap_or(&self.editor.doc);
        if let Some(id) = interactions::hit_action(doc, &session.interactions, point) {
            match doc
                .design
                .interaction_triggers
                .get(&id)
                .copied()
                .unwrap_or_default()
            {
                Trigger::DragEnd => self.motion.dragged_action = Some((id, event.position)),
                Trigger::Click => {
                    if let Err(error) = self.trigger_presentation_object(id, window, cx) {
                        self.set_status(error, true, cx);
                    }
                }
                Trigger::Hover => (),
            }
        }
    }
    pub(crate) fn presentation_interaction_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.motion.presenting || self.motion.dragged_action.is_some() {
            return;
        }
        let hit = self.doc_point(event.position).and_then(|point| {
            self.motion.session.as_ref().and_then(|s| {
                interactions::hit_action(
                    self.motion.preview.as_ref().unwrap_or(&self.editor.doc),
                    &s.interactions,
                    point,
                )
            })
        });
        if hit == self.motion.hovered_action {
            return;
        }
        self.motion.hovered_action = hit;
        if let Some(id) = hit {
            let doc = self.motion.preview.as_ref().unwrap_or(&self.editor.doc);
            if doc.design.interaction_triggers.get(&id) == Some(&Trigger::Hover)
                && let Err(error) = self.trigger_presentation_object(id, window, cx)
            {
                self.set_status(error, true, cx);
            }
        }
    }
    pub(crate) fn presentation_interaction_release(
        &mut self,
        event: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.motion.presenting || event.button != MouseButton::Left {
            return;
        }
        let Some((node, start)) = self.motion.dragged_action.take() else {
            return;
        };
        let delta = event.position - start;
        if f32::from(delta.x).hypot(f32::from(delta.y)) >= 4.
            && let Err(error) = self.trigger_presentation_object(node, window, cx)
        {
            self.set_status(error, true, cx);
        }
    }
    pub(crate) fn trigger_presentation_object(
        &mut self,
        node: NodeId,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if !self.motion.presenting {
            return Err("Start a presentation before triggering an object.".into());
        }
        let doc = self.motion.preview.as_ref().unwrap_or(&self.editor.doc);
        let actions = doc
            .design
            .interactions
            .get(&node)
            .cloned()
            .ok_or("This object has no presentation actions.")?;
        let mut ancestor = Some(node);
        while let Some(id) = ancestor {
            let item = doc.node(id).ok_or("Interactive object is missing")?;
            if !item.visible || item.opacity <= 0. {
                return Err("This interactive object is hidden.".into());
            }
            ancestor = item.parent;
        }
        let session = self
            .motion
            .session
            .as_ref()
            .ok_or("Presentation session is missing")?;
        if let Some(overlay) = session.interactions.open_overlays.last()
            && node != *overlay
            && !doc.is_ancestor(*overlay, node)
        {
            return Err("Close the active overlay before activating a background object.".into());
        }
        let mut runtime = session.interactions.clone();
        // Preflight the complete authored action list before changing live state.
        for action in &actions {
            match action {
                Action::Slide { page } if self.editor.page(*page).is_none() => {
                    return Err("The linked slide was removed from this presentation.".into());
                }
                Action::Overlay { target, operation } => runtime.overlay(*target, *operation),
                Action::CloseOverlay => {
                    runtime.close_overlay();
                }
                Action::Variant { target, variant } => runtime.variant(*target, variant.clone()),
                _ => (),
            }
        }
        runtime.source(&self.editor.doc)?;
        self.motion.session.as_mut().unwrap().interactions = runtime;
        self.stop_design_video(cx);
        self.refresh_interaction_preview(cx);
        for action in actions {
            match action {
                Action::Url { url } => cx.open_url(&url),
                Action::Next => self.presentation_step(1, cx),
                Action::Previous => self.presentation_step(-1, cx),
                Action::Slide { page } => self.presentation_goto(page, cx)?,
                Action::Back => {
                    let page = self.motion.session.as_mut().and_then(|s| {
                        s.navigating_back = true;
                        s.back_stack.pop()
                    });
                    if let Some(page) = page {
                        self.presentation_goto(page, cx)?;
                    } else if let Some(s) = self.motion.session.as_mut() {
                        s.navigating_back = false;
                    }
                }
                _ => (),
            }
        }
        Ok(())
    }
}
