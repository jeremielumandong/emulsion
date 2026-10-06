//! The Stage: the camera frame's guides and the board palette (read by
//! describe_storyboard, set by set_storyboard_settings), and outside pictures
//! (PSD, ORA, PNG, JPEG…) brought in as new panels or as layers on a panel.
use super::{def, insertion, panel_id, placement};
use crate::ToolDef;
use emulsion_core::command::Slot;
use emulsion_core::fragment::Fragment;
use emulsion_core::project::{PageId, ProjectEditor};
use emulsion_core::storyboard::{Frame, StageGuides, Storyboard};
use emulsion_core::storyboard_naming::fit_to_frame;
use emulsion_core::storyboard_stage::{DEFAULT_PALETTE, MAX_PALETTE};
use emulsion_core::{Document, Editor, Node, NodeId};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::path::Path;

/// Most files one import may read.
const MAX_FILES: usize = 50;
const EXTENSIONS: [&str; 9] = [
    "psd", "psb", "ora", "png", "jpg", "jpeg", "webp", "tif", "tiff",
];

/// `guides` for set_storyboard_settings.
pub(super) fn guide_fields() -> Value {
    let percent = json!({"type":"number","minimum":0,"maximum":100,"description":"Percentage of the camera frame; 0 hides it."});
    json!({
        "type":"object",
        "additionalProperties":false,
        "description":"Stage guides drawn over the camera frame. Omitted values stay.",
        "properties":{
            "action_safe":percent,
            "title_safe":percent,
            "field_guide":{"type":"boolean","description":"Show the field guide."},
            "fields":{"type":"integer","minimum":2,"maximum":24,"description":"Fields in the field guide (the frame is the outermost)."},
            "overscan":{"type":"number","minimum":0,"maximum":StageGuides::MAX_OVERSCAN,"description":"Space shown around the frame for art that runs outside the shot, as a percentage of the frame on each side."}
        }
    })
}

/// `palette` for set_storyboard_settings.
pub(super) fn palette_fields() -> Value {
    let colors = |min: usize, description: &str| json!({"type":"array","items":{"type":"string","maxLength":7},"minItems":min,"maxItems":MAX_PALETTE,"description":description});
    json!({
        "type":"object",
        "additionalProperties":false,
        "description":"Change the board's colour palette (#RRGGBB). Applied in order: reset or set, remove, add.",
        "properties":{
            "reset":{"type":"boolean","description":"Go back to the default storyboard palette."},
            "set":colors(0, "Replace the whole palette."),
            "remove":colors(1, "Colours to remove; each must be in the palette."),
            "add":colors(1, "Colours to append; ones already there are skipped.")
        }
    })
}

fn hex(color: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", color[0], color[1], color[2])
}

fn parse_hex(value: &Value) -> Result<[u8; 3], String> {
    let text = value.as_str().unwrap_or_default();
    let digits = text
        .strip_prefix('#')
        .filter(|d| d.len() == 6 && d.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| format!("Palette colours are #RRGGBB, not '{text}'."))?;
    let channel = |i: usize| u8::from_str_radix(&digits[i * 2..i * 2 + 2], 16).unwrap();
    Ok([channel(0), channel(1), channel(2)])
}

fn colors(spec: &Value, key: &str) -> Result<Vec<[u8; 3]>, String> {
    spec[key]
        .as_array()
        .into_iter()
        .flatten()
        .map(parse_hex)
        .collect()
}

