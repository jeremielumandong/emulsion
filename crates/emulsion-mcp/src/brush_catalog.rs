//! Document-independent, revision-checked brush authoring.
use crate::server::{ToolDef, ToolResult};
use emulsion_io::brush_library::{self as store, Catalog};
use emulsion_raster::paint::Brush;
use serde_json::{Value, json};
use std::path::Path;

pub(crate) const NAMES: &[&str] = &[
    "describe_brush_library",
    "manage_brush_library",
    "edit_brush",
    "brush_memories",
];
pub(crate) const READ_ONLY: &[&str] = &["describe_brush_library"];
type Result<T> = std::result::Result<T, String>;
fn fields(v: &Value, allowed: &[&str]) -> Result<()> {
    let o = v.as_object().ok_or("arguments must be an object")?;
    for k in o.keys() {
        if !allowed.contains(&k.as_str()) {
            return Err(format!("unknown field: {k}"));
        }
    }
    Ok(())
}
fn string<'a>(v: &'a Value, k: &str) -> Result<&'a str> {
    v.get(k)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{k} must be a string"))
}
fn index(v: &Value, k: &str) -> Result<usize> {
    v.get(k)
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| format!("{k} must be a nonnegative integer"))
}
fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
fn variant(op: &str, extras: &[(&str, &str)], optional: &[&str]) -> Value {
    let mut p = json!({"op":{"const":op}});
    let mut required = vec!["op"];
    for (key, kind) in extras {
        p[*key] = json!({"type":kind});
        if !optional.contains(key) {
            required.push(key);
        }
    }
    object(p, &required)
}
pub(crate) fn definitions() -> Vec<ToolDef> {
    let mut ops = Vec::new();
    for (op, keys) in [
        ("create_library", vec![("name", "string")]),
        (
            "create_set",
            vec![("library_id", "string"), ("name", "string")],
        ),
        (
            "create_brush",
            vec![
                ("set_id", "string"),
                ("name", "string"),
                ("settings", "object"),
            ],
        ),
        ("rename_library", vec![("id", "string"), ("name", "string")]),
        ("rename_set", vec![("id", "string"), ("name", "string")]),
        ("rename_brush", vec![("id", "string"), ("name", "string")]),
        ("duplicate_library", vec![("id", "string")]),
        (
            "duplicate_set",
            vec![("id", "string"), ("library_id", "string")],
        ),
        (
            "duplicate_brush",
            vec![("id", "string"), ("set_id", "string")],
        ),
        ("delete_library", vec![("id", "string")]),
        ("delete_set", vec![("id", "string")]),
        ("delete_brush", vec![("id", "string")]),
        ("move_library", vec![("id", "string"), ("index", "integer")]),
        (
            "move_set",
            vec![
                ("id", "string"),
                ("library_id", "string"),
                ("index", "integer"),
            ],
        ),
        (
            "move_brush",
            vec![("id", "string"), ("set_id", "string"), ("index", "integer")],
        ),
        ("pin", vec![("id", "string"), ("pinned", "boolean")]),
        ("record_use", vec![("id", "string")]),
        (
            "combine",
            vec![("primary_id", "string"), ("secondary_id", "string")],
        ),
        ("uncombine", vec![("id", "string")]),
    ] {
        ops.push(variant(op, &keys, &["settings"]));
    }
    let rev = json!({"type":"integer","minimum":0,"description":"Revision returned by describe_brush_library; stale writes fail without changing the catalog."});
    vec![
        ToolDef{name:NAMES[0].into(),description:"Read the persistent brush catalog, ordered libraries/sets, pin/recent lists and per-tool memories. Definitions include current/original/reset settings and durable source references. Does not change document history. Definitions are paginated (default 12, max 200); filter with brush_id, set_id, library_id or query.".into(),input_schema:object(json!({"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":200},"brush_id":{"type":"string"},"set_id":{"type":"string"},"library_id":{"type":"string"},"query":{"type":"string"}}),&[])},
        ToolDef{name:NAMES[1].into(),description:"Atomically manage brush libraries, sets and brushes. All operations succeed together or none persist. Uses stable IDs; returned results contain newly created IDs. Index is zero-based within the destination. Built-in protection follows the library UI. Combine/uncombine preserve originals. Create settings use the complete Brush JSON shape from discovery; omitted fields use defaults. No document history entry.".into(),input_schema:object(json!({"expected_revision":rev,"operations":{"type":"array","minItems":1,"maxItems":100,"items":{"oneOf":ops}}}),&["expected_revision","operations"])},
        ToolDef{name:NAMES[2].into(),description:"Persist Brush Studio edits. patch recursively updates only supplied settings; use complete Brush JSON names from discovery. Source runtime IDs cannot be patched: use brush source tools. Unknown fields fail; numeric settings are clamped to engine limits. Component selects primary/secondary; secondary must already exist. Metadata and combine mode are optional. action=update (default), create_reset_point, reset, restore_original. Reset actions accept no patch/metadata. Original baseline and original assets remain intact.".into(),input_schema:object(json!({"expected_revision":rev,"brush_id":{"type":"string"},"action":{"enum":["update","create_reset_point","reset","restore_original"]},"component":{"enum":["primary","secondary"]},"patch":{"type":"object"},"name":{"type":"string"},"note":{"type":"string"},"author":{"type":"object","properties":{"name":{"type":"string"},"website":{"type":"string"},"copyright":{"type":"string"},"source":{"type":"string"}},"additionalProperties":false},"combine_mode":{"enum":["Normal","Multiply","Screen"]}}),&["expected_revision","brush_id"])},
        ToolDef{name:NAMES[3].into(),description:"Inspect or persist per-brush, per-tool size/opacity memories and four marks. Tools: paint, smudge, erase, heal, clone, mask. inspect needs no revision. save/save_mark optionally accept size and opacity; otherwise use remembered/current definition values. recall_mark restores size/opacity to that tool memory; clear_mark removes a mark; clear removes memory; transfer copies remembered size/opacity into target_tool without changing the document or selected UI tool. mark indices are 0..3. All writes require expected_revision.".into(),input_schema:object(json!({"action":{"enum":["inspect","save","save_mark","recall_mark","clear_mark","clear","transfer"]},"expected_revision":rev,"brush_id":{"type":"string"},"tool":{"enum":["paint","smudge","erase","heal","clone","mask"]},"target_tool":{"enum":["paint","smudge","erase","heal","clone","mask"]},"index":{"type":"integer","minimum":0,"maximum":3},"size":{"type":"number","minimum":1,"maximum":1000},"opacity":{"type":"number","minimum":0.01,"maximum":1}}),&["action","brush_id","tool"])},
    ]
}

