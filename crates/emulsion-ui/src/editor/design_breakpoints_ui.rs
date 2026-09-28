//! Native breakpoint authoring; drafts are applied together in one transaction.
use super::*;
use emulsion_core::design_layout::{Breakpoint, BreakpointReference, FrameLimits, FrameOverrides};

#[derive(Clone)]
struct Draft {
    fields: [Entity<InputState>; 11],
    overrides: Entity<FrameOverrides>,
}
fn draft(value: Breakpoint, window: &mut Window, cx: &mut App) -> Draft {
    let padding = value
        .overrides
        .padding
        .map(|v| v.map(Some))
        .unwrap_or([None; 4]);
    let values = [
        Some(value.min_width),
        value.overrides.gap,
        padding[0],
        padding[1],
        padding[2],
        padding[3],
        value.overrides.columns.map(f64::from),
        value.overrides.limits.and_then(|l| l.min_width),
        value.overrides.limits.and_then(|l| l.max_width),
        value.overrides.limits.and_then(|l| l.min_height),
        value.overrides.limits.and_then(|l| l.max_height),
    ];
    Draft {
        fields: values.map(|v| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(v.map(|v| v.to_string()).unwrap_or_default())
            })
        }),
        overrides: cx.new(|_| value.overrides),
    }
}
fn parse(draft: &Draft, cx: &App) -> Result<Breakpoint, String> {
    let values = draft
        .fields
        .each_ref()
        .map(|f| optional_dimension(f.read(cx).value().as_ref()))
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    let mut overrides = draft.overrides.read(cx).clone();
    overrides.gap = values[1];
    let padding = &values[2..6];
    overrides.padding = if padding.iter().all(Option::is_none) {
        None
    } else if padding.iter().all(Option::is_some) {
        Some([
            padding[0].unwrap(),
            padding[1].unwrap(),
            padding[2].unwrap(),
            padding[3].unwrap(),
        ])
    } else {
        return Err("Enter all four padding values or leave all four blank to inherit.".into());
    };
    overrides.columns = match values[6] {
        Some(v) if v.fract() == 0. && (1. ..=64.).contains(&v) => Some(v as u32),
        Some(_) => return Err("Grid columns must be an integer from 1 to 64.".into()),
        None => None,
    };
    if overrides.limits.is_some() {
        overrides.limits = Some(FrameLimits {
            min_width: values[7],
            max_width: values[8],
            min_height: values[9],
            max_height: values[10],
        });
    }
    Ok(Breakpoint {
        min_width: values[0].ok_or("Enter a minimum reference width for every breakpoint.")?,
        overrides,
    })
}
fn toggle(value: &mut Option<bool>) {
    *value = match value {
        None => Some(true),
        Some(true) => Some(false),
        Some(false) => None,
    };
}
fn boolean_label(label: &str, value: Option<bool>) -> String {
    format!(
        "{label}: {}",
        match value {
            None => "inherit",
            Some(true) => "on",
            Some(false) => "off",
        }
    )
}