/// Apply `guides` and `palette` from set_storyboard_settings; the board
/// validates the result.
pub(super) fn apply_settings(board: &mut Storyboard, args: &Value) -> Result<(), String> {
    if let Some(changes) = args["guides"].as_object() {
        let mut guides = serde_json::to_value(&board.stage).map_err(|e| e.to_string())?;
        for (key, value) in changes {
            guides[key] = value.clone();
        }
        board.stage = serde_json::from_value(guides).map_err(|e| e.to_string())?;
    }
    let spec = &args["palette"];
    if !spec.is_object() {
        return Ok(());
    }
    let palette = &mut board.palette;
    match (spec["reset"] == true, spec.get("set")) {
        (true, Some(_)) => return Err("Use either reset or set for the palette.".into()),
        (true, None) => *palette = DEFAULT_PALETTE.to_vec(),
        (false, Some(_)) => {
            palette.clear();
            for color in colors(spec, "set")? {
                if !palette.contains(&color) {
                    palette.push(color);
                }
            }
        }
        (false, None) => {}
    }
    for color in colors(spec, "remove")? {
        let at = palette
            .iter()
            .position(|c| *c == color)
            .ok_or_else(|| format!("{} is not in the palette.", hex(color)))?;
        palette.remove(at);
    }
    for color in colors(spec, "add")? {
        if !palette.contains(&color) {
            palette.push(color);
        }
    }
    Ok(())
}

/// A rectangle in panel pixels, to a hundredth of a pixel.
fn rect(frame: Frame) -> Value {
    let round = |v: f64| (v * 100.).round() / 100.;
    json!({"x":round(frame.x),"y":round(frame.y),"width":round(frame.w),"height":round(frame.h)})
}

/// The guides with their rectangles in panel pixels, for describe_storyboard.
pub(super) fn guides_json(board: &Storyboard) -> Value {
    let (width, height) = (board.settings.width, board.settings.height);
    let guides = &board.stage;
    let safe = |percent: f64| (percent > 0.).then(|| rect(Frame::centred(width, height, percent)));
    let mut out = serde_json::to_value(guides).unwrap_or_default();
    out["action_safe_rect"] = safe(guides.action_safe).into();
    out["title_safe_rect"] = safe(guides.title_safe).into();
    out["field_rects"] = guides
        .field_rects(width, height)
        .into_iter()
        .map(rect)
        .collect();
    out["stage_area"] = rect(guides.stage_area(width, height));
    out
}

pub(super) fn palette_json(board: &Storyboard) -> Value {
    board.palette.iter().copied().map(hex).collect()
}

pub(super) fn definitions() -> Vec<ToolDef> {
    let mut fields = placement();
    fields["paths"] = json!({"type":"array","items":{"type":"string","maxLength":4096},"minItems":1,"maxItems":MAX_FILES,"description":"Absolute paths of PSD, PSB, ORA, PNG, JPEG, WebP or TIFF files."});
    fields["into"] = json!({"enum":["panels","layers"],"description":"panels (default): one new panel per file; layers: every file's layers on top of `panel`."});
    fields["panel"] = panel_id();
    vec![def(
        "import_storyboard_files",
        "Bring outside pictures into the storyboard: layouts, reference art or finished drawings. into \"panels\" (default) adds one panel per file, named after it, after a panel (default: the active panel) or at_start, in that panel's scene, with the default duration. into \"layers\" places every file's layers on top of `panel` (default: the active panel) and selects it. Pictures at another size or aspect are cropped to the centre and scaled to the panel resolution. Supported layers stay editable; PSD and ORA files preserve supported groups, opacity, masks, blend modes and clipping. Unsupported PSD compositing can use saved flattened appearance, disclosed in warnings and source_imports. Returns each new panel's or the placed layers with their blend mode and clipping. One Undo step.",
        fields,
        &["paths"],
    )]
}

/// Run a stage tool; `None` when `name` is not one.
pub(super) fn run(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    name: &str,
    args: &Value,
) -> Option<Result<Value, String>> {
    match name {
        "import_storyboard_files" => Some(import_files(editor, board, args)),
        _ => None,
    }
}

struct ReadFiles {
    documents: Vec<(String, Document)>,
    source_imports: Vec<Value>,
    warnings: Vec<String>,
}

