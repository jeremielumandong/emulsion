//! Responsive layout controls use persisted frame settings and normal Undo.
use super::*;
use emulsion_core::design_layout::{self as layout, Align, Flow, Frame};
use gpui_kit::component::{
    Disableable, Sizable, WindowExt,
    button::{Button, ButtonVariants},
};

impl EditorView {
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
        div().id("design-layout-controls").test_support().flex().flex_col().gap(px(6.))
            .child(div().text_color(p.muted).child("Responsive layout"))
            .child(div().grid().grid_cols(3).gap(px(4.)).children([
                (Flow::Row,"Row"),(Flow::Column,"Column"),(Flow::Grid,"Grid")
            ].into_iter().enumerate().map(|(i,(flow,label))|{
                Button::new(("design-layout-flow",i)).label(label).small().outline()
                    .disabled(!has_selection)
                    .when(frame.is_some_and(|f| f.flow==flow),|b|b.bg(p.soft_bg).text_color(p.accent))
                    .on_click(cx.listener(move |this,_,window,cx|this.design_layout_dialog(flow,window,cx)))
            })))
            .when(frame.is_some(),|d|d.child(Button::new("design-layout-remove").label("Remove automatic layout")
                .small().ghost().on_click(cx.listener(|this,_,_,cx|{
                    if !this.prepare_page_action(cx) {return;}
                    let mut design=this.editor.doc.design.clone();
                    if let Some(id)=this.selected {design.frames.remove(&id);}
                    this.execute(Command::SetDesign{design:Box::new(design)},cx);
                }))))
            .when_some(child,|d,(parent,id,settings)|d.children([
                ("design-layout-child-position", if settings.absolute { "Position: absolute" } else { "Position: in layout" }, true),
                ("design-layout-child-width", if settings.fill_width { "Width: fill frame" } else { "Width: fixed" }, false),
            ].map(|(key,label,position)|Button::new(key).label(label).small().outline()
                .on_click(cx.listener(move|this,_,_,cx|{
                    if !this.prepare_page_action(cx) {return;}
                    let mut design=this.editor.doc.design.clone();
                    if let Some(frame)=design.frames.get_mut(&parent) {
                        let child=frame.children.entry(id).or_default();
                        if position {child.absolute= !child.absolute;} else {child.fill_width= !child.fill_width;}
                        this.execute(Command::SetDesign{design:Box::new(design)},cx);
                    }
                })))))
            .child(div().text_size(px(10.)).text_color(p.muted)
                .child("Select objects or a group. Layout follows frame width and preserves editable text."))
            .into_any_element()
    }

    fn design_layout_dialog(&mut self, flow: Flow, window: &mut Window, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let ids = self.selected_layer_roots();
        if ids.is_empty() {
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
            bounds.map_or(self.editor.doc.width as f64 * 0.8, |b| b.2),
            bounds.map_or(self.editor.doc.height as f64 * 0.8, |b| b.3),
            settings.gap,
            settings.padding[0],
            settings.padding[1],
            settings.padding[2],
            settings.padding[3],
            settings.columns as f64,
        ]
        .map(|v| cx.new(|cx| InputState::new(window, cx).default_value(format!("{v:.0}"))));
        let fill = cx.new(|_| {
            !settings.children.is_empty()
                && settings.children.values().all(|child| child.fill_width)
        });
        let wrap = cx.new(|_| settings.wrap);
        let alignment = cx.new(|_| settings.align);
        let owner = cx.weak_entity();
        let ticket = self.edit_ticket();
        window.open_dialog(cx,move|dialog,_,cx|{
            let fields=fields.clone();let inputs=fields.clone();let owner=owner.clone();let ids=ids.clone();let settings=settings.clone();
            let fill_state=fill.clone();let wrap_state=wrap.clone();let align_state=alignment.clone();
            let fill_apply=fill.clone();let wrap_apply=wrap.clone();let align_apply=alignment.clone();
            dialog.title("Responsive layout").width(px(440.))
                .child(div().flex().flex_col().gap_2()
                    .child(div().grid().grid_cols(2).gap_2().children([
                        "Frame width · px","Frame height · px","Gap · px","Padding top","Padding right","Padding bottom","Padding left","Grid columns"
                    ].into_iter().enumerate().map(|(i,label)|div().child(label).child(Input::new(&fields[i]).id(("design-layout-input",i))))))
                    .child(Button::new("design-layout-fill").label(if *fill.read(cx) { "✓ Children fill available width" } else { "Children keep their own width" }).small().outline()
                        .on_click(move|_,window,cx|{fill_state.update(cx,|v,cx|{*v= !*v;cx.notify();});window.refresh();}))
                    .child(Button::new("design-layout-wrap").label(if *wrap.read(cx) { "✓ Wrap rows" } else { "Keep row on one line" }).small().outline()
                        .on_click(move|_,window,cx|{wrap_state.update(cx,|v,cx|{*v= !*v;cx.notify();});window.refresh();}))
                    .child(Button::new("design-layout-align").label(format!("Alignment: {}",match *alignment.read(cx) {Align::Start=>"Start",Align::Center=>"Center",Align::End=>"End"})).small().outline()
                        .on_click(move|_,window,cx|{align_state.update(cx,|v,cx|{*v=match *v{Align::Start=>Align::Center,Align::Center=>Align::End,Align::End=>Align::Start};cx.notify();});window.refresh();}))
                    .child(div().text_size(px(11.)).child("Children follow layer order. Text wraps without resizing its font. Changes are one Undo step.")))
                .footer(crate::widgets::form_dialog_footer("Apply layout"))
                .on_ok(move|_,_,cx|{
                    let values=inputs.each_ref().map(|i|i.read(cx).value().parse::<f64>().unwrap_or(f64::NAN));
                    let fill=*fill_apply.read(cx);let wrap=*wrap_apply.read(cx);let align=*align_apply.read(cx);
                    owner.update(cx,|this,cx|{
                        if this.edit_ticket()!=ticket {this.set_status("The page changed. Open layout again.",true,cx);return false;}
                        if values.iter().any(|v|!v.is_finite()) || values[7].fract()!=0. || !(1. ..=64.).contains(&values[7]) {
                            this.set_status("Enter valid dimensions and 1–64 columns.",true,cx);return false;
                        }
                        this.editor.begin("Responsive layout");
                        let result=(||{
                            let group=match group {Some(id)=>id,None=>this.editor.execute(Command::Group{ids:ids.clone(),name:"Responsive frame".into()}).map_err(|e|e.to_string())?.ok_or("No group created")?};
                            let mut frame=Frame{flow,gap:values[2],padding:[values[3],values[4],values[5],values[6]],columns:values[7] as u32,wrap,align,..settings.clone()};
                            for id in this.editor.doc.children(Some(group)) {if id!=frame.boundary {frame.children.entry(id).or_default().fill_width=fill;}}
                            layout::enable(&mut this.editor,group,frame,(values[0],values[1]))?;
                            Ok::<_,String>(group)
                        })();
                        match result {
                            Ok(id)=>{this.editor.end();this.set_layer_selection(vec![id],Some(id));this.after_change(cx);true}
                            Err(error)=>{this.editor.cancel();this.set_status(error,true,cx);false}
                        }
                    }).unwrap_or(false)
                })
        });
    }
}
