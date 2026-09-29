use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use emulsion_core::{
    Node, NodeKind,
    design_keyframes::{Easing, Property, Track},
};
use emulsion_raster::vector::{Path as VectorPath, PathPaint, PathStyle, StrokeCap, StrokeJoin};
use serde_json::json;
use std::collections::BTreeSet;
fn prop(v: impl Serialize) -> Value {
    json!({"a":0,"k":v})
}
fn color(c: [u8; 4]) -> Value {
    json!([
        c[0] as f64 / 255.,
        c[1] as f64 / 255.,
        c[2] as f64 / 255.,
        1.
    ])
}
fn path_value(path: &emulsion_raster::vector::SubPath) -> Value {
    json!({"c":path.closed,"v":path.anchors.iter().map(|a|[a.p.0,a.p.1]).collect::<Vec<_>>(),"i":path.anchors.iter().map(|a|[a.h_in.0-a.p.0,a.h_in.1-a.p.1]).collect::<Vec<_>>(),"o":path.anchors.iter().map(|a|[a.h_out.0-a.p.0,a.h_out.1-a.p.1]).collect::<Vec<_>>()})
}
fn paint(style: PathStyle, path: &VectorPath, stroke: bool) -> Result<Option<Value>> {
    let Some(c) = (if stroke { style.stroke } else { style.fill }) else {
        return Ok(None);
    };
    let p = if stroke {
        style.stroke_paint
    } else {
        style.fill_paint
    };
    let gradient = p.gradient_stops(c);
    let mut value = if let Some(stops) = gradient {
        let (x, y, w, h) =
            emulsion_raster::vector_geometry::bounds(path).unwrap_or((0., 0., 1., 1.));
        let (radial, angle) = match p {
            PathPaint::RadialGradient { .. } | PathPaint::RadialStops { .. } => (true, 0.),
            PathPaint::LinearGradient { angle, .. } | PathPaint::LinearStops { angle, .. } => {
                (false, f64::from(angle))
            }
            _ => unreachable!(),
        };
        let mut values = Vec::new();
        for stop in &stops {
            values.extend([
                stop.offset as f64,
                stop.color[0] as f64 / 255.,
                stop.color[1] as f64 / 255.,
                stop.color[2] as f64 / 255.,
            ]);
        }
        for stop in &stops {
            values.extend([stop.offset as f64, stop.color[3] as f64 / 255.]);
        }
        let c = (x + w / 2., y + h / 2.);
        let a = angle.to_radians();
        let extent = (a.cos().abs() * w + a.sin().abs() * h) / 2.;
        let start = if radial {
            [c.0, c.1]
        } else {
            [c.0 - a.cos() * extent, c.1 - a.sin() * extent]
        };
        let end = if radial {
            [c.0 + w.max(h) / 2., c.1]
        } else {
            [c.0 + a.cos() * extent, c.1 + a.sin() * extent]
        };
        json!({"ty":if stroke{"gs"}else{"gf"},"t":if radial{2}else{1},"g":{"p":stops.len(),"k":prop(values)},"s":prop(start),"e":prop(end),"o":prop(100.),"r":if style.even_odd{2}else{1},"h":prop(0.),"a":prop(0.)})
    } else {
        if !matches!(p, PathPaint::Solid) {
            return Err(error(
                "Pattern paint requires explicit rendered-frame export",
            ));
        }
        json!({"ty":if stroke{"st"}else{"fl"},"c":prop(color(c)),"o":prop(c[3]as f64/255.*100.),"r":if style.even_odd{2}else{1}})
    };
    if stroke {
        if style.alignment != Default::default() {
            return Err(error(
                "Inside/outside stroke alignment requires rendered-frame export",
            ));
        }
        value["w"] = prop(style.width);
        value["lc"] = json!(match style.cap {
            StrokeCap::Butt => 1,
            StrokeCap::Round => 2,
            StrokeCap::Square => 3,
        });
        value["lj"] = json!(match style.join {
            StrokeJoin::Miter => 1,
            StrokeJoin::Round => 2,
            StrokeJoin::Bevel => 3,
        });
        value["ml"] = json!(style.miter_limit);
        if style.dash_count > 0 {
            let mut d = Vec::new();
            for (i, v) in style.dash[..usize::from(style.dash_count)]
                .iter()
                .enumerate()
            {
                d.push(json!({"n":if i%2==0{"d"}else{"g"},"v":prop(v)}));
            }
            d.push(json!({"n":"o","v":prop(style.dash_offset)}));
            value["d"] = json!(d);
        }
    }
    Ok(Some(value))
}
fn shapes(path: &VectorPath, style: PathStyle) -> Result<Value> {
    let mut shapes: Vec<_> = path
        .subpaths
        .iter()
        .map(|p| json!({"ty":"sh","ks":prop(path_value(p))}))
        .collect();
    if let Some(fill) = paint(style, path, false)? {
        shapes.push(fill);
    }
    if let Some(stroke) = paint(style, path, true)? {
        shapes.push(stroke);
    }
    Ok(json!(shapes))
}
fn easing(e: Easing) -> (f64, f64) {
    match e {
        Easing::Linear | Easing::Step => (1. / 3., 2. / 3.),
        Easing::EaseIn => (0., 1. / 3.),
        Easing::EaseOut => (2. / 3., 1.),
        Easing::EaseInOut => (0., 1.),
    }
}
fn scalar_track(track: Option<&Track>, fps: u32, base: f64, factor: f64, offset: f64) -> Value {
    let Some(track) = track else {
        return prop(base * factor + offset);
    };
    let keys:Vec<_>=track.frames.iter().map(|f|{let(a,b)=easing(f.easing);json!({"t":f.time_ms as f64*fps as f64/1000.,"s":[f.value*factor+offset],"h":if f.easing==Easing::Step{1}else{0},"o":{"x":[1./3.],"y":[a]},"i":{"x":[2./3.],"y":[b]}})}).collect();
    json!({"a":1,"k":keys})
}
struct Writer<'a> {
    doc: &'a Document,
    assets: Vec<Value>,
    fonts: Vec<Value>,
    report: Report,
    frames: f64,
    asset_bytes: usize,
}
impl Writer<'_> {
    fn asset(&mut self, value: Value) -> Result<()> {
        let bytes = serde_json::to_vec(&value)
            .map_err(|e| error(e.to_string()))?
            .len();
        self.asset_bytes = self
            .asset_bytes
            .checked_add(bytes)
            .ok_or_else(|| error("Asset budget overflow"))?;
        if self.asset_bytes > MAX_BYTES {
            return Err(error("Embedded export assets exceed 64 MiB"));
        }
        self.assets.push(value);
        Ok(())
    }
    fn layers(&mut self, parent: Option<NodeId>) -> Result<Vec<Value>> {
        let nodes: Vec<_> = self
            .doc
            .nodes
            .iter()
            .filter(|n| n.parent == parent)
            .collect();
        nodes.into_iter().rev().map(|n| self.layer(n)).collect()
    }
    fn layer(&mut self, node: &Node) -> Result<Value> {
        if self.doc.design.keyframes.contains_key(&node.id)
            && self
                .doc
                .design
                .keyframes
                .keys()
                .any(|ancestor| *ancestor != node.id && self.doc.is_ancestor(*ancestor, node.id))
        {
            return Err(error(format!(
                "{} and its parent both animate; native world-axis composition requires rendered-frame export",
                node.name
            )));
        }
        if self
            .doc
            .design
            .frames
            .get(&node.id)
            .is_some_and(|f| f.clip_content)
        {
            return Err(error(format!(
                "{} clips its responsive frame; choose rendered-frame export",
                node.name
            )));
        }
        if node.blending != Default::default() {
            return Err(error(format!(
                "{} uses advanced blending; choose rendered-frame export",
                node.name
            )));
        }
        if node.mask.is_some()
            || node.clip_to.is_some()
            || (!node.styles.is_empty() && node.effects_enabled)
            || !matches!(
                node.blend,
                emulsion_raster::BlendMode::Normal | emulsion_raster::BlendMode::PassThrough
            )
        {
            return Err(error(format!(
                "{} uses masks, clipping, effects or blend modes; choose rendered-frame export",
                node.name
            )));
        }
        let mut layer = json!({"ind":node.id,"nm":node.name,"ip":0,"op":self.frames,"st":0,"sr":1,"hd":!node.visible,"ks":self.transform(node)?});
        match &node.kind {
            NodeKind::Group { .. } => {
                let layers = self.layers(Some(node.id))?;
                let id = format!("group-{}", node.id);
                self.asset(json!({"id":id,"layers":layers}))?;
                layer["ty"] = json!(0);
                layer["refId"] = json!(id);
                layer["w"] = json!(self.doc.width);
                layer["h"] = json!(self.doc.height);
            }
            NodeKind::Path { path, style, .. } => {
                layer["ty"] = json!(4);
                layer["shapes"] = shapes(path, *style)?;
            }
            NodeKind::Fill { rgba } => {
                layer["ty"] = json!(4);
                layer["shapes"] = shapes(
                    &emulsion_raster::vector_geometry::rectangle(
                        0.,
                        0.,
                        self.doc.width as f64,
                        self.doc.height as f64,
                    ),
                    PathStyle {
                        fill: Some(*rgba),
                        stroke: None,
                        ..Default::default()
                    },
                )?;
            }
            NodeKind::Raster { raster, placement } => {
                if !placement.is_identity() {
                    return Err(error(format!(
                        "{} has a placed raster transform; use rendered-frame export",
                        node.name
                    )));
                }
                if u64::from(raster.width()) * u64::from(raster.height()) > 16_000_000 {
                    return Err(error(
                        "Editable image export is limited to 16 megapixels per asset",
                    ));
                }
                let id = format!("image-{}", node.id);
                let png =
                    crate::export::png8(raster.width(), raster.height(), &raster.to_srgba8())?;
                self.asset(json!({"id":id,"w":raster.width(),"h":raster.height(),"u":"","p":format!("data:image/png;base64,{}",STANDARD.encode(png)),"e":1}))?;
                layer["ty"] = json!(2);
                layer["refId"] = json!(id);
            }
            NodeKind::Text { spec, .. } => {
                if !spec.runs.is_empty()
                    || !spec.paragraphs.is_empty()
                    || spec.rotation != 0.
                    || spec.scale_x != 1.
                    || spec.scale_y != 1.
                    || spec.text_path.is_some()
                    || spec.width.is_some()
                    || spec.height.is_some()
                    || spec.underline
                    || spec.strikethrough
                    || spec.vertical
                    || spec.warp != Default::default()
                {
                    return Err(error(format!(
                        "{} uses advanced typography; choose rendered-frame export",
                        node.name
                    )));
                }
                let name = format!("font-{}", node.id);
                self.fonts.push(json!({"fName":name,"fFamily":spec.font,"fStyle":match (spec.bold,spec.italic){(true,true)=>"Bold Italic",(true,false)=>"Bold",(false,true)=>"Italic",_=>"Regular"},"ascent":75}));
                layer["ty"] = json!(5);
                layer["t"] = json!({"d":{"k":[{"t":0,"s":{"t":spec.text.replace('\n',"\r"),"f":name,"s":spec.size,"lh":spec.size*spec.line_height,"j":0,"tr":spec.letter_spacing*1000./spec.size,"fc":color(spec.color),"ps":[spec.x,spec.y+spec.size]}}]},"a":[]});
                self.report.warn("Text remains editable and uses installed fonts; baseline/font metrics may differ between players.");
            }
            _ => {
                return Err(error(format!(
                    "{} is not a supported Lottie vector/image/text object; choose rendered-frame export",
                    node.name
                )));
            }
        }
        Ok(layer)
    }
    fn transform(&mut self, node: &Node) -> Result<Value> {
        let tracks = self.doc.design.keyframes.get(&node.id);
        let track = |p| tracks.and_then(|ts| ts.iter().find(|t| t.property == p));
        if track(Property::TextReveal).is_some() {
            return Err(error("Text reveal requires rendered-frame export"));
        }
        let bounds = emulsion_core::geometry::node_bounds(self.doc, node.id);
        let center = bounds
            .map(|b| [b.x as f64 + b.w as f64 / 2., b.y as f64 + b.h as f64 / 2.])
            .unwrap_or([0., 0.]);
        let fps = self.doc.design.fps;
        let base_opacity = f64::from(node.opacity)
            * match &node.kind {
                NodeKind::Text { spec, .. } => f64::from(spec.color[3]) / 255.,
                _ => 1.,
            };
        let mut ks = json!({"a":prop(center),"p":{"s":true,"x":scalar_track(track(Property::TranslationX),fps,0.,1.,center[0]),"y":scalar_track(track(Property::TranslationY),fps,0.,1.,center[1])},"r":scalar_track(track(Property::Rotation),fps,0.,1.,0.),"o":scalar_track(track(Property::Opacity),fps,1.,base_opacity*100.,0.)});
        let sx = track(Property::ScaleX);
        let sy = track(Property::ScaleY);
        let visibility = track(Property::Visibility);
        let mut times = BTreeSet::new();
        if sx.is_some() || sy.is_some() || visibility.is_some() {
            for t in sx.into_iter().chain(sy).chain(visibility) {
                times.extend(t.frames.iter().map(|f| f.time_ms));
            }
            for i in 0..=self.frames.ceil() as u32 {
                times.insert(
                    ((i as f64 * 1000. / fps as f64).round() as u32)
                        .min(self.doc.design.duration_ms),
                );
            }
        }
        if sx.is_none() && sy.is_none() {
            ks["s"] = prop([100, 100, 100]);
        } else {
            let keys:Vec<_>=times.iter().map(|ms|json!({"t":*ms as f64*fps as f64/1000.,"s":[sx.map(|t|t.sample(*ms)).unwrap_or(1.)*100.,sy.map(|t|t.sample(*ms)).unwrap_or(1.)*100.,100.],"h":0,"o":{"x":[0.333],"y":[0.333]},"i":{"x":[0.667],"y":[0.667]}})).collect();
            ks["s"] = json!({"a":1,"k":keys});
            self.report.warn("Independent scale channels are sampled to composition frame rate as editable vector transform keys.");
        }
        if let Some(visible) = visibility {
            let opacity = track(Property::Opacity);
            let keys:Vec<_>=times.iter().map(|ms|json!({"t":*ms as f64*fps as f64/1000.,"s":[if visible.sample(*ms)>=0.5{opacity.map(|t|t.sample(*ms)).unwrap_or(1.)*base_opacity*100.}else{0.}],"h":1})).collect();
            ks["o"] = json!({"a":1,"k":keys});
        }
        Ok(ks)
    }
}
pub(super) fn encode(doc: &Document) -> Result<(Vec<u8>, Report)> {
    doc.validate()?;
    if !doc.design.motion.is_empty() {
        return Err(error(
            "Legacy enter/exit effects require rendered-frame export; property keyframes are editable-vector supported",
        ));
    }
    let mut writer = Writer {
        doc,
        assets: vec![],
        fonts: vec![],
        report: Report::default(),
        frames: doc.design.duration_ms as f64 * doc.design.fps as f64 / 1000.,
        asset_bytes: 0,
    };
    if writer.frames > 600. {
        return Err(error("Export supports up to 600 frames"));
    }
    let layers = writer.layers(None)?;
    let result = json!({"v":"5.12.2","fr":doc.design.fps,"ip":0,"op":writer.frames,"w":doc.width,"h":doc.height,"nm":"Emulsion editable animation","ddd":0,"assets":writer.assets,"fonts":{"list":writer.fonts},"layers":layers});
    let bytes = serde_json::to_vec(&result).map_err(|e| error(e.to_string()))?;
    if bytes.len() > MAX_BYTES {
        return Err(error("Export exceeds 64 MiB"));
    }
    writer.report.nodes = doc.nodes.len();
    writer.report.animated_nodes = doc.design.keyframes.len();
    writer.report.warn("Active page only. Presentation actions, audio/video playback and slide transitions are not exported.");
    Ok((bytes, writer.report))
}