/// Every file read once, with its import evidence, before anything changes.
fn read_files(args: &Value) -> Result<ReadFiles, String> {
    let mut out = ReadFiles {
        documents: Vec::new(),
        source_imports: Vec::new(),
        warnings: Vec::new(),
    };
    for text in args["paths"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
    {
        let path = Path::new(text);
        if !path.is_absolute() {
            return Err(format!("Use an absolute path, not '{text}'."));
        }
        let extension = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if !EXTENSIONS.contains(&extension.as_str()) {
            return Err(format!(
                "'{text}' is not a PSD, PSB, ORA, PNG, JPEG, WebP or TIFF file."
            ));
        }
        let (opened, report) =
            emulsion_io::open_full_with_report(path).map_err(|e| format!("{text}: {e}"))?;
        if let Some(error) = opened.history_error {
            out.warnings
                .push(format!("{text}: History could not be restored: {error}"));
        }
        if let Some(report) = report {
            out.source_imports.push(
                json!({"path":text,"report":crate::workspace_tools::psd_report_value(report)}),
            );
            if report.profile_decision == emulsion_io::psd::ImportProfileDecision::SavedAppearance {
                out.warnings.push(format!("{text}: Imported the PSD's saved appearance as a flattened layer; some layer or compositing settings could not be preserved as editable content."));
            }
        }
        let doc = opened.doc;
        let name = path
            .file_stem()
            .map_or_else(|| "Import".into(), |s| s.to_string_lossy().into_owned());
        out.documents.push((name, doc));
    }
    Ok(out)
}

/// Several pictures as one document at the panel size, later files on top,
/// so placing them is one Undo step.
fn stack(documents: Vec<(String, Document)>, width: u32, height: u32) -> Result<Document, String> {
    let fit = |doc: &Document| {
        fit_to_frame(doc, width, height)
            .map_err(|error| format!("Could not fit imported picture to the panel: {error}"))
    };
    if documents.len() == 1 {
        return fit(&documents[0].1);
    }
    let mut sheet = Editor::new(Document::new(width, height), None);
    for (_, doc) in documents {
        let doc = fit(&doc)?;
        let roots: Vec<_> = doc
            .nodes
            .iter()
            .filter(|n| n.parent.is_none())
            .map(|n| n.id)
            .collect();
        if !roots.is_empty() {
            Fragment::capture(&doc, &roots)?.paste(&mut sheet, Slot::TOP, (0., 0.))?;
        }
    }
    Ok(sheet.doc)
}

/// Prepare every input before selecting the destination panel.
fn place_imported_layers(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    target: PageId,
    documents: Vec<(String, Document)>,
) -> Result<Vec<NodeId>, String> {
    let doc = stack(documents, board.settings.width, board.settings.height)?;
    let previous = editor.active_page();
    editor.set_active_page(target)?;
    match editor.place_layers(&doc) {
        Ok(placed) => Ok(placed),
        Err(error) => match editor.set_active_page(previous) {
            Ok(()) => Err(error),
            Err(restore_error) => Err(format!(
                "{error} Could not restore the previously active panel {previous}: {restore_error}"
            )),
        },
    }
}

/// Layers with their blend mode, opacity, clipping and mask, in stack order.
fn layers_json(doc: &Document, keep: impl Fn(&Node) -> bool) -> Vec<Value> {
    doc.nodes
        .iter()
        .filter(|n| keep(n))
        .map(|n| {
            let mut layer = json!({
                "id":n.id,
                "name":n.name,
                "blend":n.blend.label(),
                "opacity":(n.opacity * 100.).round(),
            });
            if let Some(parent) = n.parent {
                layer["parent"] = parent.into();
            }
            if let Some(base) = n.clip_to {
                layer["clipped_to"] = base.into();
            }
            if n.has_mask() {
                layer["mask"] = true.into();
            }
            layer
        })
        .collect()
}

fn import_files(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    args: &Value,
) -> Result<Value, String> {
    let into_layers = args["into"] == "layers";
    if into_layers && (args.get("after").is_some() || args.get("at_start").is_some()) {
        return Err("after and at_start place new panels; give panel with into \"layers\".".into());
    }
    if !into_layers && args.get("panel").is_some() {
        return Err(
            "panel is for into \"layers\"; new panels go after a panel or at_start.".into(),
        );
    }
    let ReadFiles {
        documents,
        source_imports,
        warnings,
    } = read_files(args)?;
    if !into_layers {
        let after = insertion(args, editor.active_page())?;
        let ids = editor.import_panels(after, documents)?;
        let panels: Vec<_> = ids
            .iter()
            .map(|id| {
                let name = editor.page_list().iter().find(|m| m.id == *id).unwrap();
                let doc = &editor.page(*id).unwrap().doc;
                json!({"panel":id,"name":name.name,"layers":layers_json(doc, |_| true)})
            })
            .collect();
        return Ok(
            json!({"panels":panels,"active_panel":editor.active_page(),"source_imports":source_imports,"warnings":warnings}),
        );
    }
    let target = args["panel"].as_u64().unwrap_or(editor.active_page());
    if !board.panels.contains_key(&target) {
        return Err(format!("Panel {target} does not exist."));
    }
    let placed = place_imported_layers(editor, board, target, documents)?;
    let included: HashSet<_> = placed
        .iter()
        .flat_map(|id| editor.doc.subtree(*id))
        .collect();
    Ok(json!({
        "panel":target,
        "layers":layers_json(&editor.doc, |n| included.contains(&n.id)),
        "active_panel":editor.active_page(),
        "source_imports":source_imports,
        "warnings":warnings,
    }))
}

#[cfg(test)]
mod import_report_tests {
    use super::*;

    #[test]
    fn storyboard_file_reads_preserve_reports_for_each_psd_without_inference() {
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../emulsion-io/tests/fixtures/psd/blending/knockout-deep-nested-pt.psd");
        let (expected, report) = emulsion_io::open_full_with_report(&fixture).unwrap();
        let loaded = read_files(&json!({"paths":[fixture, fixture]})).unwrap();
        assert_eq!(loaded.documents.len(), 2);
        assert_eq!(loaded.source_imports.len(), 2);
        for ((_, document), evidence) in loaded.documents.iter().zip(&loaded.source_imports) {
            crate::document_contents::assert_document_contents(
                document,
                &expected.doc,
                "storyboard source import",
            );
            assert_eq!(evidence["path"], json!(fixture));
            assert_eq!(
                evidence["report"],
                crate::workspace_tools::psd_report_value(report.unwrap())
            );
        }
        let fallback = report.unwrap().profile_decision
            == emulsion_io::psd::ImportProfileDecision::SavedAppearance;
        assert_eq!(loaded.warnings.len(), if fallback { 2 } else { 0 });
    }
}

#[cfg(test)]
mod placement_admission_tests {
    use super::*;
    use emulsion_core::project::ProjectKind;
    use emulsion_core::{Command, NodeKind, SmartPlacement};
    use emulsion_raster::projective::Projective2;
    use emulsion_raster::{Placement, Raster};
    use std::sync::Arc;

    fn projected_with_effect() -> Document {
        let mut doc = Document::new(29_994, 1);
        let mut node = Node::smart(
            1,
            "Imported Smart",
            Arc::new(Raster::solid(2, 2, [1.; 4])),
            vec![],
            Placement::default(),
        );
        let NodeKind::Smart { placement, .. } = &mut node.kind else {
            unreachable!()
        };
        *placement = SmartPlacement::Projective(Projective2::IDENTITY);
        node.styles
            .push(emulsion_core::styles::LayerStyle::DropShadow {
                color: [0; 3],
                opacity: 100.,
                angle: 0.,
                distance: 0.,
                size: 1.,
            });
        doc.nodes.push(node);
        doc.next_id = 2;
        doc.validate().unwrap();
        doc
    }

    #[test]
    fn rejected_single_and_multiple_imports_preserve_destination_and_redo() {
        let mut editor =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(29_995, 1)).unwrap();
        let previous = editor.active_page();
        let target = editor
            .add_page(Document::new(29_995, 1), "Destination".into(), 0.)
            .unwrap();
        editor.set_active_page(previous).unwrap();
        editor
            .execute(Command::AddNode {
                node: Box::new(Node::new(
                    0,
                    "Redo target",
                    NodeKind::Fill { rgba: [255; 4] },
                )),
                slot: Slot::TOP,
            })
            .unwrap();
        assert!(editor.undo());
        let board = editor.storyboard().unwrap().clone();
        let before = editor.snapshot().unwrap();
        let stamp = editor.stamp();
        let history = editor.history.len();
        for documents in [
            vec![("Must refuse".into(), projected_with_effect())],
            vec![
                ("Fits first".into(), Document::new(4, 4)),
                ("Must refuse".into(), projected_with_effect()),
            ],
        ] {
            // This is the same boundary called by import_files after decoding.
            let error = place_imported_layers(&mut editor, &board, target, documents).unwrap_err();
            assert!(
                error.starts_with("Could not fit imported picture"),
                "{error}"
            );
            assert!(error.contains("padded effect canvas"), "{error}");
            assert_eq!(editor.active_page(), previous);
            assert_eq!(editor.stamp(), stamp);
            assert_eq!(editor.history.len(), history);
            assert!(editor.can_redo());
            let after = editor.snapshot().unwrap();
            assert_eq!(after.next_page_id, before.next_page_id);
            assert_eq!(after.pages.len(), before.pages.len());
            for (old, new) in before.pages.iter().zip(&after.pages) {
                assert_eq!(new.meta, old.meta);
                assert_eq!(new.doc, old.doc);
            }
        }
        assert!(editor.redo());
        assert_eq!(editor.doc.nodes[0].name, "Redo target");
    }

    #[test]
    fn single_import_is_fitted_before_placement_and_empty_input_restores_page() {
        let mut doc = Document::new(4, 2);
        let pixels = Arc::new(Raster::solid(4, 2, [1.; 4]));
        doc.nodes.push(Node::raster(
            1,
            "Picture",
            pixels.clone(),
            Placement::default(),
        ));
        doc.next_id = 2;
        let fitted = stack(vec![("Picture".into(), doc.clone())], 8, 4).unwrap();
        assert_eq!((fitted.width, fitted.height), (8, 4));
        let NodeKind::Raster {
            raster, placement, ..
        } = &fitted.nodes[0].kind
        else {
            unreachable!()
        };
        assert!(Arc::ptr_eq(raster, &pixels));
        assert_eq!(placement.scale_x, 2.);
        let mut editor =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(8, 4)).unwrap();
        let previous = editor.active_page();
        let target = editor
            .add_page(Document::new(8, 4), "Destination".into(), 0.)
            .unwrap();
        editor.set_active_page(previous).unwrap();
        let board = editor.storyboard().unwrap().clone();
        let stamp = editor.stamp();
        let error = place_imported_layers(
            &mut editor,
            &board,
            target,
            vec![("Empty".into(), Document::new(4, 2))],
        )
        .unwrap_err();
        assert!(error.contains("no layers to place"), "{error}");
        assert_eq!(editor.active_page(), previous);
        assert_eq!(editor.stamp(), stamp);
        let target_before = editor.page(target).unwrap().doc.clone();
        let history = editor.page(target).unwrap().history.len();
        let placed =
            place_imported_layers(&mut editor, &board, target, vec![("Picture".into(), doc)])
                .unwrap();
        assert_eq!(placed.len(), 1);
        assert_eq!(editor.active_page(), target);
        assert_eq!(editor.doc.node(placed[0]).unwrap().name, "Picture");
        assert_eq!(editor.history.len(), history + 1);
        assert!(editor.undo());
        assert_eq!(editor.doc, target_before);
    }
}
