//! Enhance: one-click photo tools in the spirit of Luminar. Every tool lands
//! as ordinary editable content — a smart filter on the photo, or model-made
//! layers with masks — so undo, Properties and the assistant see nothing
//! special. The panel then shows each active tool's sliders in place.

use super::*;
use emulsion_ai::jobs::Job;
use emulsion_ai::models::Task;
use emulsion_ai::{depth, matte, sky};
use emulsion_filters::Filter;
use emulsion_raster::{IRect, Mask};
use gpui_kit::component::{
    Sizable,
    button::{Button, ButtonVariants},
};
use std::collections::HashMap;

/// What a tool does when it is switched on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Action {
    /// Add the catalogue filter with this key to the photo's filter stack.
    Filter(&'static str),
    /// Run a model-backed composition; its card has an Apply button.
    Ai(AiTool),
    /// Hand over to an existing editor command.
    Command(Shortcut),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum AiTool {
    Sky,
    Relight,
    Fog,
    Bokeh,
    Expand,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Shortcut {
    Faces,
    Upscale,
    RemoveObjects,
    RemoveBackground,
}

struct ToolDef {
    id: &'static str,
    /// Catalog keys for the card's name and description.
    title: &'static str,
    blurb: &'static str,
    action: Action,
}

const fn tool(
    id: &'static str,
    title: &'static str,
    blurb: &'static str,
    action: Action,
) -> ToolDef {
    ToolDef {
        id,
        title,
        blurb,
        action,
    }
}

/// Section catalog keys with their tools.
const SECTIONS: &[(&str, &[ToolDef])] = &[
    (
        "editor.enhance_ui.section_essentials",
        &[
            tool(
                "enhance",
                "editor.enhance_ui.enhance",
                "editor.enhance_ui.enhance_blurb",
                Action::Filter("enhance"),
            ),
            tool(
                "structure",
                "editor.enhance_ui.structure",
                "editor.enhance_ui.structure_blurb",
                Action::Filter("structure"),
            ),
            tool(
                "denoise",
                "editor.enhance_ui.denoise",
                "editor.enhance_ui.denoise_blurb",
                Action::Filter("reduce_noise"),
            ),
            tool(
                "sharpen",
                "editor.enhance_ui.sharpen",
                "editor.enhance_ui.sharpen_blurb",
                Action::Filter("smart_sharpen"),
            ),
            tool(
                "grain",
                "editor.enhance_ui.grain",
                "editor.enhance_ui.grain_blurb",
                Action::Filter("add_noise"),
            ),
        ],
    ),
    (
        "editor.enhance_ui.section_creative",
        &[
            tool(
                "glow",
                "editor.enhance_ui.glow",
                "editor.enhance_ui.glow_blurb",
                Action::Filter("glow"),
            ),
            tool(
                "orton",
                "editor.enhance_ui.orton",
                "editor.enhance_ui.orton_blurb",
                Action::Filter("orton"),
            ),
            tool(
                "sunrays",
                "editor.enhance_ui.sunrays",
                "editor.enhance_ui.sunrays_blurb",
                Action::Filter("sunrays"),
            ),
            tool(
                "golden_hour",
                "editor.enhance_ui.golden_hour",
                "editor.enhance_ui.golden_hour_blurb",
                Action::Filter("golden_hour"),
            ),
            tool(
                "dramatic",
                "editor.enhance_ui.dramatic",
                "editor.enhance_ui.dramatic_blurb",
                Action::Filter("dramatic"),
            ),
        ],
    ),
    (
        "editor.enhance_ui.section_portrait",
        &[
            tool(
                "skin",
                "editor.enhance_ui.skin",
                "editor.enhance_ui.skin_blurb",
                Action::Filter("skin_smooth"),
            ),
            tool(
                "faces",
                "editor.enhance_ui.faces",
                "editor.enhance_ui.faces_blurb",
                Action::Command(Shortcut::Faces),
            ),
            tool(
                "bokeh",
                "editor.enhance_ui.bokeh",
                "editor.enhance_ui.bokeh_blurb",
                Action::Ai(AiTool::Bokeh),
            ),
        ],
    ),
    (
        "editor.enhance_ui.section_landscape",
        &[
            tool(
                "sky",
                "editor.enhance_ui.sky",
                "editor.enhance_ui.sky_blurb",
                Action::Ai(AiTool::Sky),
            ),
            tool(
                "atmosphere",
                "editor.enhance_ui.atmosphere",
                "editor.enhance_ui.atmosphere_blurb",
                Action::Filter("atmosphere"),
            ),
            tool(
                "relight",
                "editor.enhance_ui.relight",
                "editor.enhance_ui.relight_blurb",
                Action::Ai(AiTool::Relight),
            ),
            tool(
                "fog",
                "editor.enhance_ui.fog",
                "editor.enhance_ui.fog_blurb",
                Action::Ai(AiTool::Fog),
            ),
        ],
    ),
    (
        "editor.enhance_ui.section_erase",
        &[
            tool(
                "remove",
                "editor.enhance_ui.remove",
                "editor.enhance_ui.remove_blurb",
                Action::Command(Shortcut::RemoveObjects),
            ),
            tool(
                "background",
                "editor.enhance_ui.background",
                "editor.enhance_ui.background_blurb",
                Action::Command(Shortcut::RemoveBackground),
            ),
            tool(
                "expand",
                "editor.enhance_ui.expand",
                "editor.enhance_ui.expand_blurb",
                Action::Ai(AiTool::Expand),
            ),
            tool(
                "upscale",
                "editor.enhance_ui.upscale",
                "editor.enhance_ui.upscale_blurb",
                Action::Command(Shortcut::Upscale),
            ),
        ],
    ),
];

/// One-click looks: a stack of Enhance filters with chosen settings.
/// A filter key with the parameters it is set to.
type LookStep = (&'static str, &'static [(&'static str, f32)]);

pub(crate) const LOOKS: &[(&str, &[LookStep])] = &[
    (
        "Vivid",
        &[
            ("enhance", &[("amount", 60.0), ("sky", 40.0)]),
            ("structure", &[("amount", 25.0)]),
        ],
    ),
    (
        "Landscape",
        &[
            ("enhance", &[("amount", 50.0), ("sky", 70.0)]),
            ("structure", &[("amount", 40.0)]),
            ("atmosphere", &[("amount", -25.0)]),
        ],
    ),
    (
        "Soft portrait",
        &[
            ("skin_smooth", &[("amount", 55.0)]),
            ("orton", &[("amount", 18.0)]),
            ("golden_hour", &[("amount", 20.0)]),
        ],
    ),
    (
        "Dreamy",
        &[
            ("orton", &[("amount", 45.0)]),
            ("glow", &[("amount", 35.0)]),
        ],
    ),
    (
        "Golden",
        &[
            ("golden_hour", &[("amount", 65.0)]),
            ("glow", &[("amount", 20.0)]),
        ],
    ),
    (
        "Moody",
        &[
            ("dramatic", &[("amount", 45.0)]),
            ("atmosphere", &[("amount", 15.0)]),
        ],
    ),
];

/// The filter `key` from the catalogue with `params` applied.
pub(crate) fn look_filter(key: &str, params: &[(&str, f32)]) -> Option<Filter> {
    let mut f = Filter::catalogue().into_iter().find(|f| f.key() == key)?;
    for (k, v) in params {
        f.set_param(k, *v);
    }
    Some(f)
}

#[derive(Default)]
pub(crate) struct EnhanceState {
    /// The expanded tool card.
    pub(crate) open: Option<&'static str>,
    /// Chosen built-in sky; `None` means the custom image below.
    pub(crate) sky: Option<sky::SkyPreset>,
    custom_sky: Option<(String, Arc<Raster>)>,
    /// Layers each model-backed tool made, for its sliders.
    pub(crate) made: HashMap<AiTool, Vec<NodeId>>,
}

/// The undo step name, kept in English like other history entries.
fn ai_label(tool: AiTool) -> &'static str {
    match tool {
        AiTool::Sky => "Sky replacement",
        AiTool::Relight => "Relight",
        AiTool::Fog => "Depth fog",
        AiTool::Bokeh => "Portrait bokeh",
        AiTool::Expand => "Expand",
    }
}

/// The tool's name in the interface language.
fn ai_title(tool: AiTool) -> String {
    match tool {
        AiTool::Sky => t!("editor.enhance_ui.sky"),
        AiTool::Relight => t!("editor.enhance_ui.relight"),
        AiTool::Fog => t!("editor.enhance_ui.fog"),
        AiTool::Bokeh => t!("editor.enhance_ui.bokeh"),
        AiTool::Expand => t!("editor.enhance_ui.expand"),
    }
    .into_owned()
}

/// A one-click look's displayed name; `LOOKS` keeps the English identifier.
fn look_label(name: &str) -> String {
    match name {
        "Vivid" => t!("editor.enhance_ui.look_vivid"),
        "Landscape" => t!("editor.enhance_ui.look_landscape"),
        "Soft portrait" => t!("editor.enhance_ui.look_soft_portrait"),
        "Dreamy" => t!("editor.enhance_ui.look_dreamy"),
        "Golden" => t!("editor.enhance_ui.look_golden"),
        "Moody" => t!("editor.enhance_ui.look_moody"),
        _ => name.to_owned().into(),
    }
    .into_owned()
}

fn sky_label(preset: sky::SkyPreset) -> String {
    match preset {
        sky::SkyPreset::Blue => t!("editor.enhance_ui.sky_blue"),
        sky::SkyPreset::Clouds => t!("editor.enhance_ui.sky_clouds"),
        sky::SkyPreset::GoldenHour => t!("editor.enhance_ui.sky_golden_hour"),
        sky::SkyPreset::Sunset => t!("editor.enhance_ui.sky_sunset"),
        sky::SkyPreset::Dusk => t!("editor.enhance_ui.sky_dusk"),
        sky::SkyPreset::Stormy => t!("editor.enhance_ui.sky_stormy"),
    }
    .into_owned()
}

/// The part of a mask with sky in it: hardened a little so the new sky does
/// not ghost into trees and roofs, then feathered so edges stay soft.
fn sky_layer_mask(raw: &Mask) -> Mask {
    let m = matte::harden(raw, 40, 215);
    emulsion_raster::select::feather(&m, 1.5)
}

/// A soft grey mask from a 0–1 map with a power curve.
fn curve_mask(m: &Mask, gamma: f32, invert: bool) -> Mask {
    let px: Vec<u8> = m
        .to_gray8()
        .into_iter()
        .map(|v| {
            let t = v as f32 / 255.0;
            let t = if invert { 1.0 - t } else { t };
            (t.powf(gamma) * 255.0).round() as u8
        })
        .collect();
    Mask::from_gray8(m.width(), m.height(), &px)
}

/// The ring added around a `w × h` picture grown by `pad` on every side,
/// reaching `overlap` pixels into the picture so the fill blends.
pub(crate) fn expand_ring(w: u32, h: u32, pad: u32, overlap: u32) -> Mask {
    let (nw, nh) = (w + 2 * pad, h + 2 * pad);
    let inner = IRect::new(
        (pad + overlap) as i32,
        (pad + overlap) as i32,
        w.saturating_sub(2 * overlap) as i32,
        h.saturating_sub(2 * overlap) as i32,
    );
    let px: Vec<u8> = (0..nh)
        .flat_map(|y| {
            (0..nw).map(move |x| {
                let inside = (x as i32) >= inner.x
                    && (x as i32) < inner.x + inner.w
                    && (y as i32) >= inner.y
                    && (y as i32) < inner.y + inner.h;
                if inside { 0 } else { 255 }
            })
        })
        .collect();
    Mask::from_gray8(nw, nh, &px)
}

impl EditorView {
    /// The layer Enhance filters work on: the selected pixel or smart layer,
    /// else the topmost visible one — usually the photo itself.
    pub(crate) fn enhance_target(&self) -> Option<NodeId> {
        let doc = &self.editor.doc;
        let usable = |id: NodeId| {
            doc.locked_ancestor(id).is_none()
                && doc.node(id).is_some_and(|n| {
                    matches!(n.kind, NodeKind::Raster { .. } | NodeKind::Smart { .. })
                })
        };
        if let Some(id) = self.selected.filter(|id| usable(*id)) {
            return Some(id);
        }
        doc.nodes
            .iter()
            .rev()
            .find(|n| n.visible && usable(n.id))
            .map(|n| n.id)
    }

    /// Where the filter `key` sits in the target's stack.
    fn enhance_filter_index(&self, id: NodeId, key: &str) -> Option<usize> {
        match &self.editor.doc.node(id)?.kind {
            NodeKind::Smart { filters, .. } => filters.iter().position(|f| f.key() == key),
            _ => None,
        }
    }

    /// Switch a filter tool on (or just open it when it already is).
    pub(crate) fn enhance_filter(&mut self, key: &'static str, cx: &mut Context<Self>) {
        if !self.effects_ready() {
            self.set_status(t!("editor.enhance_ui.finish_first"), false, cx);
            return;
        }
        let Some(id) = self.enhance_target() else {
            self.set_status(t!("editor.enhance_ui.open_photo_first"), false, cx);
            return;
        };
        if self.enhance_filter_index(id, key).is_none() {
            let Some(f) = look_filter(key, &[]) else {
                return;
            };
            self.set_layer_selection(vec![id], Some(id));
            self.add_filters(id, vec![f], cx);
            self.set_status(t!("editor.enhance_ui.added_filter"), false, cx);
        }
        cx.notify();
    }

    /// Apply a one-click look to the photo.
    pub(crate) fn enhance_look(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some((_, steps)) = LOOKS.iter().find(|(n, _)| *n == name) else {
            return;
        };
        if !self.effects_ready() {
            self.set_status(t!("editor.enhance_ui.finish_first"), false, cx);
            return;
        }
        let Some(id) = self.enhance_target() else {
            self.set_status(t!("editor.enhance_ui.open_photo_first"), false, cx);
            return;
        };
        let filters: Vec<Filter> = steps
            .iter()
            .filter_map(|(key, params)| look_filter(key, params))
            .collect();
        self.set_layer_selection(vec![id], Some(id));
        self.add_filters(id, filters, cx);
        self.set_status(
            t!("editor.enhance_ui.applied_look", name = look_label(name)),
            false,
            cx,
        );
    }

    fn enhance_toggle(&mut self, def: &'static ToolDef, cx: &mut Context<Self>) {
        let open = self.enhance.open == Some(def.id);
        self.enhance.open = (!open).then_some(def.id);
        match def.action {
            Action::Filter(key) if !open => self.enhance_filter(key, cx),
            _ => cx.notify(),
        }
    }

    fn enhance_command(&mut self, what: Shortcut, cx: &mut Context<Self>) {
        match what {
            Shortcut::Faces => self.restore_faces(cx),
            Shortcut::Upscale => self.ai_upscale(cx),
            Shortcut::RemoveBackground => self.remove_background(cx),
            Shortcut::RemoveObjects => {
                self.set_remove_mode(true, cx);
                self.set_status(t!("editor.enhance_ui.paint_remove"), false, cx);
            }
        }
    }

    pub(crate) fn run_enhance_ai(&mut self, tool: AiTool, cx: &mut Context<Self>) {
        if !self.effects_ready() || self.pending_edit_job.is_some() {
            self.set_status(t!("editor.enhance_ui.finish_first"), false, cx);
            return;
        }
        match tool {
            AiTool::Sky => self.replace_sky(cx),
            AiTool::Relight => self.depth_tool(AiTool::Relight, cx),
            AiTool::Fog => self.depth_tool(AiTool::Fog, cx),
            AiTool::Bokeh => self.portrait_bokeh(cx),
            AiTool::Expand => self.expand_canvas(0.15, cx),
        }
    }

    /// Add `nodes` on top in one undo step and remember them for `tool`.
    fn land_layers(&mut self, tool: AiTool, nodes: Vec<Node>, cx: &mut Context<Self>) {
        self.editor.begin(ai_label(tool));
        let mut ids = Vec::new();
        for node in nodes {
            if let Some(id) = self.execute(
                Command::AddNode {
                    node: Box::new(node),
                    slot: Slot::TOP,
                },
                cx,
            ) {
                ids.push(id);
            }
        }
        self.editor.end();
        if let Some(&first) = ids.first() {
            self.set_layer_selection(vec![first], Some(first));
        }
        self.enhance.made.insert(tool, ids);
        self.enhance.open = Some(match tool {
            AiTool::Sky => "sky",
            AiTool::Relight => "relight",
            AiTool::Fog => "fog",
            AiTool::Bokeh => "bokeh",
            AiTool::Expand => "expand",
        });
        cx.notify();
    }

    /// Replace the sky: segment it, add the new sky masked to it, and tint
    /// the foreground toward the new sky's colour so the light agrees.
    pub(crate) fn replace_sky(&mut self, cx: &mut Context<Self>) {
        if emulsion_ai::models::installed_for(Task::Sky).is_none() {
            self.set_status(super::ai_tools::missing(Task::Sky), true, cx);
            return;
        }
        let preset = self.enhance.sky;
        let custom = self.enhance.custom_sky.clone();
        if preset.is_none() && custom.is_none() {
            self.set_status(t!("editor.enhance_ui.choose_sky_first"), false, cx);
            return;
        }
        let ticket = self.begin_edit_job();
        let img = self.composite_raster();
        let (w, h) = (self.editor.doc.width, self.editor.doc.height);
        let job = Job::new();
        self.watch_job(job.clone(), &t!("editor.enhance_ui.replacing_sky"), cx);
        let j = job.clone();
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_spawn(async move {
                    let img = img.await;
                    let r = sky::mask(&img, &j).and_then(|raw| {
                        let Some(horizon) =
                            sky::horizon(&raw).filter(|_| sky::coverage(&raw) > 0.01)
                        else {
                            return Err(emulsion_ai::runner::RunError::Other(
                                t!("editor.enhance_ui.no_sky").into_owned(),
                            ));
                        };
                        j.set_stage(t!("editor.enhance_ui.stage_drawing_sky"));
                        let (name, new_sky) = match (preset, &custom) {
                            (Some(p), _) => {
                                (p.label().to_string(), sky::render_preset(p, w, h, horizon))
                            }
                            (None, Some((name, r))) => (name.clone(), sky::fit_image(r, w, h)),
                            (None, None) => unreachable!(),
                        };
                        let mask = sky_layer_mask(&raw);
                        let tint = sky::mean_color(&new_sky, &mask);
                        Ok((name, new_sky, mask, tint))
                    });
                    j.finish();
                    r
                })
                .await;
            this.update(cx, |this, cx| {
                if !this.accept_edit_result(ticket, &ai_title(AiTool::Sky), cx) || job.cancelled() {
                    return;
                }
                match r {
                    Ok((name, new_sky, mask, tint)) => {
                        let mask = Arc::new(mask);
                        let mut sky_node = Node::raster(
                            0,
                            format!("Sky · {name}"),
                            Arc::new(new_sky),
                            Placement::default(),
                        )
                        .from_model("skyseg");
                        sky_node.mask = Some(mask.clone());
                        let (hue, saturation) = sky::hue_saturation(tint);
                        let mut relight = Node::adjust(
                            0,
                            Adjustment::PhotoFilter {
                                hue,
                                saturation: saturation.max(30.0),
                                density: 25.0,
                                preserve_luminosity: true,
                            },
                        );
                        relight.name = "Sky relight".into();
                        relight.mask = Some(Arc::new(emulsion_raster::select::invert(&mask)));
                        this.land_layers(AiTool::Sky, vec![sky_node, relight], cx);
                        this.set_status(t!("editor.enhance_ui.sky_replaced"), false, cx);
                    }
                    Err(e) => this.set_status(
                        t!(
                            "editor.enhance_ui.tool_error",
                            tool = ai_title(AiTool::Sky),
                            error = e
                        ),
                        true,
                        cx,
                    ),
                }
            })
            .ok();
        })
        .detach();
    }

    /// Pick a photo of a sky for sky replacement.
    fn choose_custom_sky(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(t!("editor.enhance_ui.choose_sky").into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let name = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Custom".into());
            let decoded = cx
                .background_spawn(async move {
                    emulsion_io::import::decode(&path).map_err(|e| e.to_string())
                })
                .await;
            this.update(cx, |this, cx| match decoded {
                Ok(d) => {
                    this.enhance.custom_sky = Some((name, Arc::new(d.raster)));
                    this.enhance.sky = None;
                    cx.notify();
                }
                Err(e) => {
                    this.set_status(t!("editor.enhance_ui.sky_image_error", error = e), true, cx)
                }
            })
            .ok();
        })
        .detach();
    }

    /// Relight and depth fog both start from a depth map.
    fn depth_tool(&mut self, tool: AiTool, cx: &mut Context<Self>) {
        if depth::available().is_none() {
            self.set_status(super::ai_tools::missing(Task::Depth), true, cx);
            return;
        }
        let ticket = self.begin_edit_job();
        let img = self.composite_raster();
        let job = Job::new();
        self.watch_job(job.clone(), &ai_title(tool), cx);
        let j = job.clone();
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_spawn(async move {
                    let img = img.await;
                    let r = depth::estimate(&img, &j).map(|m| m.to_mask());
                    j.finish();
                    r
                })
                .await;
            this.update(cx, |this, cx| {
                if !this.accept_edit_result(ticket, &ai_title(tool), cx) || job.cancelled() {
                    return;
                }
                let near = match r {
                    Ok(m) => m,
                    Err(e) => {
                        this.set_status(
                            t!(
                                "editor.enhance_ui.tool_error",
                                tool = ai_title(tool),
                                error = e
                            ),
                            true,
                            cx,
                        );
                        return;
                    }
                };
                let model = depth::available().map(|m| m.id).unwrap_or("depth");
                let nodes = match tool {
                    AiTool::Relight => {
                        let exposure = |exposure| Adjustment::Exposure {
                            exposure,
                            offset: 0.0,
                            gamma: 1.0,
                        };
                        let mut n = Node::adjust(0, exposure(0.6)).from_model(model);
                        n.name = "Relight · near".into();
                        n.mask = Some(Arc::new(curve_mask(&near, 1.5, false)));
                        let mut f = Node::adjust(0, exposure(-0.3)).from_model(model);
                        f.name = "Relight · far".into();
                        f.mask = Some(Arc::new(curve_mask(&near, 1.5, true)));
                        vec![n, f]
                    }
                    _ => {
                        let mut fog = Node::new(
                            0,
                            "Depth fog",
                            NodeKind::Fill {
                                rgba: [226, 231, 236, 255],
                            },
                        )
                        .from_model(model);
                        fog.opacity = 0.55;
                        fog.mask = Some(Arc::new(curve_mask(&near, 1.8, true)));
                        vec![fog]
                    }
                };
                this.land_layers(tool, nodes, cx);
                this.set_status(
                    t!("editor.enhance_ui.masked_layers", tool = ai_title(tool)),
                    false,
                    cx,
                );
            })
            .ok();
        })
        .detach();
    }

    /// Blur a copy of the picture everywhere but the subject.
    fn portrait_bokeh(&mut self, cx: &mut Context<Self>) {
        let use_matte = matte::available().is_some();
        if !use_matte && depth::available().is_none() {
            self.set_status(super::ai_tools::missing(Task::Matte), true, cx);
            return;
        }
        let ticket = self.begin_edit_job();
        let img = self.composite_raster();
        let job = Job::new();
        self.watch_job(
            job.clone(),
            &t!("editor.enhance_ui.blurring_background"),
            cx,
        );
        let j = job.clone();
        cx.spawn(async move |this, cx| {
            let r = cx
                .background_spawn(async move {
                    let img = img.await;
                    let subject = if use_matte {
                        matte::matte(&img, &Default::default(), &j)
                    } else {
                        depth::estimate(&img, &j).map(|m| curve_mask(&m.to_mask(), 0.6, false))
                    };
                    let r = subject.map(|subject| {
                        j.set_stage(t!("editor.enhance_ui.stage_blurring"));
                        let radius =
                            (img.width().min(img.height()) as f32 / 120.0).clamp(4.0, 30.0);
                        let bg =
                            emulsion_raster::select::invert(&emulsion_raster::select::feather(
                                &matte::harden(&subject, 30, 225),
                                3.0,
                            ));
                        let node = Node::smart(
                            0,
                            "Portrait bokeh",
                            Arc::new(img),
                            vec![Filter::LensBlur { radius }],
                            Placement::default(),
                        );
                        (node, bg)
                    });
                    j.finish();
                    r
                })
                .await;
            this.update(cx, |this, cx| {
                if !this.accept_edit_result(ticket, &ai_title(AiTool::Bokeh), cx) || job.cancelled()
                {
                    return;
                }
                match r {
                    Ok((mut node, bg)) => {
                        node.mask = Some(Arc::new(bg));
                        this.land_layers(AiTool::Bokeh, vec![node], cx);
                        this.set_status(t!("editor.enhance_ui.background_blurred"), false, cx);
                    }
                    Err(e) => this.set_status(
                        t!(
                            "editor.enhance_ui.tool_error",
                            tool = ai_title(AiTool::Bokeh),
                            error = e
                        ),
                        true,
                        cx,
                    ),
                }
            })
            .ok();
        })
        .detach();
    }

    /// Grow the canvas by `fraction` of its shorter side on every side and
    /// fill the new edges with the inpainting model, or from the
    /// surroundings when it is not installed.
    pub(crate) fn expand_canvas(&mut self, fraction: f32, cx: &mut Context<Self>) {
        let (w, h) = (self.editor.doc.width, self.editor.doc.height);
        let pad = ((w.min(h) as f32 * fraction).round() as u32).max(8);
        if w + 2 * pad > 16_384 || h + 2 * pad > 16_384 {
            self.set_status(t!("editor.enhance_ui.too_large"), true, cx);
            return;
        }
        self.editor.begin("Expand");
        self.crop_canvas(
            IRect::new(
                -(pad as i32),
                -(pad as i32),
                (w + 2 * pad) as i32,
                (h + 2 * pad) as i32,
            ),
            0.0,
            false,
            false,
            cx,
        );
        let ring = expand_ring(w, h, pad, 4);
        self.execute(
            Command::SetSelection {
                selection: Some(Arc::new(ring)),
            },
            cx,
        );
        self.editor.end();
        self.fit_pending = true;
        self.enhance.open = Some("expand");
        if emulsion_ai::inpaint::available().is_some() {
            self.ai_fill(cx);
        } else {
            self.content_aware_fill(cx);
        }
    }

    // ── The panel ────────────────────────────────────────────────────────

    pub(crate) fn enhance_panel(&mut self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let target = self.enhance_target();
        let target_name = target
            .and_then(|id| self.editor.doc.node(id))
            .map(|n| n.name.clone());
        let mut body = div()
            .id("enhance-panel")
            .test_support()
            .flex()
            .flex_col()
            .gap(px(6.))
            .px(px(12.))
            .py(px(9.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(label(t!("editor.enhance_ui.enhance"), p))
                    .child(div().flex_1())
                    .child(mono(
                        target_name.unwrap_or_else(|| t!("editor.enhance_ui.no_photo").into()),
                        9.,
                        p.muted,
                    )),
            );
        let mut looks = div().flex().flex_wrap().items_center().gap(px(4.)).child(
            mono(t!("editor.enhance_ui.looks"), 9., p.muted)
                .w(px(44.))
                .flex_none(),
        );
        for (i, (name, _)) in LOOKS.iter().enumerate() {
            let n: &'static str = name;
            looks = looks.child(
                chip(("enhance-look", i), look_label(n), false, p)
                    .on_click(cx.listener(move |this, _, _, cx| this.enhance_look(n, cx))),
            );
        }
        body = body.child(looks);
        for (section, tools) in SECTIONS {
            body = body.child(
                div()
                    .pt(px(6.))
                    .border_t_1()
                    .border_color(p.line)
                    .child(label(t!(*section), p)),
            );
            for def in tools.iter() {
                body = body.child(self.enhance_tool(def, target, p, cx));
            }
        }
        body.into_any_element()
    }

    fn enhance_tool(
        &mut self,
        def: &'static ToolDef,
        target: Option<NodeId>,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let active = match def.action {
            Action::Filter(key) => target
                .and_then(|id| self.enhance_filter_index(id, key))
                .is_some(),
            Action::Ai(tool) => !self.enhance_made(tool).is_empty(),
            Action::Command(_) => false,
        };
        let open = self.enhance.open == Some(def.id);
        let mut card = div().flex().flex_col().gap(px(5.)).child(
            Button::new(SharedString::from(format!("enhance-{}", def.id)))
                .label(format!(
                    "{} {}{}",
                    if open { "▾" } else { "▸" },
                    t!(def.title),
                    if active { "  ●" } else { "" }
                ))
                .small()
                .ghost()
                .w_full()
                .justify_start()
                .when(active, |b| b.text_color(p.accent))
                .on_click(cx.listener(move |this, _, _, cx| this.enhance_toggle(def, cx))),
        );
        if !open {
            return card.into_any_element();
        }
        card = card.child(
            div()
                .px(px(6.))
                .text_size(px(10.5))
                .text_color(p.muted)
                .child(t!(def.blurb)),
        );
        let mut controls = div().flex().flex_col().gap(px(6.)).px(px(6.)).pb(px(6.));
        match def.action {
            Action::Filter(key) => {
                if let Some((id, idx)) =
                    target.and_then(|id| Some((id, self.enhance_filter_index(id, key)?)))
                {
                    let specs = match &self.editor.doc.node(id).map(|n| &n.kind) {
                        Some(NodeKind::Smart { filters, .. }) => filters[idx].params(),
                        _ => Vec::new(),
                    };
                    for spec in specs {
                        let norm = (spec.value - spec.min) / (spec.max - spec.min).max(1e-6);
                        controls = controls.child(self.param_slider(
                            SliderKey::Filter(id, idx, spec.key),
                            spec.label,
                            spec.display(),
                            norm,
                            (spec.min, spec.max, spec.step),
                            p,
                            cx,
                        ));
                    }
                    controls = controls.child(
                        chip(
                            SharedString::from(format!("enhance-off-{}", def.id)),
                            t!("editor.enhance_ui.remove_filter"),
                            false,
                            p,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.remove_filter(id, idx, cx);
                        })),
                    );
                }
            }
            Action::Command(what) => {
                controls = controls.child(
                    button(
                        SharedString::from(format!("enhance-run-{}", def.id)),
                        t!(def.title),
                        false,
                        p,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.enhance_command(what, cx))),
                );
            }
            Action::Ai(tool) => {
                if tool == AiTool::Sky {
                    controls = controls.child(self.sky_choices(p, cx));
                }
                controls = controls.children(self.enhance_made_sliders(tool, p, cx));
                controls = controls.child(
                    button(
                        SharedString::from(format!("enhance-run-{}", def.id)),
                        if tool == AiTool::Expand {
                            t!("editor.enhance_ui.expand_canvas")
                        } else {
                            t!("editor.enhance_ui.apply")
                        },
                        false,
                        p,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.run_enhance_ai(tool, cx)))
                    .test_support(),
                );
            }
        }
        card.child(controls).into_any_element()
    }

    /// Layers `tool` made that are still in the document.
    fn enhance_made(&self, tool: AiTool) -> Vec<NodeId> {
        self.enhance
            .made
            .get(&tool)
            .map(|ids| {
                ids.iter()
                    .copied()
                    .filter(|id| self.editor.doc.node(*id).is_some())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn enhance_made_sliders(
        &mut self,
        tool: AiTool,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut out = Vec::new();
        for id in self.enhance_made(tool) {
            let Some(node) = self.editor.doc.node(id) else {
                continue;
            };
            let name = node.name.clone();
            let opacity = node.opacity;
            let slider = match &node.kind {
                NodeKind::Adjust(Adjustment::Exposure { exposure, .. }) => Some((
                    SliderKey::Param(id, "exposure"),
                    t!("editor.enhance_ui.layer_exposure", name = name).into_owned(),
                    format!("{exposure:+.2} ev"),
                    (exposure + 2.0) / 4.0,
                    (-2.0, 2.0, 0.01),
                )),
                NodeKind::Adjust(Adjustment::PhotoFilter { density, .. }) => Some((
                    SliderKey::Param(id, "density"),
                    t!("editor.enhance_ui.relight_scene").into_owned(),
                    format!("{density:.0}%"),
                    density / 100.0,
                    (0.0, 100.0, 1.0),
                )),
                NodeKind::Smart { filters, .. } => filters.first().and_then(|f| {
                    let spec = f.params().into_iter().find(|s| s.key == "radius")?;
                    Some((
                        SliderKey::Filter(id, 0, "radius"),
                        t!("editor.enhance_ui.blur").into_owned(),
                        spec.display(),
                        (spec.value - spec.min) / (spec.max - spec.min),
                        (spec.min, spec.max, spec.step),
                    ))
                }),
                _ => Some((
                    SliderKey::Opacity(id),
                    t!("editor.enhance_ui.layer_strength", name = name).into_owned(),
                    format!("{:.0}%", opacity * 100.0),
                    opacity,
                    (0.0, 100.0, 1.0),
                )),
            };
            if let Some((key, label, display, norm, spec)) = slider {
                out.push(
                    self.param_slider(key, &label, display, norm, spec, p, cx)
                        .into_any_element(),
                );
            }
        }
        out
    }

    fn sky_choices(&mut self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let mut row = div().flex().flex_wrap().gap(px(4.));
        for (i, preset) in sky::SkyPreset::ALL.into_iter().enumerate() {
            row = row.child(
                chip(
                    ("enhance-sky", i),
                    sky_label(preset),
                    self.enhance.sky == Some(preset),
                    p,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.enhance.sky = Some(preset);
                    cx.notify();
                }))
                .test_support(),
            );
        }
        let custom = self
            .enhance
            .custom_sky
            .as_ref()
            .map(|(name, _)| t!("editor.enhance_ui.custom_image", name = name));
        row = row.child(
            chip(
                "enhance-sky-custom",
                custom.unwrap_or_else(|| t!("editor.enhance_ui.your_image")),
                self.enhance.sky.is_none() && self.enhance.custom_sky.is_some(),
                p,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                if this.enhance.custom_sky.is_some() && this.enhance.sky.is_some() {
                    this.enhance.sky = None;
                    cx.notify();
                } else {
                    this.choose_custom_sky(cx);
                }
            })),
        );
        row.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{Action, LOOKS, Mask, SECTIONS, curve_mask, expand_ring, look_filter};

    #[test]
    fn every_tool_filter_and_look_names_a_catalogue_filter() {
        for (_, tools) in SECTIONS {
            for t in tools.iter() {
                if let Action::Filter(key) = t.action {
                    assert!(look_filter(key, &[]).is_some(), "{key}");
                }
            }
        }
        for (name, steps) in LOOKS {
            for (key, params) in steps.iter() {
                let f = look_filter(key, params).unwrap_or_else(|| panic!("{name}: {key}"));
                for (k, v) in params.iter() {
                    let got = f
                        .params()
                        .into_iter()
                        .find(|s| s.key == *k)
                        .map(|s| s.value);
                    assert_eq!(got, Some(*v), "{name}: {key}.{k}");
                }
            }
        }
    }

    #[test]
    fn tool_ids_are_unique() {
        let mut ids: Vec<_> = SECTIONS
            .iter()
            .flat_map(|(_, t)| t.iter().map(|t| t.id))
            .collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n);
    }

    #[test]
    fn expand_ring_covers_only_the_new_edges_and_a_seam() {
        let m = expand_ring(20, 10, 5, 2).to_gray8();
        let (w, h) = (30usize, 20usize);
        assert_eq!(m.len(), w * h);
        let at = |x: usize, y: usize| m[y * w + x];
        assert_eq!(at(0, 0), 255);
        assert_eq!(at(6, 6), 255, "the seam reaches into the picture");
        assert_eq!(at(15, 10), 0, "the middle stays");
        assert_eq!(at(29, 19), 255);
    }

    #[test]
    fn curve_mask_inverts_and_shapes() {
        let m = Mask::from_gray8(2, 1, &[0, 255]);
        assert_eq!(curve_mask(&m, 1.0, true).to_gray8(), vec![255, 0]);
        assert_eq!(curve_mask(&m, 2.0, false).to_gray8(), vec![0, 255]);
    }
}