pub(crate) fn execute(name: &str, args: &Value) -> ToolResult {
    execute_at(&store::root(), name, args)
}
fn execute_at(root: &Path, name: &str, args: &Value) -> ToolResult {
    match run(root, name, args) {
        Ok(v) => ToolResult::text(v.to_string()),
        Err(e) => ToolResult::error(e),
    }
}
fn run(root: &Path, name: &str, args: &Value) -> Result<Value> {
    let report = store::load_from_with_report(root).map_err(|e| e.to_string())?;
    let mut catalog = report.catalog;
    if name == NAMES[0] {
        fields(
            args,
            &[
                "offset",
                "limit",
                "brush_id",
                "set_id",
                "library_id",
                "query",
            ],
        )?;
        for key in ["brush_id", "set_id", "library_id", "query"] {
            if args.get(key).is_some() {
                string(args, key)?;
            }
        }
        let offset = if args.get("offset").is_some() {
            index(args, "offset")?
        } else {
            0
        };
        let limit = if args.get("limit").is_some() {
            index(args, "limit")?
        } else {
            12
        };
        if !(1..=200).contains(&limit) {
            return Err("limit must be 1..200".into());
        }
        let query = args
            .get("query")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_lowercase();
        let rows: Vec<_> = catalog
            .brushes
            .iter()
            .filter(|b| {
                args.get("brush_id").is_none_or(|id| id == &b.id)
                    && args.get("set_id").is_none_or(|id| id == &b.set_id)
                    && args.get("library_id").is_none_or(|id| {
                        catalog
                            .sets
                            .iter()
                            .any(|s| s.id == b.set_id && id == &s.library_id)
                    })
                    && (query.is_empty()
                        || b.name.to_lowercase().contains(&query)
                        || b.note.to_lowercase().contains(&query))
            })
            .collect();
        let page: Vec<_> = rows.iter().skip(offset).take(limit).collect();
        return Ok(
            json!({"revision":catalog.revision,"schema_version":catalog.schema_version,"libraries":catalog.libraries,"sets":catalog.sets,"pinned":catalog.pinned,"recent":catalog.recent,"tool_memories":catalog.tool_memories,"brushes":page,"total":rows.len(),"offset":offset,"next_offset":(offset.saturating_add(page.len())<rows.len()).then(||offset+page.len()),"warnings":report.warnings}),
        );
    }
    if name == NAMES[3] && args.get("action").and_then(Value::as_str) == Some("inspect") {
        fields(args, &["action", "brush_id", "tool"])?;
        let id = string(args, "brush_id")?;
        let tool = tool(args, "tool")?;
        if catalog.brush(id).is_none() {
            return Err(format!("brush {id} was not found"));
        }
        return Ok(json!({"revision":catalog.revision,"memory":catalog.tool_memory(tool,id)}));
    }
    let revision = args
        .get("expected_revision")
        .and_then(Value::as_u64)
        .ok_or("expected_revision must be a nonnegative integer")?;
    if revision != catalog.revision {
        return Err(format!(
            "Brush library revision conflict: expected {revision}, current {}. Reload and retry.",
            catalog.revision
        ));
    }
    let result = match name {
        "manage_brush_library" => {
            fields(args, &["expected_revision", "operations"])?;
            let ops = args
                .get("operations")
                .and_then(Value::as_array)
                .ok_or("operations must be an array")?;
            if ops.is_empty() || ops.len() > 100 {
                return Err("operations must contain 1..100 entries".into());
            }
            let mut results = Vec::new();
            for op in ops {
                results.push(manage(&mut catalog, op)?);
            }
            json!({"results":results})
        }
        "edit_brush" => edit(&mut catalog, args)?,
        "brush_memories" => memory(&mut catalog, args)?,
        _ => return Err(format!("unknown brush catalog tool {name}")),
    };
    let saved = store::commit_to(root, revision, &catalog).map_err(|e| e.to_string())?;
    Ok(
        json!({"revision":saved.revision,"result":result,"warnings":report.warnings,"document_changed":false}),
    )
}
fn manage(c: &mut Catalog, v: &Value) -> Result<Value> {
    let op = string(v, "op")?;
    let allowed: &[&str] = match op {
        "create_library" => &["op", "name"],
        "create_set" => &["op", "name", "library_id"],
        "create_brush" => &["op", "name", "set_id", "settings"],
        "rename_library" | "rename_set" | "rename_brush" => &["op", "id", "name"],
        "duplicate_library" | "delete_library" | "delete_set" | "delete_brush" | "record_use"
        | "uncombine" => &["op", "id"],
        "duplicate_set" => &["op", "id", "library_id"],
        "duplicate_brush" => &["op", "id", "set_id"],
        "move_library" => &["op", "id", "index"],
        "move_set" => &["op", "id", "library_id", "index"],
        "move_brush" => &["op", "id", "set_id", "index"],
        "pin" => &["op", "id", "pinned"],
        "combine" => &["op", "primary_id", "secondary_id"],
        _ => return Err(format!("unknown operation {op}")),
    };
    fields(v, allowed)?;
    let s = |key| string(v, key);
    let result: std::result::Result<Value, store::Error> = match op {
        "create_library" => c.create_library(s("name")?).map(|id| json!({"id":id})),
        "create_set" => c
            .create_set(s("library_id")?, s("name")?)
            .map(|id| json!({"id":id})),
        "create_brush" => {
            let brush = if let Some(p) = v.get("settings") {
                patch_brush(Brush::default(), p)?
            } else {
                Brush::default()
            };
            c.add_brush(s("set_id")?, s("name")?, brush)
                .map(|id| json!({"id":id}))
        }
        "rename_library" => c.rename_library(s("id")?, s("name")?).map(|_| json!(null)),
        "rename_set" => c.rename_set(s("id")?, s("name")?).map(|_| json!(null)),
        "rename_brush" => c.rename_brush(s("id")?, s("name")?).map(|_| json!(null)),
        "duplicate_library" => c.duplicate_library(s("id")?).map(|id| json!({"id":id})),
        "duplicate_set" => c
            .duplicate_set(s("id")?, s("library_id")?)
            .map(|id| json!({"id":id})),
        "duplicate_brush" => c
            .duplicate_brush(s("id")?, s("set_id")?)
            .map(|id| json!({"id":id})),
        "delete_library" => c.delete_library(s("id")?).map(|_| json!(null)),
        "delete_set" => c.delete_set(s("id")?).map(|_| json!(null)),
        "delete_brush" => c.delete_brush(s("id")?).map(|_| json!(null)),
        "move_library" => c
            .reorder_library(s("id")?, index(v, "index")?)
            .map(|_| json!(null)),
        "move_set" => c
            .move_set(s("id")?, s("library_id")?, index(v, "index")?)
            .map(|_| json!(null)),
        "move_brush" => c
            .move_brush(s("id")?, s("set_id")?, index(v, "index")?)
            .map(|_| json!(null)),
        "pin" => c
            .pin(
                s("id")?,
                v.get("pinned")
                    .and_then(Value::as_bool)
                    .ok_or("pinned must be a boolean")?,
            )
            .map(|_| json!(null)),
        "record_use" => c.record_use(s("id")?).map(|_| json!(null)),
        "combine" => c
            .combine_brushes(s("primary_id")?, s("secondary_id")?)
            .map(|id| json!({"id":id})),
        "uncombine" => c
            .uncombine_brush(s("id")?)
            .map(|(a, b)| json!({"primary_id":a,"secondary_id":b})),
        _ => unreachable!(),
    };
    result
        .map(|r| json!({"op":op,"result":r}))
        .map_err(|e| e.to_string())
}
fn merge_known(base: &mut Value, patch: &Value, path: &str) -> Result<()> {
    if let Some(obj) = base.as_object_mut() {
        let updates = patch
            .as_object()
            .ok_or_else(|| format!("{path} must be an object"))?;
        for (key, value) in updates {
            let target = obj
                .get_mut(key)
                .ok_or_else(|| format!("unknown setting {path}.{key}"))?;
            merge_known(target, value, &format!("{path}.{key}"))?;
        }
    } else {
        *base = patch.clone();
    }
    Ok(())
}
fn patch_brush(brush: Brush, patch: &Value) -> Result<Brush> {
    let obj = patch.as_object().ok_or("patch must be an object")?;
    if obj.contains_key("tip") || obj.contains_key("grain_tex") {
        return Err(
            "Source runtime IDs cannot be edited as settings; use brush source tools".into(),
        );
    }
    crate::exec::apply_brush_settings(brush, patch).map_err(|e| {
        e.content
            .first()
            .and_then(|v| v["text"].as_str())
            .unwrap_or("invalid brush settings")
            .to_string()
    })
}

