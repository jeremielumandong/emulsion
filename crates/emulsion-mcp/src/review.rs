//! Metrics and images for review by the calling assistant; no hidden vision model.
use crate::{preview, server::ToolResult};
use emulsion_ai::critique::{CritiqueContext, REVIEW_POLICY, analyze_with_context, rank_with_jev};
use emulsion_core::Document;
use serde_json::{Value, json};

pub(crate) fn critique(doc: &Document, args: &Value) -> Result<ToolResult, ToolResult> {
    let context: CritiqueContext = match args.get("context") {
        None => CritiqueContext::default(),
        Some(v) => serde_json::from_value(v.clone())
            .map_err(|e| ToolResult::error(format!("invalid critique context: {e}")))?,
    };
    let include_images = match args.get("include_images") {
        None => true,
        Some(v) => v
            .as_bool()
            .ok_or_else(|| ToolResult::error("include_images must be boolean"))?,
    };
    let count = args
        .get("count")
        .and_then(Value::as_u64)
        .unwrap_or(3)
        .clamp(1, 8) as usize;
    let mut images = Vec::new();
    if include_images {
        images.extend(preview::view(doc, &json!({}))?.content);
        if let Some(region) = args.get("region") {
            images.extend(preview::view(doc, &json!({"region": region}))?.content);
        }
    } else if args.get("region").is_some() {
        return Err(ToolResult::error("region requires include_images=true"));
    }
    let mut c = analyze_with_context(doc, &context);
    if let Ok(k) = std::env::var("TYPESAFE_API_KEY")
        && !k.is_empty()
    {
        let _ = rank_with_jev(&emulsion_ai::jev::Jev::new(k), &mut c);
    }
    let observations: Vec<_> = c
        .issues
        .iter()
        .take(count)
        .map(|i| json!({"key": i.key, "priority": i.severity, "note": i.text}))
        .collect();
    let mut result = ToolResult::text(json!({
        "context": c.context, "ranked_by": c.ranked_by, "review_policy": REVIEW_POLICY,
        "observations": observations,
        "issues": c.issues.iter().take(count).map(|i| json!({"key": i.key, "severity": i.severity, "note": i.text})).collect::<Vec<_>>(),
        "legacy_fields": "issues/severity are compatibility aliases for observations/review priority, not defects or quality scores",
        "metrics": c.metrics,
        "visual_review": {"status": "pending_assistant_review", "images_included": include_images,
            "evidence_available": if include_images { "current_composition_and_optional_detail" } else { "metrics_only" },
            "instruction": "Inspect current images against the user's brief and available references before writing a visual critique. If images are omitted, get a current get_view first; if a feature is too small, request a detail get_view region. Read the whole composition first, then inspect the highest-risk relationship at detail scale. Check subject identity and required features, gesture, silhouette, anatomy/proportions and perspective where relevant to the requested style, contact and overlap order, focal hierarchy, and medium character. For interacting figures or objects, trace which form connects to which and what is in front at the contact point. Preserve deliberate abstraction, flattened space, invented proportions and symmetry when requested; judge custom and hybrid styles against their stated traits. Separate intended style from an accidental loss of readability. Confirm suspected seams or guide lines in document images before treating screen overlays as artwork. Cite visible evidence and document coordinates using the returned image mapping; label uncertain judgments unproven and resolve them with a crop when useful. Do not claim these checks were completed by the metric analyzer. Correct only brief-relevant problems at the current stage within the playbook's remaining iteration budget.",
            "response_contract": {
                "purpose": "Guidance for the calling assistant's review; these are not generated findings or completed checks.",
                "preserve": "Name the effective visual choices that repairs must preserve.",
                "priorities": "Report up to three image-supported problems in impact order, with structure and action readability before surface polish. Fewer or no problems are valid; do not fill a quota.",
                "finding_fields": {
                    "priority": "blocking, important or polish; no numeric quality score",
                    "location": "Named feature and document-space [x, y, width, height] when supported by the image mapping; do not invent precise coordinates",
                    "evidence": "Describe the visible relationship and why it conflicts with the intended result; distinguish observation from inference",
                    "repair": "Specify the smallest concrete redraw, placement, overlap, value or edge change that addresses the cause while preserving the intended style",
                    "recheck": "State what should read clearly in a fresh full view and relevant detail crop after the repair"
                },
                "repair_loop": "When editing is requested, repair the highest-impact problem first, then inspect fresh full and detail views against its recheck criterion. Do not hide unresolved construction with hatching, effects or added detail. If the repair fails, revise the underlying shapes instead of repeating cosmetic edits. If only critique is requested, return the repair plan without modifying the drawing.",
                "completion": "Report which checks passed, what remains unresolved or unproven, and whether the current stage is ready to advance. Do not infer completion from tool success, metrics or exhausted iteration budget."
            }}
    }).to_string());
    result.content.extend(images);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::Editor;

    #[test]
    fn tool_returns_intent_and_composition_and_detail_images() {
        let mut editor = Editor::new(Document::new(200, 100), None);
        let context = json!({"medium": "manga", "style": "", "stage": "gesture", "composition_intent": "centred symmetric portrait", "user_constraints": ["keep symmetry"]});
        let result = crate::exec::execute(
            &mut editor,
            "critique",
            &json!({"context": context, "region": [40,20,80,60]}),
        );
        assert!(!result.is_error, "{result:?}");
        assert_eq!(
            result
                .content
                .iter()
                .filter(|b| b["type"] == "image")
                .count(),
            2
        );
        let meta: Value =
            serde_json::from_str(result.content[0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(meta["context"], context);
        assert_eq!(meta["visual_review"]["status"], "pending_assistant_review");
        assert_eq!(
            meta["visual_review"]["evidence_available"],
            "current_composition_and_optional_detail"
        );
        assert!(meta["visual_review"].get("findings").is_none());
        let mapping: Value =
            serde_json::from_str(result.content[4]["text"].as_str().unwrap()).unwrap();
        assert_eq!(mapping["region"], json!([40, 20, 80, 60]));
    }

    #[test]
    fn tool_rejects_malformed_context_and_preserves_metrics_only_option() {
        let doc = Document::new(20, 20);
        assert!(critique(&doc, &json!({"context": {"stage": 7}})).is_err());
        assert!(critique(&doc, &json!({"context": {"style": ["cubist"]}})).is_err());
        assert!(
            critique(
                &doc,
                &json!({"include_images": false, "region": [0,0,10,10]})
            )
            .is_err()
        );
        let result = critique(&doc, &json!({"include_images": false})).unwrap();
        assert_eq!(result.content.len(), 1);
        let meta: Value =
            serde_json::from_str(result.content[0]["text"].as_str().unwrap()).unwrap();
        let review = &meta["visual_review"];
        assert_eq!(review["status"], "pending_assistant_review");
        assert_eq!(review["images_included"], false);
        assert_eq!(review["evidence_available"], "metrics_only");
        assert!(
            review["instruction"]
                .as_str()
                .unwrap()
                .contains("get_view first")
        );
        assert!(review.get("findings").is_none());
        let contract = &review["response_contract"];
        for field in ["priority", "location", "evidence", "repair", "recheck"] {
            assert!(contract["finding_fields"][field].is_string());
        }
        // The new assistant guidance leaves existing observation clients intact.
        for (observation, legacy) in meta["observations"]
            .as_array()
            .unwrap()
            .iter()
            .zip(meta["issues"].as_array().unwrap())
        {
            assert_eq!(observation["key"], legacy["key"]);
            assert_eq!(observation["note"], legacy["note"]);
            assert_eq!(observation["priority"], legacy["severity"]);
        }
    }

    #[test]
    fn mcp_json_rpc_carries_images_mapping_and_context() {
        struct Host(Editor);
        impl crate::server::ToolHost for Host {
            fn tools(&self) -> Vec<crate::server::ToolDef> {
                crate::tools::definitions()
            }
            fn call(&mut self, name: &str, args: &Value) -> ToolResult {
                crate::exec::execute(&mut self.0, name, args)
            }
        }
        let context = json!({
            "medium": "digital ink",
            "style": "My invented moon-map × cut-paper cubism / naïve pixel-folk hybrid",
            "stage": "develop",
            "composition_intent": "centred, mirror-symmetric emblem",
            "user_constraints": ["Keep the deliberately flat values", "Preserve symmetry", "Use flattened space and invented proportions"]
        });
        let calls = [
            json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get_view","arguments":{"region":[10,20,30,40]}}}),
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"critique","arguments":{"context":context,"count":8}}}),
            json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"list_brushes","arguments":{"query":"Maru"}}}),
            json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"critique","arguments":{"context":{"medium":"watercolour","composition_intent":"centred"}}}}),
        ];
        let input = calls
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        let mut out = Vec::new();
        let mut doc = Document::new(100, 100);
        emulsion_core::command::Command::AddNode {
            node: Box::new(emulsion_core::Node::raster(
                0,
                "Intentional flat symmetric field",
                std::sync::Arc::new(emulsion_raster::Raster::solid(
                    100,
                    100,
                    [0.4, 0.4, 0.4, 1.0],
                )),
                emulsion_raster::Placement::default(),
            )),
            slot: emulsion_core::command::Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap();
        crate::server::serve(Host(Editor::new(doc, None)), input.as_bytes(), &mut out).unwrap();
        let responses: Vec<Value> = String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert!(
            responses[0]["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|t| t["name"] == "get_view"
                    && t["inputSchema"]["properties"]["region"].is_object())
        );
        let schema = &responses[0]["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "critique")
            .unwrap()["inputSchema"]["properties"]["context"];
        assert_eq!(schema["properties"]["style"]["type"], "string");
        assert!(schema["properties"]["style"].get("enum").is_none());
        assert!(schema.get("required").is_none());
        for response in &responses[1..] {
            assert_eq!(response["result"]["isError"], false);
            assert!(
                response["result"]["content"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|b| b["type"] == "image")
            );
        }
        let mapping: Value = serde_json::from_str(
            responses[1]["result"]["content"][1]["text"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(mapping["region"], json!([10, 20, 30, 40]));
        let review: Value = serde_json::from_str(
            responses[2]["result"]["content"][0]["text"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(review["context"], context);
        assert!(review["metrics"]["symmetry"].as_f64().unwrap() > 0.92);
        let observations = review["observations"].as_array().unwrap();
        assert!(
            observations
                .iter()
                .any(|o| o["key"] == "flat_values"
                    && o["note"].as_str().unwrap().contains("only if"))
        );
        assert!(observations.iter().any(|o| o["key"] == "symmetry"
            && o["note"].as_str().unwrap().contains("preserve")));
        assert!(review["review_policy"].as_str().unwrap().contains("style"));
        let instruction = review["visual_review"]["instruction"].as_str().unwrap();
        assert!(instruction.contains("style"));
        assert!(instruction.contains("flattened space"));
        let legacy: Value = serde_json::from_str(
            responses[4]["result"]["content"][0]["text"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(legacy["context"]["style"], "");
        assert_eq!(legacy["context"]["composition_intent"], "centred");
    }
}
