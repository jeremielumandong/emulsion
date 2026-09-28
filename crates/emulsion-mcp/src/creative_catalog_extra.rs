//! Folder and portable font mutations share the catalog revision transaction.
use super::*;
pub(super) const NAMES: &[&str] = &[
    "set_creative_asset_folder",
    "move_creative_asset",
    "remove_creative_asset_folder",
    "import_brand_font",
    "remove_brand_font",
];
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Folder {
    expected_revision: u64,
    id: Option<u64>,
    name: String,
    parent: Option<u64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Move {
    expected_revision: u64,
    id: u64,
    folder: Option<u64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FontImport {
    expected_revision: u64,
    brand_id: u64,
    path: PathBuf,
    #[serde(default)]
    set_default: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FontRemove {
    expected_revision: u64,
    brand_id: u64,
    alias: String,
}
pub(super) fn run(root: &Path, name: &str, args: &Value) -> Result<Value, String> {
    match name {
        "set_creative_asset_folder" => {
            let a: Folder = parse(args)?;
            edit(root, a.expected_revision, |c| {
                c.set_asset_folder(a.id, a.name, a.parent)
            })
        }
        "move_creative_asset" => {
            let a: Move = parse(args)?;
            if args.get("folder").is_none() {
                return Err("Provide folder ID or null for unfiled.".into());
            }
            edit(root, a.expected_revision, |c| {
                c.move_creative_asset(a.id, a.folder)?;
                Ok(a.id)
            })
        }
        "remove_creative_asset_folder" => {
            let a: Remove = parse(args)?;
            edit(root, a.expected_revision, |c| {
                c.remove_asset_folder(a.id)?;
                Ok(a.id)
            })
        }
        "import_brand_font" => {
            let a: FontImport = parse(args)?;
            let font = crate::design_brand_tools::load_font(&a.path)?;
            edit(root, a.expected_revision, |c| {
                let brand = c
                    .brands
                    .iter_mut()
                    .find(|b| b.id == a.brand_id)
                    .ok_or_else(|| error("Brand kit no longer exists."))?;
                let alias = font.alias().to_owned();
                if a.set_default {
                    brand.font = alias.clone();
                }
                brand.fonts.insert(alias.clone(), font);
                Ok(alias)
            })
        }
        "remove_brand_font" => {
            let a: FontRemove = parse(args)?;
            edit(root, a.expected_revision, |c| {
                let brand = c
                    .brands
                    .iter_mut()
                    .find(|b| b.id == a.brand_id)
                    .ok_or_else(|| error("Brand kit no longer exists."))?;
                if brand.font == a.alias || brand.typography.values().any(|r| r.font == a.alias) {
                    return Err(error(
                        "Change the kit's default font and typography roles before removing this font.",
                    ));
                }
                if brand.fonts.remove(&a.alias).is_none() {
                    return Err(error("Embedded brand font no longer exists."));
                }
                Ok(a.alias)
            })
        }
        _ => Err("Unknown creative portability tool".into()),
    }
}
pub(super) fn definitions() -> Vec<ToolDef> {
    let id = json!({"type":"integer","minimum":1});
    let revision = json!({"type":"integer","minimum":0});
    let parent = json!({"type":["integer","null"],"minimum":1});
    let def = |name: &str, description: &str, properties: Value, required: &[&str]| ToolDef {
        name: name.into(),
        description: description.into(),
        input_schema: json!({"type":"object","additionalProperties":false,"properties":properties,"required":required}),
    };
    vec![
        def(
            NAMES[0],
            "Create or rename/move a nested creative asset folder. Omit id to create; parent null is top level. Cycles, invalid references and depth beyond 32 are rejected atomically.",
            json!({"expected_revision":revision,"id":id,"name":{"type":"string","minLength":1,"maxLength":200},"parent":parent}),
            &["expected_revision", "name"],
        ),
        def(
            NAMES[1],
            "Move an asset reference into a folder or null for unfiled. Never moves its source file or placed copies.",
            json!({"expected_revision":revision,"id":id,"folder":parent}),
            &["expected_revision", "id", "folder"],
        ),
        def(
            NAMES[2],
            "Remove a folder while moving its direct assets and child folders into its parent. Source files are preserved.",
            json!({"expected_revision":revision,"id":id}),
            &["expected_revision", "id"],
        ),
        def(
            NAMES[3],
            "Embed a local TTF/OTF into a brand kit using a private content alias. Up to 8 MiB/file, no downloads or OS font installation. Native editable-embedding permissions are checked. Optional set_default uses it as the kit font.",
            json!({"expected_revision":revision,"brand_id":id,"path":{"type":"string","minLength":1},"set_default":{"type":"boolean","default":false}}),
            &["expected_revision", "brand_id", "path"],
        ),
        def(
            NAMES[4],
            "Remove an unused embedded font from a brand kit. Rejects fonts still used by its default typography or named roles. Existing documents retain their own embedded copies.",
            json!({"expected_revision":revision,"brand_id":id,"alias":{"type":"string","minLength":1}}),
            &["expected_revision", "brand_id", "alias"],
        ),
    ]
}