fn edit(c: &mut Catalog, v: &Value) -> Result<Value> {
    fields(
        v,
        &[
            "expected_revision",
            "brush_id",
            "action",
            "component",
            "patch",
            "name",
            "note",
            "author",
            "combine_mode",
        ],
    )?;
    let id = string(v, "brush_id")?;
    let action = if v.get("action").is_some() {
        string(v, "action")?
    } else {
        "update"
    };
    if action != "update" {
        fields(v, &["expected_revision", "brush_id", "action"])?;
        match action {
            "create_reset_point" => c.create_reset_point(id),
            "reset" => c.reset_brush(id),
            "restore_original" => c.restore_original(id),
            _ => return Err(format!("unknown action {action}")),
        }
        .map_err(|e| e.to_string())?;
    } else {
        let component = if v.get("component").is_some() {
            string(v, "component")?
        } else {
            "primary"
        };
        if !["primary", "secondary"].contains(&component) {
            return Err("component must be primary or secondary".into());
        }
        if let Some(name) = v.get("name") {
            c.rename_brush(id, name.as_str().ok_or("name must be a string")?)
                .map_err(|e| e.to_string())?;
        }
        let b = c
            .brush_mut(id)
            .ok_or_else(|| format!("brush {id} was not found"))?;
        if component == "secondary" && b.secondary.is_none() {
            return Err("brush has no secondary component; combine brushes first".into());
        }
        if let Some(patch) = v.get("patch") {
            if component == "primary" {
                b.brush = patch_brush(b.brush, patch)?;
            } else {
                b.secondary = Some(patch_brush(b.secondary.expect("checked"), patch)?);
            }
        }
        if v.get("note").is_some() {
            b.note = string(v, "note")?.into();
        }
        if let Some(author) = v.get("author") {
            fields(author, &["name", "website", "copyright", "source"])?;
            let mut value = serde_json::to_value(&b.author).map_err(|e| e.to_string())?;
            merge_known(&mut value, author, "author")?;
            b.author = serde_json::from_value(value).map_err(|e| e.to_string())?;
        }
        if let Some(mode) = v.get("combine_mode") {
            if b.secondary.is_none() {
                return Err("combine_mode requires a dual brush".into());
            }
            b.combine_mode = serde_json::from_value(mode.clone()).map_err(|e| e.to_string())?;
        }
        if !["patch", "name", "note", "author", "combine_mode"]
            .iter()
            .any(|k| v.get(k).is_some())
        {
            return Err("update requires settings or metadata".into());
        }
    }
    Ok(json!({"brush":c.brush(id)}))
}
fn tool<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    let t = string(v, key)?;
    if !["paint", "smudge", "erase", "heal", "clone", "mask"].contains(&t) {
        return Err(format!("unsupported {key}: {t}"));
    }
    Ok(t)
}
fn memory(c: &mut Catalog, v: &Value) -> Result<Value> {
    let action = string(v, "action")?;
    let extra: &[&str] = match action {
        "save" => &["size", "opacity"],
        "save_mark" => &["size", "opacity", "index"],
        "recall_mark" | "clear_mark" => &["index"],
        "clear" => &[],
        "transfer" => &["target_tool"],
        _ => return Err(format!("unknown memory action {action}")),
    };
    let mut allowed = vec!["action", "expected_revision", "brush_id", "tool"];
    allowed.extend_from_slice(extra);
    fields(v, &allowed)?;
    let id = string(v, "brush_id")?;
    let tool = tool(v, "tool")?;
    let definition = c
        .brush(id)
        .ok_or_else(|| format!("brush {id} was not found"))?;
    let mut brush = definition.brush;
    if let Some(m) = c.tool_memory(tool, id) {
        brush.size = m.brush.size;
        brush.opacity = m.brush.opacity;
    }
    for key in ["size", "opacity"] {
        if let Some(value) = v.get(key) {
            let n = value
                .as_f64()
                .ok_or_else(|| format!("{key} must be a number"))? as f32;
            if key == "size" {
                brush.size = n;
            } else {
                brush.opacity = n;
            }
        }
    }
    if brush != brush.sanitized() {
        return Err("memory size/opacity are outside supported ranges".into());
    }
    let result = match action {
        "save" => c.remember_tool(tool, id, brush),
        "save_mark" => c.save_mark(tool, id, index(v, "index")?, brush),
        "clear_mark" => c.remove_mark(tool, id, index(v, "index")?),
        "recall_mark" => {
            let i = index(v, "index")?;
            let mark = c
                .tool_memory(tool, id)
                .and_then(|m| m.marks.get(i))
                .copied()
                .flatten()
                .ok_or("mark was not found (valid indices 0..3)")?;
            brush.size = mark.size;
            brush.opacity = mark.opacity;
            c.remember_tool(tool, id, brush)
        }
        "clear" => {
            c.tool_memories.remove(&format!("{tool}/{id}"));
            Ok(())
        }
        "transfer" => c.remember_tool(self::tool(v, "target_tool")?, id, brush),
        _ => unreachable!(),
    };
    result.map_err(|e| e.to_string())?;
    let target = if action == "transfer" {
        string(v, "target_tool")?
    } else {
        tool
    };
    Ok(json!({"tool":target,"brush_id":id,"memory":c.tool_memory(target,id)}))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root() -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "emulsion-mcp-catalog-{}",
            store::new_id("test").replace(':', "-")
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }
    fn call(p: &Path, name: &str, v: Value) -> Value {
        let r = execute_at(p, name, &v);
        assert!(!r.is_error, "{:?}", r);
        serde_json::from_str(r.content[0]["text"].as_str().unwrap()).unwrap()
    }
    #[test]
    fn transaction_failure_and_stale_revision_leave_disk_unchanged() {
        let p = root();
        let before = store::load_from(&p).unwrap();
        let bad = json!({"expected_revision":0,"operations":[{"op":"create_library","name":"temporary"},{"op":"delete_brush","id":"missing"}]});
        assert!(execute_at(&p, NAMES[1], &bad).is_error);
        assert_eq!(before, store::load_from(&p).unwrap());
        call(
            &p,
            NAMES[1],
            json!({"expected_revision":0,"operations":[{"op":"create_library","name":"kept"}]}),
        );
        assert!(execute_at(&p,NAMES[1],&json!({"expected_revision":0,"operations":[{"op":"create_library","name":"stale"}]})).is_error);
        assert_eq!(store::load_from(&p).unwrap().revision, 1);
        std::fs::remove_dir_all(p).unwrap();
    }
    #[test]
    fn nested_edits_reset_and_original_are_independent() {
        let p = root();
        let c = store::load_from(&p).unwrap();
        let id = c.brushes[0].id.clone();
        let original = c.brush(&id).unwrap().brush;
        call(
            &p,
            NAMES[2],
            json!({"expected_revision":0,"brush_id":id,"patch":{"advanced":{"shape":{"count":3}}}}),
        );
        call(
            &p,
            NAMES[2],
            json!({"expected_revision":1,"brush_id":id,"action":"create_reset_point"}),
        );
        call(
            &p,
            NAMES[2],
            json!({"expected_revision":2,"brush_id":id,"patch":{"advanced":{"shape":{"count":7}}}}),
        );
        call(
            &p,
            NAMES[2],
            json!({"expected_revision":3,"brush_id":id,"action":"reset"}),
        );
        assert_eq!(
            store::load_from(&p)
                .unwrap()
                .brush(&id)
                .unwrap()
                .brush
                .advanced
                .shape
                .count,
            3
        );
        call(
            &p,
            NAMES[2],
            json!({"expected_revision":4,"brush_id":id,"action":"restore_original"}),
        );
        assert_eq!(
            store::load_from(&p).unwrap().brush(&id).unwrap().brush,
            original
        );
        assert!(
            execute_at(
                &p,
                NAMES[2],
                &json!({"expected_revision":5,"brush_id":id,"patch":{"advanced":{"imaginary":3}}})
            )
            .is_error
        );
        assert!(
            execute_at(
                &p,
                NAMES[2],
                &json!({"expected_revision":5,"brush_id":id,"patch":{"tip":123}})
            )
            .is_error
        );
        std::fs::remove_dir_all(p).unwrap();
    }
    #[test]
    fn hierarchy_dual_edits_and_pagination_preserve_originals() {
        let p = root();
        let make = |op| json!({"expected_revision":store::load_from(&p).unwrap().revision,"operations":[op]});
        let response = call(
            &p,
            NAMES[1],
            make(json!({"op":"create_library","name":"Test"})),
        );
        let library = response["result"]["results"][0]["result"]["id"]
            .as_str()
            .unwrap();
        let response = call(
            &p,
            NAMES[1],
            make(json!({"op":"create_set","library_id":library,"name":"Set"})),
        );
        let set = response["result"]["results"][0]["result"]["id"]
            .as_str()
            .unwrap();
        let response = call(
            &p,
            NAMES[1],
            make(json!({"op":"create_brush","set_id":set,"name":"One","settings":{"size":90}})),
        );
        let first = response["result"]["results"][0]["result"]["id"]
            .as_str()
            .unwrap();
        let response = call(
            &p,
            NAMES[1],
            make(json!({"op":"duplicate_brush","id":first,"set_id":set})),
        );
        let second = response["result"]["results"][0]["result"]["id"]
            .as_str()
            .unwrap();
        let response = call(
            &p,
            NAMES[1],
            make(json!({"op":"combine","primary_id":first,"secondary_id":second})),
        );
        let dual = response["result"]["results"][0]["result"]["id"]
            .as_str()
            .unwrap();
        let revision = store::load_from(&p).unwrap().revision;
        call(
            &p,
            NAMES[2],
            json!({"expected_revision":revision,"brush_id":dual,"component":"secondary","patch":{"size":33},"combine_mode":"Multiply"}),
        );
        let c = store::load_from(&p).unwrap();
        assert_eq!(c.brush(dual).unwrap().secondary.unwrap().size, 33.);
        assert_eq!(c.brush(first).unwrap().brush.size, 90.);
        assert_eq!(c.brush(second).unwrap().brush.size, 90.);
        let page = call(&p, NAMES[0], json!({"library_id":library,"limit":1}));
        assert_eq!(page["total"], 3);
        assert_eq!(page["next_offset"], 1);
        call(&p, NAMES[1], make(json!({"op":"uncombine","id":dual})));
        let c = store::load_from(&p).unwrap();
        assert!(c.brush(dual).unwrap().secondary.is_some());
        let saved = c.clone();
        assert!(
            execute_at(
                &p,
                NAMES[1],
                &make(json!({"op":"pin","id":first,"pinned":true,"typo":0}))
            )
            .is_error
        );
        assert_eq!(store::load_from(&p).unwrap(), saved);
        std::fs::remove_dir_all(p).unwrap();
    }
    #[test]
    fn marks_transfer_and_strict_arguments() {
        let p = root();
        let id = store::load_from(&p).unwrap().brushes[0].id.clone();
        call(
            &p,
            NAMES[3],
            json!({"expected_revision":0,"action":"save_mark","brush_id":id,"tool":"paint","index":2,"size":85,"opacity":0.4}),
        );
        call(
            &p,
            NAMES[3],
            json!({"expected_revision":1,"action":"save","brush_id":id,"tool":"paint","size":15}),
        );
        call(
            &p,
            NAMES[3],
            json!({"expected_revision":2,"action":"recall_mark","brush_id":id,"tool":"paint","index":2}),
        );
        call(
            &p,
            NAMES[3],
            json!({"expected_revision":3,"action":"transfer","brush_id":id,"tool":"paint","target_tool":"smudge"}),
        );
        let c = store::load_from(&p).unwrap();
        assert_eq!(c.tool_memory("smudge", &id).unwrap().brush.size, 85.);
        assert!(
            execute_at(
                &p,
                NAMES[3],
                &json!({"action":"inspect","brush_id":id,"tool":"paint","size":20})
            )
            .is_error
        );
        assert_eq!(c, store::load_from(&p).unwrap());
        std::fs::remove_dir_all(p).unwrap();
    }
}
