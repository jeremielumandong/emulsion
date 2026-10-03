//! Line mileage: the stroke length drawn on each panel and the project.
use super::def;
use crate::ToolDef;
use emulsion_core::project::ProjectEditor;
use emulsion_core::project::mileage::Mileage;
use serde_json::{Value, json};

pub(super) fn definitions() -> Vec<ToolDef> {
    vec![def(
        "describe_storyboard_mileage",
        "Read-only. Line mileage: the total length of the strokes drawn on each panel (brush, eraser, vector lines and shapes) and on the whole storyboard, in panel pixels, millimetres and a readable label (m/cm/mm at the panel's resolution), with a football-pitch comparison once over a metre. Mileage counts ink on the page: undoing a stroke takes it off and redoing it puts it back. The person can reset it in the panel inspector.",
        json!({}),
        &[],
    )]
}

fn mileage_json(m: Mileage) -> Value {
    json!({
        "pixels":(m.px * 10.).round() / 10.,
        "mm":(m.mm * 10.).round() / 10.,
        "label":m.label(),
        "comparison":m.comparison(),
    })
}

pub(super) fn run(editor: &ProjectEditor, name: &str) -> Option<Result<Value, String>> {
    if name != "describe_storyboard_mileage" {
        return None;
    }
    let panels: Vec<Value> = editor
        .page_list()
        .iter()
        .map(|m| {
            let mut entry = mileage_json(editor.panel_mileage(m.id));
            entry["panel"] = json!(m.id);
            entry["name"] = json!(m.name);
            entry
        })
        .collect();
    Some(Ok(json!({
        "project":mileage_json(editor.project_mileage()),
        "panels":panels,
    })))
}

#[cfg(test)]
mod tests {
    use super::super::tests::{board, call};
    use serde_json::json;

    #[test]
    fn mileage_reads_per_panel_and_project() {
        let mut e = board();
        e.record_ink(1, 72. * 100.);
        let out = call(&mut e, "describe_storyboard_mileage", json!({}));
        assert_eq!(out["project"]["pixels"], 7200.0);
        assert_eq!(out["project"]["label"], "2.5 m");
        assert_eq!(out["panels"][0]["panel"], 1);
        assert_eq!(out["panels"][0]["mm"], 2540.0);
        assert!(
            out["project"]["comparison"]
                .as_str()
                .unwrap()
                .contains("football")
        );
    }
}