impl EditorView {
    pub(crate) fn design_breakpoints_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let Some(group) = self.selected else {
            return;
        };
        if !self.layout_targets_editable(&[group], cx) {
            return;
        }
        let Some(frame) = self.editor.doc.design.frames.get(&group) else {
            return;
        };
        let reference = cx.new(|_| frame.breakpoint_reference);
        let drafts = frame
            .breakpoints
            .iter()
            .cloned()
            .map(|b| draft(b, window, cx))
            .collect::<Vec<_>>();
        let drafts = cx.new(|_| drafts);
        let error = cx.new(|_| String::new());
        let page_width = layout::reference_width(&self.editor.doc, group)
            .unwrap_or(f64::from(self.editor.doc.width));
        let active = layout::active_breakpoint(&self.editor.doc, group);
        let ticket = self.edit_ticket();
        let owner = cx.weak_entity();
        window.open_dialog(cx,move|dialog,window,cx|{
            let add = drafts.clone();
            let apply = drafts.clone();
            let owner = owner.clone();
            let error_apply = error.clone();
            let change_reference=reference.clone();let apply_reference=reference.clone();
            dialog.title("Responsive breakpoints").width(px(510.))
                .child(div().id("design-breakpoints-body").test_support().max_h(px((f32::from(window.viewport_size().height)-220.).clamp(100.,680.))).overflow_y_scroll().flex().flex_col().gap_3()
                    .child(Button::new("design-breakpoint-reference").label(if *reference.read(cx)==BreakpointReference::Canvas {"Width reference: canvas"}else{"Width reference: parent container"}).small().outline().on_click(move|_,window,cx|{change_reference.update(cx,|v,cx|{*v=if *v==BreakpointReference::Canvas {BreakpointReference::Container}else{BreakpointReference::Canvas};cx.notify();});window.refresh();}))
                    .child(div().text_size(px(12.)).child(format!("Current reference: {page_width}px · Active: {}",active.map(|v|format!("{v}px and wider")).unwrap_or_else(||"base settings".into()))))
                    .child(div().text_size(px(11.)).child("The highest matching width inherits directly from base settings. Container rules use the responsive parent’s inner width, or canvas at the top level. Content-sized query ancestors are not allowed."))
                    .child(Button::new("design-breakpoint-add").label("Add breakpoint").small().outline().disabled(drafts.read(cx).len()>=16)
                        .on_click(move|_,window,cx|{
                            let mut width=page_width.clamp(1.,100000.);
                            let used=add.read(cx).iter().filter_map(|d|d.fields[0].read(cx).value().parse::<f64>().ok()).collect::<Vec<_>>();
                            while used.contains(&width) {width=if width>=100000. {1.} else {width+1.};}
                            let value=draft(Breakpoint{min_width:width,overrides:FrameOverrides::default()},window,cx);
                            add.update(cx,|values,cx|{values.push(value);cx.notify();});window.refresh();
                        }))
                    .children(drafts.read(cx).iter().cloned().enumerate().map(|(index,value)|{
                        let remove=drafts.clone();let state=value.overrides.clone();let settings=state.read(cx).clone();
                        let flow=state.clone();let align=state.clone();let limits=state.clone();
                        div().id(("design-breakpoint-row",index)).test_support().flex().flex_col().gap_2()
                            .child(div().flex().justify_between().child(format!("Breakpoint {}",index+1)).child(Button::new(("design-breakpoint-remove",index)).label("Remove").small().ghost().on_click(move|_,window,cx|{remove.update(cx,|values,cx|{values.remove(index);cx.notify();});window.refresh();})))
                            .child(div().grid().grid_cols(2).gap_2().children(["Minimum reference width · px","Gap · px","Padding top","Padding right","Padding bottom","Padding left","Grid columns"].into_iter().enumerate().map(|(field,label)|div().child(label).child(Input::new(&value.fields[field]).id(("design-breakpoint-input",index*7+field))))))
                            .child(Button::new(("design-breakpoint-limits",index)).label(if settings.limits.is_some(){"Size limits: override"}else{"Size limits: inherit"}).small().outline().on_click(move|_,window,cx|{limits.update(cx,|v,cx|{v.limits=if v.limits.is_some(){None}else{Some(FrameLimits::default())};cx.notify();});window.refresh();}))
                            .when(settings.limits.is_some(),|d|d.child(div().grid().grid_cols(2).gap_2().children(["Minimum width","Maximum width","Minimum height","Maximum height"].into_iter().enumerate().map(|(field,label)|div().child(label).child(Input::new(&value.fields[7+field]).id(("design-breakpoint-limit",index*4+field)))))))
                            .child(Button::new(("design-breakpoint-flow",index)).label(format!("Flow: {}",match settings.flow{None=>"inherit",Some(Flow::Row)=>"row",Some(Flow::Column)=>"column",Some(Flow::Grid)=>"grid"})).small().outline()
                                .on_click(move|_,window,cx|{flow.update(cx,|v,cx|{v.flow=match v.flow{None=>Some(Flow::Row),Some(Flow::Row)=>Some(Flow::Column),Some(Flow::Column)=>Some(Flow::Grid),Some(Flow::Grid)=>None};cx.notify();});window.refresh();}))
                            .child(Button::new(("design-breakpoint-align",index)).label(format!("Alignment: {}",match settings.align{None=>"inherit",Some(Align::Start)=>"start",Some(Align::Center)=>"center",Some(Align::End)=>"end"})).small().outline()
                                .on_click(move|_,window,cx|{align.update(cx,|v,cx|{v.align=match v.align{None=>Some(Align::Start),Some(Align::Start)=>Some(Align::Center),Some(Align::Center)=>Some(Align::End),Some(Align::End)=>None};cx.notify();});window.refresh();}))
                            .children([("Wrap rows",settings.wrap),("Fit width to content",settings.hug_width),("Fit height to content",settings.hug_height),("Clip content",settings.clip_content)].into_iter().enumerate().map(move|(field,(label,enabled))|{
                                let state=state.clone();Button::new(("design-breakpoint-toggle",index*4+field)).label(boolean_label(label,enabled)).small().outline().on_click(move|_,window,cx|{state.update(cx,|v,cx|{toggle(match field{0=>&mut v.wrap,1=>&mut v.hug_width,2=>&mut v.hug_height,_=>&mut v.clip_content});cx.notify();});window.refresh();})
                            }))
                    })))
                .footer(div().id("design-breakpoints-footer").test_support().flex().flex_col().gap_2()
                    .when(!error.read(cx).is_empty(),|d|d.child(div().id("design-breakpoints-error").test_support().child(error.read(cx).clone())))
                    .child(crate::widgets::form_dialog_footer("Apply breakpoints")))
                .on_ok(move|_,window,cx|{
                    let values=apply.read(cx).iter().map(|d|parse(d,cx)).collect::<Result<Vec<_>,_>>();
                    let accepted=owner.update(cx,|this,cx|{
                        if this.edit_ticket()!=ticket {this.set_status("The page changed. Open breakpoints again.",true,cx);return false;}
                        if !this.layout_targets_editable(&[group],cx) {return false;}
                        let values=match values{Ok(v)=>v,Err(e)=>{this.set_status(e,true,cx);return false;}};
                        let mut design=this.editor.doc.design.clone();
                        let Some(frame)=design.frames.get_mut(&group) else{return false;};
                        frame.breakpoints=values;frame.breakpoint_reference= *apply_reference.read(cx);
                        match this.editor.execute(Command::SetDesign{design:Box::new(design)}){
                            Ok(_)=>{this.after_change(cx);true},Err(e)=>{this.set_status(e.to_string(),true,cx);false}
                        }
                    }).unwrap_or(false);
                    if !accepted {
                        let message=owner.read_with(cx,|this,_|this.status.as_ref().map(|(text,_)|text.to_string())).ok().flatten().unwrap_or_else(||"The document is no longer available.".into());
                        error_apply.update(cx,|value,cx|{*value=message;cx.notify();});window.refresh();
                    }
                    accepted
                })
        });
    }
}
