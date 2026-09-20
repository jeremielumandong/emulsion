//! Offline integration of the assistant's native drawing/correction workflow.
//! No model calls: exercise the same public tool entry point a provider uses.
use base64::prelude::{BASE64_STANDARD, Engine};
use emulsion_core::{Document, Editor, NodeId, NodeKind};
use emulsion_mcp::{exec, server::ToolResult};
use emulsion_raster::{Raster, composite::flatten};
use serde_json::{Value, json};
use std::sync::Arc;

fn call(editor: &mut Editor, tool: &str, args: Value) -> ToolResult {
    let result = exec::execute(editor, tool, &args);
    assert!(!result.is_error, "{tool}: {:?}", result.content);
    result
}

fn json_result(result: &ToolResult) -> Value {
    result
        .content
        .iter()
        .filter_map(|block| block["text"].as_str())
        .find_map(|text| serde_json::from_str(text).ok())
        .expect("JSON tool result")
}

fn named_node(editor: &mut Editor, name: &str) -> NodeId {
    // Resolve returned document metadata instead of assuming consecutive ids.
    let described = json_result(&call(editor, "describe_document", json!({})));
    described["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["name"] == name)
        .unwrap()["id"]
        .as_u64()
        .unwrap()
}

fn pixels(editor: &Editor, id: NodeId) -> Arc<Raster> {
    let NodeKind::Raster { raster, .. } = &editor.doc.node(id).unwrap().kind else {
        panic!("paint must remain on its own raster layer")
    };
    raster.clone()
}

fn png(result: &ToolResult) -> Vec<u8> {
    let block = result
        .content
        .iter()
        .find(|block| block["type"] == "image")
        .unwrap();
    assert_eq!(block["mimeType"], "image/png");
    BASE64_STANDARD
        .decode(block["data"].as_str().unwrap())
        .unwrap()
}

#[test]
fn editable_leaf_study_preserves_selection_and_soft_edges_during_targeted_correction() {
    let mut editor = Editor::new(Document::new(128, 96), None);
    call(
        &mut editor,
        "draw_path",
        json!({
            "name":"Paper", "d":"M 0 0 H 128 V 96 H 0 Z", "stroke":"none", "fill":"#D8C8A8"
        }),
    );
    call(
        &mut editor,
        "draw_path",
        json!({
            "name":"Leaf silhouette", "d":"M 24 68 Q 35 34 64 20 Q 95 34 104 68 Q 64 88 24 68 Z",
            "stroke":"#315A35", "width":2, "fill":"#548552"
        }),
    );
    let leaf = named_node(&mut editor, "Leaf silhouette");
    call(
        &mut editor,
        "draw_path",
        json!({
            "name":"Stem", "d":"M 64 58 Q 69 76 62 91", "stroke":"#315A35", "width":2, "fill":"none"
        }),
    );
    let stem = named_node(&mut editor, "Stem");
    call(
        &mut editor,
        "translate_node",
        json!({"node":stem,"dx":2,"dy":0}),
    );
    assert!(matches!(
        editor.doc.node(stem).unwrap().kind,
        NodeKind::Path { .. }
    ));
    call(&mut editor, "path_to_selection", json!({"node":leaf}));
    call(&mut editor, "add_layer", json!({"name":"Leaf light"}));
    let light = named_node(&mut editor, "Leaf light");
    let brushes = call(&mut editor, "list_brushes", json!({"query":"Round oil"}));
    assert!(!png(&brushes).is_empty());
    let paint = call(
        &mut editor,
        "paint",
        json!({
            "node":light, "brush":"Round oil", "color":"#95BB66",
            "settings":{"size":32,"hardness":0.1,"opacity":0.55,"flow":1,"wetness":0,"relief":0,"grain_strength":0,"taper_start":0,"taper_end":0,"stabilizer":0},
            "strokes":[{"points":[[42,57,1],[87,52,1]]}]
        }),
    );
    assert_eq!(image::load_from_memory(&png(&paint)).unwrap().width(), 128);
    // Only the workflow's own temporary selection is cleared.
    call(&mut editor, "deselect", json!({}));
    call(
        &mut editor,
        "set_path",
        json!({
            "node":leaf, "d":"M 24 68 Q 35 34 64 18 Q 95 34 104 68 Q 64 88 24 68 Z"
        }),
    );
    let editable_outline = editor.doc.node(leaf).unwrap().clone();

    // The person selects a local area for a colour correction. Subsequent
    // paint/erase/inspection/undo must retain this exact selection object.
    call(
        &mut editor,
        "select_rect",
        json!({"x":48,"y":42,"width":28,"height":20}),
    );
    let selection = editor.doc.selection.clone().unwrap();
    let described = json_result(&call(&mut editor, "describe_document", json!({})));
    assert_eq!(
        described["selection"],
        json!({"x":48,"y":42,"width":28,"height":20})
    );
    let before = pixels(&editor, light);
    assert!(
        before
            .to_pixels()
            .iter()
            .any(|p| p[3] > 0 && p[3] < u16::MAX)
    );
    call(
        &mut editor,
        "paint",
        json!({
            "node":light,"brush":"Acrylic","color":"#E4C473","alpha_lock":true,
            "settings":{"size":40,"hardness":1,"opacity":1,"flow":1,"wetness":0,"relief":0,"grain_strength":0,"taper_start":0,"taper_end":0,"stabilizer":0},
            "strokes":[{"points":[[5,54,1],[120,54,1]]}]
        }),
    );
    let corrected = pixels(&editor, light);
    let mut changed = 0;
    for y in 0..96 {
        for x in 0..128 {
            let old = before.get(x, y);
            let new = corrected.get(x, y);
            assert_eq!(new[3], old[3], "glazing changed coverage at {x},{y}");
            if selection.get(x, y) == 0 {
                assert_eq!(new, old, "paint escaped the selection");
            }
            if new != old {
                changed += 1;
            }
        }
    }
    assert!(
        changed > 30,
        "the correction must actually change the selected colour"
    );
    assert_eq!(editor.doc.node(leaf), Some(&editable_outline));
    assert!(Arc::ptr_eq(
        editor.doc.selection.as_ref().unwrap(),
        &selection
    ));

    call(
        &mut editor,
        "paint",
        json!({
            "node":light,"brush":"Hard eraser","alpha_lock":false,
            "settings":{"size":8,"hardness":1,"opacity":1,"flow":1,"taper_start":0,"taper_end":0,"stabilizer":0},
            "strokes":[{"points":[[64,48,1],[64,58,1]]}]
        }),
    );
    let erased = pixels(&editor, light);
    assert!(corrected.get(64, 54)[3] > 0);
    assert_eq!(erased.get(64, 54)[3], 0);
    for y in 0..96 {
        for x in 0..128 {
            if selection.get(x, y) == 0 {
                assert_eq!(erased.get(x, y), corrected.get(x, y));
            }
        }
    }
    // Erasing reveals the coloured leaf underneath, not invented white paper.
    let composite = flatten(&editor.doc.composite_tree(), 0).to_srgba8();
    let center = ((54 * 128 + 64) * 4) as usize;
    assert_ne!(&composite[center..center + 3], &[255, 255, 255]);
    call(
        &mut editor,
        "get_view",
        json!({"region":[44,38,36,28],"max_size":128}),
    );
    call(&mut editor, "undo", json!({}));
    assert!(
        Arc::ptr_eq(&pixels(&editor, light), &corrected),
        "one undo restores the whole eraser call"
    );
    assert!(Arc::ptr_eq(
        editor.doc.selection.as_ref().unwrap(),
        &selection
    ));
    assert_eq!(editor.doc.node(leaf), Some(&editable_outline));

    let view = call(&mut editor, "get_view", json!({"max_size":128}));
    let bytes = png(&view);
    let image = image::load_from_memory(&bytes).unwrap().to_rgba8();
    assert_eq!(image.dimensions(), (128, 96));
    assert_eq!(image.get_pixel(8, 8).0, [216, 200, 168, 255]);
    assert_ne!(image.get_pixel(64, 54), image.get_pixel(8, 8));
    if std::env::var_os("EMULSION_DRAWING_WORKFLOW_ARTIFACT").is_some() {
        let path = std::env::temp_dir().join("emulsion-drawing-workflow.png");
        std::fs::write(&path, bytes).unwrap();
        eprintln!("Drawing workflow preview: {}", path.display());
    }

    let native = std::env::temp_dir().join(format!(
        "emulsion-drawing-workflow-{}.ora",
        std::process::id()
    ));
    call(
        &mut editor,
        "save_document",
        json!({"path":native.to_str().unwrap()}),
    );
    let reopened = emulsion_io::open(&native).unwrap();
    assert!(matches!(
        reopened.node(leaf).unwrap().kind,
        NodeKind::Path { .. }
    ));
    assert!(matches!(
        reopened.node(light).unwrap().kind,
        NodeKind::Raster { .. }
    ));
    assert_eq!(reopened.nodes.len(), 4);
    // Default native source depth is eight bits; allow its quantization while
    // checking that the edited layers reconstruct the same visible picture.
    let expected = flatten(&editor.doc.composite_tree(), 0).to_srgba8();
    let restored = flatten(&reopened.composite_tree(), 0).to_srgba8();
    assert!(
        expected
            .iter()
            .zip(restored)
            .all(|(&a, b)| a.abs_diff(b) <= 2)
    );
    std::fs::remove_file(native).unwrap();
}
