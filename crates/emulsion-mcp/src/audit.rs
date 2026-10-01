//! Reproducible inventory and registration/schema checks; never executes tools.
use serde_json::{Value, json};
use std::collections::HashSet;

pub fn requires_workspace(name: &str) -> bool {
    crate::library_tools::is_tool(name)
        || crate::workspace_tools::NAMES.contains(&name)
        || crate::project_tools::is_tool(name)
        || crate::diagram_project_tools::is_tool(name)
        || crate::storyboard_tools::is_tool(name)
        || crate::project_variable_tools::is_tool(name)
        || crate::editor_host_tools::is_tool(name)
        || crate::smart_source_tools::is_tool(name)
        || crate::print_tools::is_tool(name)
        || crate::design_motion_tools::HOST_TOOLS.contains(&name)
        || matches!(
            name,
            "list_raw_documents"
                | "set_raw_comparison"
                | "synchronize_raw"
                | "attach_reference_folder"
                | "get_reference_attachments"
                | "get_reference_image"
        )
}

fn check_schema(schema: &Value, path: &str, issues: &mut Vec<String>) {
    if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
        if let Some(required) = schema.get("required").and_then(Value::as_array) {
            let mut seen = HashSet::new();
            for key in required {
                if !key
                    .as_str()
                    .is_some_and(|k| properties.contains_key(k) && seen.insert(k))
                {
                    issues.push(format!("{path}: invalid/duplicate required property {key}"));
                }
            }
        }
        for (key, child) in properties {
            check_schema(child, &format!("{path}.{key}"), issues);
        }
    }
    for key in ["items", "additionalProperties"] {
        if let Some(child) = schema.get(key).filter(|v| v.is_object()) {
            check_schema(child, &format!("{path}.{key}"), issues);
        }
    }
    for key in ["oneOf", "anyOf", "allOf", "prefixItems"] {
        if let Some(children) = schema.get(key).and_then(Value::as_array) {
            for (i, child) in children.iter().enumerate() {
                check_schema(child, &format!("{path}.{key}[{i}]"), issues);
            }
        }
    }
    for (min, max) in [
        ("minimum", "maximum"),
        ("minItems", "maxItems"),
        ("minLength", "maxLength"),
    ] {
        if let (Some(a), Some(b)) = (schema[min].as_f64(), schema[max].as_f64())
            && a > b
        {
            issues.push(format!("{path}: {min} exceeds {max}"));
        }
    }
    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        let mut seen = HashSet::new();
        if values.is_empty() || values.iter().any(|v| !seen.insert(v.to_string())) {
            issues.push(format!("{path}: empty/duplicate enum values"));
        }
    }
}

pub fn report() -> Value {
    let definitions = crate::tools::definitions();
    let mut names = HashSet::new();
    let mut issues = Vec::new();
    let mut tools = Vec::new();
    for definition in definitions {
        let name = &definition.name;
        if !names.insert(name.clone()) {
            issues.push(format!("Duplicate tool: {name}"));
        }
        if definition.description.trim().is_empty() || definition.input_schema["type"] != "object" {
            issues.push(format!("{name}: missing description or object schema"));
        }
        check_schema(&definition.input_schema, name, &mut issues);
        let read_only = crate::tools::is_read_only(name);
        let destructive = crate::tools::is_destructive(name);
        if read_only && destructive {
            issues.push(format!("{name}: conflicting approval classification"));
        }
        tools.push(json!({"name":name,"description":definition.description,
            "input_schema":definition.input_schema,"read_only":read_only,
            "requires_confirmation":destructive,
            "route":if requires_workspace(name) {"live_workspace"} else {"document_or_catalog"}}));
    }
    for name in crate::tools::read_only_names()
        .chain(crate::tools::HEAVY.iter().copied())
        .chain(crate::tools::destructive_names())
        .chain(crate::design_motion_tools::HOST_TOOLS.iter().copied())
    {
        if !names.contains(name) {
            issues.push(format!("Unregistered classified tool: {name}"));
        }
    }
    tools.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    json!({"tool_count":tools.len(),"issues":issues,"tools":tools})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_registered_tool_has_a_coherent_schema_and_policy() {
        let report = report();
        assert_eq!(report["issues"], json!([]), "{}", report["issues"]);
        println!("{} MCP tools audited", report["tool_count"]);
    }

    #[test]
    fn every_workspace_tool_explains_its_host_requirement_offline() {
        for tool in crate::tools::definitions()
            .into_iter()
            .filter(|t| requires_workspace(&t.name))
        {
            let mut editor = emulsion_core::Editor::new(emulsion_core::Document::new(16, 16), None);
            let result = crate::exec::execute(&mut editor, &tool.name, &json!({}));
            let text = result
                .content
                .iter()
                .filter_map(|v| v["text"].as_str())
                .collect::<String>();
            assert!(result.is_error, "{}: {text}", tool.name);
            assert!(
                !text.to_lowercase().contains("unknown tool"),
                "{}: {text}",
                tool.name
            );
            assert!(
                text.contains("host") || text.contains("relay"),
                "{}: {text}",
                tool.name
            );
        }
    }
}
