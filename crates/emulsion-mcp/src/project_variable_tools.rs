//! Live project variable libraries preserve relay origin and grouped Undo.
use crate::{ToolDef, ToolResult};
use emulsion_core::{design_variable_project as library, project::ProjectEditor};
use serde_json::{Value, json};
pub const READ_ONLY: &[&str] = &["list_project_variables"];
pub const DESTRUCTIVE: &[&str] = &[
    "share_project_variable",
    "import_project_variable",
    "publish_project_variable",
    "rename_project_variable",
    "remove_project_variable",
    "detach_project_variable",
];
pub fn is_tool(name: &str) -> bool {
    READ_ONLY.contains(&name) || DESTRUCTIVE.contains(&name)
}
pub fn definitions() -> Vec<ToolDef> {
    let name = json!({"type":"string","minLength":1,"maxLength":80});
    [
 ("list_project_variables","Inspect page-local names, resolved values and stable project library identities.",json!({}),vec![]),
 ("share_project_variable","Share an active-page local variable with every current project page. Name collisions reject atomically; already shared variables publish only to linked pages.",json!({"name":name}),vec!["name"]),
 ("import_project_variable","Import another page's variable into the active page with a distinct local name; establish a shared identity on both pages in one Undo.",json!({"source_page":{"type":"integer","minimum":1},"name":name,"target_name":name}),vec!["source_page","name","target_name"]),
 ("publish_project_variable","Publish the active-page value to every variable alias with the same library identity. Locked consumers reject the complete operation.",json!({"name":name}),vec!["name"]),
 ("rename_project_variable","Rename matching library variables on every linked page, preserving bindings. Collisions or multiple aliases on one page reject atomically.",json!({"name":name,"new_name":name}),vec!["name","new_name"]),
 ("remove_project_variable","Remove matching library variables from all linked pages while keeping resolved object appearances. One Undo restores all bindings.",json!({"name":name}),vec!["name"]),
 ("detach_project_variable","Make only the active-page variable local; preserve its name, value and existing object bindings.",json!({"name":name}),vec!["name"]),
 ].into_iter().map(|(name,description,properties,required)|ToolDef{name:name.into(),description:description.into(),input_schema:json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})}).collect()
}
pub fn execute(project: &mut ProjectEditor, name: &str, args: &Value) -> Option<ToolResult> {
    if !is_tool(name) {
        return None;
    }
    Some(match run(project, name, args) {
        Ok(v) => ToolResult::text(v.to_string()),
        Err(e) => ToolResult::error(e),
    })
}
fn run(project: &mut ProjectEditor, name: &str, args: &Value) -> Result<Value, String> {
    let args = args.as_object().ok_or("Expected an argument object.")?;
    let allowed: &[&str] = match name {
        "list_project_variables" => &[],
        "import_project_variable" => &["source_page", "name", "target_name"],
        "rename_project_variable" => &["name", "new_name"],
        _ => &["name"],
    };
    if args.keys().any(|k| !allowed.contains(&k.as_str())) {
        return Err("Unknown project variable argument.".into());
    }
    let string = |key| {
        args.get(key)
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{key} must be a string."))
    };
    match name {
        "list_project_variables" => {}
        "share_project_variable" => {
            library::share(project, string("name")?)?;
        }
        "import_project_variable" => library::import(
            project,
            args.get("source_page")
                .and_then(Value::as_u64)
                .filter(|v| *v > 0)
                .ok_or("source_page must be a positive page ID.")?,
            string("name")?,
            string("target_name")?,
        )?,
        "publish_project_variable" => {
            library::publish(project, string("name")?)?;
        }
        "rename_project_variable" => {
            library::rename(project, string("name")?, string("new_name")?)?
        }
        "remove_project_variable" => library::remove(project, string("name")?)?,
        _ => library::detach(project, string("name")?)?,
    }
    Ok(
        json!({"active_page":project.active_page(),"pages":project.page_list().iter().map(|p|{let d=&project.page(p.id).unwrap().doc.design;json!({"page":p.id,"name":p.name,"variables":d.variables,"libraries":d.variable_libraries})}).collect::<Vec<_>>()}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::{Document, design_variables as variables, project::ProjectKind};
    #[test]
    fn project_variable_tools_are_strict_and_grouped() {
        let mut p =
            ProjectEditor::new_project(ProjectKind::Design, Document::new(100, 100)).unwrap();
        let a = p.active_page();
        variables::set(&mut p, "Gap", variables::Value::Number(20.)).unwrap();
        p.add_page(Document::new(100, 100), "B".into(), 0.).unwrap();
        let result = execute(
            &mut p,
            "import_project_variable",
            &json!({"source_page":a,"name":"Gap","target_name":"Space"}),
        )
        .unwrap();
        assert!(!result.is_error, "{:?}", result.content);
        let before = p.doc.clone();
        assert!(
            execute(
                &mut p,
                "publish_project_variable",
                &json!({"name":"Space","unexpected":true})
            )
            .unwrap()
            .is_error
        );
        assert_eq!(p.doc, before);
        assert!(
            !execute(&mut p, "list_project_variables", &json!({}))
                .unwrap()
                .is_error
        );
        p.undo();
        assert!(p.doc.design.variables.is_empty());
        assert!(p.page(a).unwrap().doc.design.variable_libraries.is_empty());
    }
}
