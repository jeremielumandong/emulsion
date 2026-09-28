//! Responsive sizing stays native, validates atomically and uses normal Undo.
use super::*;
use emulsion_core::design_layout::{self as layout, Align, Flow, Frame};
use gpui_kit::component::{
    Disableable, Sizable, WindowExt,
    button::{Button, ButtonVariants},
};

#[path = "design_breakpoints_ui.rs"]
mod breakpoints;

fn optional_dimension(value: &str) -> Result<Option<f64>, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    let number: f64 = value
        .parse()
        .map_err(|_| "Enter a number or leave the optional limit blank")?;
    if !number.is_finite() {
        return Err("Dimensions must be finite numbers".into());
    }
    Ok(Some(number))
}

impl EditorView {
    fn layout_targets_editable(&mut self, ids: &[NodeId], cx: &mut Context<Self>) -> bool {
        if ids.iter().any(|id| {
            let locks = self.editor.doc.layer_locks(*id);
            self.editor.doc.node(*id).is_none()
                || self.editor.doc.locked_ancestor(*id).is_some()
                || locks.position
                || locks.pixels
                || locks.transparency
        }) {
            self.set_status(
                "Unlock the selected objects and their parent frame before changing layout.",
                true,
                cx,
            );
            false
        } else {
            true
        }
    }
    pub(super) fn design_layout_controls(&self, p: &Palette, cx: &Context<Self>) -> AnyElement {
        let frame = self
            .selected
            .and_then(|id| self.editor.doc.design.frames.get(&id));
        let has_selection = !self.selected_layer_roots().is_empty();
        let child = self.selected.and_then(|id| {
            let parent = self.editor.doc.node(id)?.parent?;
            let frame = self.editor.doc.design.frames.get(&parent)?;
            (frame.boundary != id).then_some((
                parent,
                id,
                frame.children.get(&id).copied().unwrap_or_default(),
            ))
        });
        div()
            .id("design-layout-controls")
            .test_support()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(div().text_color(p.muted).child("Responsive layout"))
            .child(
                div().grid().grid_cols(3).gap(px(4.)).children(
                    [
                        (Flow::Row, "Row"),
                        (Flow::Column, "Column"),
                        (Flow::Grid, "Grid"),
                    ]
                    .into_iter()
                    .enumerate()
                    .map(|(i, (flow, label))| {
                        Button::new(("design-layout-flow", i))
                            .label(label)
                            .small()
                            .outline()
                            .disabled(!has_selection)
                            .when(frame.is_some_and(|f| f.flow == flow), |b| {
                                b.bg(p.soft_bg).text_color(p.accent)
                            })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.design_layout_dialog(flow, window, cx)
                            }))
                    }),
                ),
            )
            .when(frame.is_some(), |d| {
                d.child(
                    Button::new("design-layout-breakpoints")
                        .label("Canvas width breakpoints…")
                        .small()
                        .outline()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.design_breakpoints_dialog(window, cx)
                        })),
                )
                .child(
                    Button::new("design-layout-remove")
                        .label("Remove automatic layout")
                        .small()
                        .ghost()
                        .on_click(cx.listener(|this, _, _, cx| {
                            if !this.prepare_page_action(cx) {
                                return;
                            }
                            let Some(id) = this.selected else {
                                return;
                            };
                            if !this.layout_targets_editable(&[id], cx) {
                                return;
                            }
                            let mut design = this.editor.doc.design.clone();
                            design.frames.remove(&id);
                            this.execute(
                                Command::SetDesign {
                                    design: Box::new(design),
                                },
                                cx,
                            );
                        })),
                )
            })
            .when_some(child, |d, (parent, id, settings)| {
                d.child(div().text_size(px(11.)).text_color(p.muted).child(format!(
                    "{} · {} width · {} height",
                    if settings.absolute {
                        "Absolute"
                    } else {
                        "In layout"
                    },
                    if settings.fill_width { "Fill" } else { "Fixed" },
                    if settings.fill_height {
                        "Fill"
                    } else {
                        "Fixed"
                    }
                )))
                .child(
                    Button::new("design-layout-child-sizing")
                        .label("Object sizing & limits")
                        .small()
                        .outline()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.design_layout_child_dialog(parent, id, window, cx)
                        })),
                )
            })
            .child(
                div().text_size(px(10.)).text_color(p.muted).child(
                    "Set frame flow, content sizing and object limits. Text stays editable.",
                ),
            )
            .into_any_element()
    }

    pub(crate) fn design_layout_dialog(
        &mut self,
        flow: Flow,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let ids = self.selected_layer_roots();
        if ids.is_empty() || !self.layout_targets_editable(&ids, cx) {
            return;
        }
        let group = (ids.len() == 1 && self.editor.doc.node(ids[0]).is_some_and(|n| n.is_group()))
            .then_some(ids[0]);
        let settings = group
            .and_then(|id| self.editor.doc.design.frames.get(&id))
            .cloned()
            .unwrap_or_default();
        let bounds = group.and_then(|id| layout::bounds(&self.editor.doc, id));
        let fields = [
            Some(bounds.map_or(self.editor.doc.width as f64 * 0.8, |b| b.2)),
            Some(bounds.map_or(self.editor.doc.height as f64 * 0.8, |b| b.3)),
            Some(settings.gap),
            Some(settings.padding[0]),
            Some(settings.padding[1]),
            Some(settings.padding[2]),
            Some(settings.padding[3]),
            Some(settings.columns as f64),
            settings.min_width,
            settings.max_width,
            settings.min_height,
            settings.max_height,
        ]
        .map(|v| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(v.map(|v| v.to_string()).unwrap_or_default())
            })
        });
        let original_fill =
            !settings.children.is_empty() && settings.children.values().all(|c| c.fill_width);
        let fill = cx.new(|_| original_fill);
        let wrap = cx.new(|_| settings.wrap);
        let clip = cx.new(|_| settings.clip_content);
        let hug_width = cx.new(|_| settings.hug_width);
        let hug_height = cx.new(|_| settings.hug_height);
        let alignment = cx.new(|_| settings.align);
        let owner = cx.weak_entity();
        let ticket = self.edit_ticket();
        let error = cx.new(|_| String::new());
        window.open_dialog(cx,move|dialog,window,cx|{
            let fields=fields.clone();let inputs=fields.clone();let owner=owner.clone();let ids=ids.clone();let settings=settings.clone();
            let fill_state=fill.clone();let wrap_state=wrap.clone();let align_state=alignment.clone();
            let fill_apply=fill.clone();let wrap_apply=wrap.clone();let align_apply=alignment.clone();
            let width_state=hug_width.clone();let width_apply=hug_width.clone();let height_state=hug_height.clone();let height_apply=hug_height.clone();
            let error_apply=error.clone();
            let clip_state=clip.clone();let clip_apply=clip.clone();
            dialog.title("Responsive layout").width(px(480.))
                .child(div().id("design-layout-dialog-body").test_support().max_h(px((f32::from(window.viewport_size().height)-220.).clamp(100.,680.))).overflow_y_scroll().flex().flex_col().gap_2()
                    .child(div().grid().grid_cols(2).gap_2().children([
                        "Frame width · px","Frame height · px","Gap · px","Padding top","Padding right","Padding bottom","Padding left","Grid columns","Minimum width","Maximum width","Minimum height","Maximum height"
                    ].into_iter().enumerate().map(|(i,label)|div().child(label).child(Input::new(&fields[i]).id(("design-layout-input",i))))))
                    .child(div().text_size(px(11.)).child("Leave a limit blank to remove it. Dimensions may include decimals."))
                    .child(Button::new("design-layout-fill").label(if *fill.read(cx) {"✓ Children fill available width"} else {"Keep individual child widths"}).small().outline()
                        .on_click(move|_,window,cx|{fill_state.update(cx,|v,cx|{*v= !*v;cx.notify();});window.refresh();}))
                    .child(Button::new("design-layout-clip").label(if *clip.read(cx) {"✓ Clip content to frame"} else {"Allow content outside frame"}).small().outline()
                        .on_click(move|_,window,cx|{clip_state.update(cx,|v,cx|{*v= !*v;cx.notify();});window.refresh();}))
                    .child(Button::new("design-layout-wrap").label(if *wrap.read(cx) {"✓ Wrap rows"} else {"Keep row on one line"}).small().outline()
                        .on_click(move|_,window,cx|{wrap_state.update(cx,|v,cx|{*v= !*v;cx.notify();});window.refresh();}))
                    .child(Button::new("design-layout-hug-width").label(if *hug_width.read(cx) {"Width: fit content"} else {"Width: fixed"}).small().outline()
                        .on_click(move|_,window,cx|{width_state.update(cx,|v,cx|{*v= !*v;cx.notify();});window.refresh();}))
                    .child(Button::new("design-layout-hug-height").label(if *hug_height.read(cx) {"Height: fit content"} else {"Height: fixed"}).small().outline()
                        .on_click(move|_,window,cx|{height_state.update(cx,|v,cx|{*v= !*v;cx.notify();});window.refresh();}))
                    .child(Button::new("design-layout-align").label(format!("Alignment: {}",match *alignment.read(cx){Align::Start=>"Start",Align::Center=>"Center",Align::End=>"End"})).small().outline()
                        .on_click(move|_,window,cx|{align_state.update(cx,|v,cx|{*v=match *v{Align::Start=>Align::Center,Align::Center=>Align::End,Align::End=>Align::Start};cx.notify();});window.refresh();}))
                    .child(div().text_size(px(11.)).child("Fill and fit-content cannot share the same axis. Text reflows without changing font size.")))
                .footer(div().id("design-layout-footer").test_support().flex().flex_col().gap_2().when(!error.read(cx).is_empty(),|d|d.child(div().id("design-layout-error").test_support().text_size(px(12.)).child(error.read(cx).clone()))).child(crate::widgets::form_dialog_footer("Apply layout")))
                .on_ok(move|_,window,cx|{
                    let parsed=inputs.each_ref().map(|i|optional_dimension(i.read(cx).value().as_ref()));
                    let fill=*fill_apply.read(cx);let wrap=*wrap_apply.read(cx);let align=*align_apply.read(cx);
                    let clip_content=*clip_apply.read(cx);
                    let hug_width=*width_apply.read(cx);let hug_height=*height_apply.read(cx);
                    let accepted=owner.update(cx,|this,cx|{
                        if this.edit_ticket()!=ticket {this.set_status("The page changed. Open layout again.",true,cx);return false;}
                        if !this.layout_targets_editable(&ids,cx) {return false;}
                        let values = match parsed.into_iter().collect::<Result<Vec<_>,_>>() {Ok(values)=>values,Err(error)=>{this.set_status(error,true,cx);return false;}};
                        if values[..8].iter().any(Option::is_none) || values[7].unwrap().fract()!=0. || !(1. ..=64.).contains(&values[7].unwrap()) {
                            this.set_status("Enter valid dimensions and 1–64 columns.",true,cx);return false;
                        }
                        this.editor.begin("Responsive layout");
                        let result=(||{
                            let group=match group {Some(id)=>id,None=>this.editor.execute(Command::Group{ids:ids.clone(),name:"Responsive frame".into()}).map_err(|e|e.to_string())?.ok_or("No group created")?};
                            let mut frame=Frame{flow,clip_content,gap:values[2].unwrap(),padding:[values[3].unwrap(),values[4].unwrap(),values[5].unwrap(),values[6].unwrap()],columns:values[7].unwrap() as u32,wrap,align,hug_width,hug_height,min_width:values[8],max_width:values[9],min_height:values[10],max_height:values[11],..settings.clone()};
                            if fill!=original_fill {for id in this.editor.doc.children(Some(group)) {if id!=frame.boundary {frame.children.entry(id).or_default().fill_width=fill;}}}
                            layout::enable(&mut this.editor,group,frame,(values[0].unwrap(),values[1].unwrap()))?;
                            Ok::<_,String>(group)
                        })();
                        match result {
                            Ok(id)=>{this.editor.end();this.set_layer_selection(vec![id],Some(id));this.after_change(cx);true}
                            Err(error)=>{this.editor.cancel();this.set_status(error,true,cx);false}
                        }
                    }).unwrap_or(false);
                    if !accepted {
                        let message=owner.read_with(cx,|this,_|this.status.as_ref().map(|(text,_)|text.to_string())).ok().flatten().unwrap_or_else(||"The document is no longer available.".into());
                        error_apply.update(cx,|error,cx|{*error=message;cx.notify();});
                        window.refresh();
                    }
                    accepted
                })
        });
    }

    pub(crate) fn design_layout_child_dialog(
        &mut self,
        parent: NodeId,
        id: NodeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) || !self.layout_targets_editable(&[parent, id], cx) {
            return;
        }
        let Some(frame) = self.editor.doc.design.frames.get(&parent) else {
            return;
        };
        if frame.boundary == id || self.editor.doc.node(id).and_then(|n| n.parent) != Some(parent) {
            return;
        }
        let settings = frame.children.get(&id).copied().unwrap_or_default();
        let fields = [
            settings.min_width,
            settings.max_width,
            settings.min_height,
            settings.max_height,
            settings.aspect_ratio,
        ]
        .map(|v| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(v.map(|v| v.to_string()).unwrap_or_default())
            })
        });
        let state = cx.new(|_| settings);
        let ratio = layout::item_dimensions(&self.editor.doc, id)
            .and_then(|(w, h)| (h > 0.).then_some(w / h));
        let owner = cx.weak_entity();
        let ticket = self.edit_ticket();
        let error = cx.new(|_| String::new());
        window.open_dialog(cx,move|dialog,window,cx|{
            let inputs=fields.clone();let owner=owner.clone();let apply=state.clone();
            let settings=*state.read(cx);
            let aspect=fields[4].clone();
            let buttons=[("design-layout-child-position",if settings.absolute {"Position: absolute"} else {"Position: in layout"}),
                ("design-layout-child-width",if settings.fill_width {"Width: fill frame"} else {"Width: fixed"}),
                ("design-layout-child-height",if settings.fill_height {"Height: fill frame"} else {"Height: fixed"})];
            let error_apply=error.clone();
            dialog.title("Object sizing & limits").width(px(440.))
                .child(div().id("design-layout-child-dialog-body").test_support().max_h(px((f32::from(window.viewport_size().height)-220.).clamp(100.,680.))).overflow_y_scroll().flex().flex_col().gap_2()
                    .children(buttons.into_iter().enumerate().map(|(i,(key,label))|{
                        let state=state.clone();Button::new(key).label(label).small().outline().on_click(move|_,window,cx|{
                            state.update(cx,|value,cx|{match i{0=>value.absolute= !value.absolute,1=>value.fill_width= !value.fill_width,_=>value.fill_height= !value.fill_height};cx.notify();});window.refresh();
                        })
                    }))
                    .child(div().grid().grid_cols(2).gap_2().children(["Minimum width","Maximum width","Minimum height","Maximum height","Aspect ratio · width / height"].into_iter().enumerate().map(|(i,label)|div().child(label).child(Input::new(&fields[i]).id(("design-layout-child-input",i))))))
                    .child(Button::new("design-layout-child-aspect").label(if fields[4].read(cx).value().trim().is_empty(){"Keep aspect ratio"}else{"Unlock aspect ratio"}).small().outline().disabled(ratio.is_none())
                        .on_click(move|_,window,cx|{let value=if aspect.read(cx).value().trim().is_empty(){ratio.map(|v|v.to_string()).unwrap_or_default()}else{String::new()};aspect.update(cx,|state,cx|state.set_value(value,window,cx));window.refresh();}))
                    .child(div().text_size(px(11.)).child("Blank limits are unrestricted. Set a ratio to lock proportions; clear it to unlock. Text uses paragraph dimensions without distorting glyphs.")))
                .footer(div().id("design-layout-child-footer").test_support().flex().flex_col().gap_2().when(!error.read(cx).is_empty(),|d|d.child(div().id("design-layout-child-error").test_support().text_size(px(12.)).child(error.read(cx).clone()))).child(crate::widgets::form_dialog_footer("Apply sizing")))
                .on_ok(move|_,window,cx|{
                    let parsed=inputs.each_ref().map(|i|optional_dimension(i.read(cx).value().as_ref()));
                    let mut settings=*apply.read(cx);
                    let accepted=owner.update(cx,|this,cx|{
                        if this.edit_ticket()!=ticket {this.set_status("The page changed. Open object sizing again.",true,cx);return false;}
                        if !this.layout_targets_editable(&[parent,id],cx) {return false;}
                        let values=match parsed.into_iter().collect::<Result<Vec<_>,_>>() {Ok(v)=>v,Err(error)=>{this.set_status(error,true,cx);return false;}};
                        settings.min_width=values[0];settings.max_width=values[1];settings.min_height=values[2];settings.max_height=values[3];settings.aspect_ratio=values[4];
                        let mut design=this.editor.doc.design.clone();
                        let Some(frame)=design.frames.get_mut(&parent) else{return false;};
                        if this.editor.doc.node(id).and_then(|n|n.parent)!=Some(parent) {return false;}
                        frame.children.insert(id,settings);
                        match this.editor.execute(Command::SetDesign{design:Box::new(design)}) {
                            Ok(_)=>{this.after_change(cx);true},Err(error)=>{this.set_status(error.to_string(),true,cx);false}
                        }
                    }).unwrap_or(false);
                    if !accepted {
                        let message=owner.read_with(cx,|this,_|this.status.as_ref().map(|(text,_)|text.to_string())).ok().flatten().unwrap_or_else(||"The document is no longer available.".into());
                        error_apply.update(cx,|error,cx|{*error=message;cx.notify();});
                        window.refresh();
                    }
                    accepted
                })
        });
    }
}
