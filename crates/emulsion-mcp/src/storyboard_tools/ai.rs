//! AI image operations on storyboard panels: subject selection, subject
//! masks, background removal, upscale, denoise, expand and generative fill
//! over one or many panels, through `emulsion_ai::panels`. The panel size
//! never changes; results are new layers (originals hidden or kept), layer
//! masks or selections. All panels land as one Undo step.
//!
//! Hosts that can work off the UI thread call [`prepare`], run
//! [`Prepared::compute`] on a worker and hand the result to [`finish`];
//! `run` does all three in turn.
use super::{def, panel_ids};
use crate::ToolDef;
use emulsion_ai::jobs::Job;
use emulsion_ai::panels::{
    self, Area, MaskOutput, Models, Op, PanelInput, PanelNote, Report, Request, Target,
};
use emulsion_core::project::{PageId, ProjectEditor};
use serde_json::{Value, json};
use std::sync::Arc;

pub const TOOL: &str = "run_storyboard_ai";

pub(super) fn definitions() -> Vec<ToolDef> {
    vec![def(
        TOOL,
        "Run an AI image operation on storyboard panels (one, or a batch: the panel size never changes). operation: select_subject (the subject becomes each panel's selection), subject_mask (a layer mask on `layer`), remove_background (a cut-out layer above the source, which is hidden), upscale (a copy of the layer holding up to the model's ×2/×4 more pixels at the same size on the panel, so it stays sharp when enlarged or under a camera push-in; the source is hidden), denoise (a cleaned copy; the source is hidden), expand (shrink the picture inside the frame by `amount` on each side and fill the new border; the source is hidden), fill (paint `area`: with `prompt` through the person's image provider from Settings › Image generation, else from the surroundings with the local fill model; the result is a new layer and the original stays). `layer` names the layer to work on in each panel (case-insensitive; default the whole panel as it looks); `node` picks a layer by ID when one panel is given. Prompts are only sent to the configured provider; without one, prompted operations are refused. Local models must be installed (see list_models / download_model). Locked panels and panels that fail are listed in `failed` with the reason; the others change together as one Undo step.",
        json!({
            "panels":panel_ids(),
            "operation":{"enum":["select_subject","subject_mask","remove_background","upscale","denoise","expand","fill"]},
            "layer":{"type":"string","minLength":1,"maxLength":800,"description":"Layer name to work on in each panel. Default: the whole panel."},
            "node":{"type":"integer","minimum":1,"description":"Layer ID from describe_document, with exactly one panel."},
            "prompt":{"type":"string","maxLength":4000,"description":"expand and fill: what to paint, sent to the configured image provider. Empty or absent uses the local fill model."},
            "area":{"enum":["selection","layer","whole"],"description":"fill: each panel's selection (default), the opaque pixels of `area_layer`, or the whole frame (needs a prompt)."},
            "area_layer":{"type":"string","minLength":1,"maxLength":800,"description":"fill with area layer: the layer whose pixels mark the area, e.g. Sky."},
            "amount":{"type":"number","minimum":0.02,"maximum":0.5,"description":"expand: the new border on each side as a fraction of the frame. Default 0.15."}
        }),
        &["panels", "operation"],
    )]
}

/// The request from the tool arguments.
pub fn request(args: &Value) -> Result<Request, String> {
    let prompt = args["prompt"].as_str().map(str::to_string);
    let op = match args["operation"].as_str().unwrap_or_default() {
        "select_subject" => Op::Subject(MaskOutput::Selection),
        "subject_mask" => Op::Subject(MaskOutput::LayerMask),
        "remove_background" => Op::Subject(MaskOutput::CutOut),
        "upscale" => Op::Upscale,
        "denoise" => Op::Denoise,
        "expand" => Op::Expand {
            amount: args["amount"].as_f64().unwrap_or(0.15) as f32,
            prompt,
        },
        "fill" => Op::Fill {
            prompt,
            area: match args["area"].as_str() {
                Some("whole") => Area::Whole,
                Some("layer") => Area::Layer(
                    args["area_layer"]
                        .as_str()
                        .ok_or("area layer needs area_layer")?
                        .to_string(),
                ),
                _ => Area::Selection,
            },
        },
        other => return Err(format!("Unknown operation {other}")),
    }
    .normalized();
    let target = match (args["node"].as_u64(), args["layer"].as_str()) {
        (Some(_), Some(_)) => return Err("Use either layer or node".into()),
        (Some(node), None) => {
            if args["panels"].as_array().map_or(0, Vec::len) != 1 {
                return Err("node needs exactly one panel; use layer for several".into());
            }
            Target::Layer(node)
        }
        (None, Some(name)) => Target::Named(name.to_string()),
        (None, None) => Target::Panel,
    };
    if matches!(op, Op::Subject(MaskOutput::LayerMask)) && target == Target::Panel {
        return Err("subject_mask needs layer or node".into());
    }
    Ok(Request { op, target })
}

