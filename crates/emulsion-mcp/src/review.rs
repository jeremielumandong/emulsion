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
            "instruction": "Inspect the supplied images against the user's brief and references. Check subject identity and required features, anatomy/proportions and perspective where relevant to the requested style, overlaps, composition intent, and medium character. Preserve deliberate abstraction, flattened space, invented proportions and symmetry when requested; judge custom and hybrid styles against their stated traits. Cite visible evidence and document coordinates; label uncertain judgments unproven. If needed request a detail get_view region. Do not claim these checks were completed by the metric analyzer. Correct only brief-relevant problems at the current stage within the playbook's remaining iteration budget."}
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
        assert_eq!(
            critique(&doc, &json!({"include_images": false}))
                .unwrap()
                .content
                .len(),
            1
        );
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