/// A call read from the project, ready to compute off the UI thread.
pub struct Prepared {
    pub request: Request,
    pub inputs: Vec<PanelInput>,
    pub skipped: Vec<PanelNote>,
    pub backend: Models,
}

/// What [`Prepared::compute`] produced, for [`finish`].
pub struct Computed {
    request: Request,
    inputs: Vec<PanelInput>,
    skipped: Vec<PanelNote>,
    report: Result<Report, String>,
}

/// Read the panels and settings for a call.
pub fn prepare(editor: &ProjectEditor, args: &Value) -> Result<Prepared, String> {
    let request = request(args)?;
    let ids: Vec<PageId> = super::ids(&args["panels"]);
    let (inputs, skipped) = panels::gather(editor, &ids)?;
    Ok(Prepared {
        request,
        inputs,
        skipped,
        backend: Models {
            image: crate::exec::image_config(),
        },
    })
}

impl Prepared {
    /// Run the models or provider; slow, so hosts call it on a worker.
    pub fn compute(self, job: &Arc<Job>) -> Computed {
        let report = panels::run(&self.inputs, &self.request, &self.backend, job);
        job.finish();
        Computed {
            request: self.request,
            inputs: self.inputs,
            skipped: self.skipped,
            report,
        }
    }
}

/// Commit what was computed as one Undo step and describe it.
pub fn finish(editor: &mut ProjectEditor, computed: Computed) -> Result<Value, String> {
    let report = computed.report?;
    let label = computed.request.op.label();
    let applied = panels::commit(editor, &computed.inputs, report, computed.skipped, label)?;
    let rows = |rows: &[PanelNote], key: &str| -> Vec<Value> {
        rows.iter()
            .map(|(id, name, text)| json!({"panel":id,"name":name,key:text}))
            .collect()
    };
    Ok(json!({
        "changed":rows(&applied.done, "summary"),
        "failed":rows(&applied.failed, "error"),
        "cancelled":applied.cancelled,
        "message":applied.summary(label),
    }))
}

pub(super) fn run(
    editor: &mut ProjectEditor,
    name: &str,
    args: &Value,
) -> Option<Result<Value, String>> {
    (name == TOOL).then(|| {
        let prepared = prepare(editor, args)?;
        finish(editor, prepared.compute(&Job::new()))
    })
}

#[cfg(test)]
mod tests {
    use super::super::tests::board;
    use super::*;

    #[test]
    fn arguments_become_requests() {
        let r = request(&json!({"panels":[1,2],"operation":"fill","prompt":" a dog ","area":"layer","area_layer":"Sky","layer":"BG"})).unwrap();
        assert_eq!(
            r,
            Request {
                op: Op::Fill {
                    prompt: Some("a dog".into()),
                    area: Area::Layer("Sky".into())
                },
                target: Target::Named("BG".into())
            }
        );
        let r = request(&json!({"panels":[1],"operation":"expand","prompt":"","node":4})).unwrap();
        assert_eq!(
            r.op,
            Op::Expand {
                amount: 0.15,
                prompt: None
            }
        );
        assert_eq!(r.target, Target::Layer(4));
        for bad in [
            json!({"panels":[1,2],"operation":"upscale","node":4}),
            json!({"panels":[1],"operation":"subject_mask"}),
            json!({"panels":[1],"operation":"fill","area":"layer"}),
            json!({"panels":[1],"operation":"upscale","node":4,"layer":"A"}),
        ] {
            assert!(request(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn missing_models_or_providers_change_nothing() {
        let mut editor = board();
        let panel = editor.page_list()[0].id;
        let before = editor.stamp();
        // A missing model refuses the call before any panel runs.
        let result = super::super::execute(
            &mut editor,
            TOOL,
            &json!({"panels":[panel],"operation":"upscale"}),
        );
        if emulsion_ai::upscale::available().is_none() {
            assert!(result.is_error);
            assert!(
                result.content[0]["text"]
                    .as_str()
                    .unwrap()
                    .contains("Needs")
            );
            assert!(
                result.content[0]["text"]
                    .as_str()
                    .unwrap()
                    .contains("Settings › Local models")
            );
            assert!(editor.stamp() == before);
        }
        // Locked panels are listed, not run.
        let result = super::super::execute(
            &mut editor,
            "set_storyboard_locks",
            &json!({"panels":[panel],"locked":true}),
        );
        assert!(!result.is_error, "{}", result.content[0]["text"]);
        let (inputs, skipped) = panels::gather(&editor, &[panel, 999]).unwrap();
        assert!(inputs.is_empty());
        assert_eq!(skipped.len(), 2);
    }
}
