//! Native creative catalog management. Writes reload under the catalog OS lock.
#[path = "creative_catalog_extra.rs"]
mod extra;
use crate::{ToolResult, server::ToolDef};
use emulsion_io::creative_library::{self as library, AssetKind, Catalog};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
pub const NAMES: &[&str] = &[
    "set_creative_asset_folder",
    "move_creative_asset",
    "remove_creative_asset_folder",
    "import_brand_font",
    "remove_brand_font",
    "get_creative_catalog",
    "register_creative_asset",
    "update_creative_asset",
    "remove_creative_asset",
    "set_brand_kit",
    "remove_brand_kit",
    "set_asset_collection",
    "remove_asset_collection",
    "import_brand_kit",
    "export_brand_kit",
    "install_design_template",
];
pub const READ_ONLY: &[&str] = &["get_creative_catalog"];
pub const DESTRUCTIVE: &[&str] = &[
    "set_creative_asset_folder",
    "move_creative_asset",
    "remove_creative_asset_folder",
    "import_brand_font",
    "remove_brand_font",
    "register_creative_asset",
    "update_creative_asset",
    "remove_creative_asset",
    "set_brand_kit",
    "remove_brand_kit",
    "set_asset_collection",
    "remove_asset_collection",
    "import_brand_kit",
    "export_brand_kit",
    "install_design_template",
];
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Get {
    #[serde(default)]
    offset: usize,
    #[serde(default = "limit")]
    limit: usize,
}
fn limit() -> usize {
    100
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Register {
    expected_revision: u64,
    path: PathBuf,
    kind: AssetKind,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AssetPatch {
    expected_revision: u64,
    id: u64,
    name: Option<String>,
    tags: Option<Vec<String>>,
    attribution: Option<String>,
    license: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Remove {
    expected_revision: u64,
    id: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Brand {
    typography: Option<
        std::collections::BTreeMap<String, emulsion_core::design_brand_assets::TypographyRole>,
    >,
    palettes: Option<std::collections::BTreeMap<String, Vec<[u8; 4]>>>,
    expected_revision: u64,
    id: Option<u64>,
    name: String,
    font: String,
    colors: Vec<[u8; 4]>,
    #[serde(default)]
    logos: Vec<u64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Collection {
    expected_revision: u64,
    id: Option<u64>,
    name: String,
    assets: Vec<u64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Import {
    expected_revision: u64,
    path: PathBuf,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Export {
    id: u64,
    path: PathBuf,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Install {
    path: PathBuf,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BrandFile {
    #[serde(default)]
    typography:
        std::collections::BTreeMap<String, emulsion_core::design_brand_assets::TypographyRole>,
    #[serde(default)]
    palettes: std::collections::BTreeMap<String, Vec<[u8; 4]>>,
    #[serde(default)]
    fonts: std::collections::BTreeMap<String, emulsion_core::design_fonts::EmbeddedFont>,
    version: u32,
    name: String,
    font: String,
    colors: Vec<[u8; 4]>,
}
fn parse<T: serde::de::DeserializeOwned>(args: &Value) -> Result<T, String> {
    serde_json::from_value(args.clone()).map_err(|e| e.to_string())
}
fn error(value: impl Into<String>) -> emulsion_io::IoError {
    emulsion_io::IoError::Manifest(value.into())
}
fn edit<T: serde::Serialize>(
    root: &Path,
    revision: u64,
    change: impl FnOnce(&mut Catalog) -> emulsion_io::Result<T>,
) -> Result<Value, String> {
    let (catalog,value) = library::update(root, |catalog| {
        if catalog.revision != revision { return Err(error("Creative catalog changed. Inspect get_creative_catalog and retry with its current revision.")); }
        change(catalog)
    }).map_err(|e|e.to_string())?;
    Ok(json!({"revision":catalog.revision,"result":value}))
}
fn absolute(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        Err("Use an absolute local file path.".into())
    } else {
        Ok(())
    }
}
pub fn execute(root: &Path, name: &str, args: &Value) -> ToolResult {
    match run(root, name, args) {
        Ok(value) => ToolResult::text(value.to_string()),
        Err(error) => ToolResult::error(error),
    }
}
fn run(root: &Path, name: &str, args: &Value) -> Result<Value, String> {
    if extra::NAMES.contains(&name) {
        return extra::run(root, name, args);
    }
    match name {
        "get_creative_catalog" => {
            let a: Get = parse(args)?;
            if !(1..=200).contains(&a.limit) {
                return Err("limit must be 1–200".into());
            }
            let c = library::load(root).map_err(|e| e.to_string())?;
            Ok(
                json!({"revision":c.revision,"total_assets":c.assets.len(),"offset":a.offset,"next_offset":(a.offset.saturating_add(a.limit)<c.assets.len()).then_some(a.offset.saturating_add(a.limit)),"assets":c.assets.iter().skip(a.offset).take(a.limit).collect::<Vec<_>>(),"brands":c.brands.iter().map(|b|json!({"id":b.id,"name":b.name,"font":b.font,"colors":b.colors,"logos":b.logos,"typography":b.typography,"palettes":b.palettes,"fonts":b.fonts.values().map(|font|json!({"alias":font.alias(),"family":font.family(),"bytes":font.bytes().len()})).collect::<Vec<_>>()})).collect::<Vec<_>>(),"collections":c.collections,"asset_folders":c.asset_folders}),
            )
        }
        "register_creative_asset" => {
            let a: Register = parse(args)?;
            absolute(&a.path)?;
            // Image/logo registration follows the native reference catalog. It
            // does not place, transcode, upload or modify the original file.
            if matches!(a.kind, AssetKind::Stencil) {
                return Err("Use install_diagram_stencil_pack for stencil files.".into());
            }
            if a.kind == AssetKind::Template {
                return Err(
                    "Use install_design_template to validate/install template packs.".into(),
                );
            }
            edit(root, a.expected_revision, |c| c.add_asset(a.path, a.kind))
        }
        "update_creative_asset" => {
            let a: AssetPatch = parse(args)?;
            if a.name.is_none()
                && a.tags.is_none()
                && a.attribution.is_none()
                && a.license.is_none()
            {
                return Err("Provide an asset metadata field to update.".into());
            }
            edit(root, a.expected_revision, |c| {
                let asset = c
                    .assets
                    .iter_mut()
                    .find(|v| v.id == a.id)
                    .ok_or_else(|| error("Asset no longer exists."))?;
                if let Some(v) = a.name {
                    asset.name = v;
                }
                if let Some(v) = a.tags {
                    asset.tags = v;
                }
                if let Some(v) = a.attribution {
                    asset.attribution = v;
                }
                if let Some(v) = a.license {
                    asset.license = v;
                }
                Ok(a.id)
            })
        }
        "remove_creative_asset" | "remove_brand_kit" | "remove_asset_collection" => {
            let a: Remove = parse(args)?;
            edit(root, a.expected_revision, |c| {
                let exists = match name {
                    "remove_creative_asset" => c.assets.iter().any(|v| v.id == a.id),
                    "remove_brand_kit" => c.brands.iter().any(|v| v.id == a.id),
                    _ => c.collections.iter().any(|v| v.id == a.id),
                };
                if !exists {
                    return Err(error("Catalog entry no longer exists."));
                }
                match name {
                    "remove_creative_asset" => c.remove_asset(a.id),
                    "remove_brand_kit" => c.brands.retain(|v| v.id != a.id),
                    _ => c.collections.retain(|v| v.id != a.id),
                }
                Ok(a.id)
            })
        }
        "set_brand_kit" => {
            let a: Brand = parse(args)?;
            edit(root, a.expected_revision, |c| {
                let id = if let Some(id) = a.id {
                    let brand = c
                        .brands
                        .iter_mut()
                        .find(|b| b.id == id)
                        .ok_or_else(|| error("Brand kit no longer exists."))?;
                    brand.name = a.name;
                    brand.font = a.font;
                    brand.colors = a.colors;
                    id
                } else {
                    c.add_brand(a.name, a.font, a.colors)?
                };
                let brand = c.brands.iter_mut().find(|b| b.id == id).unwrap();
                brand.logos = a.logos;
                if let Some(roles) = a.typography {
                    brand.typography = roles;
                }
                if let Some(palettes) = a.palettes {
                    brand.palettes = palettes;
                }
                Ok(id)
            })
        }
        "set_asset_collection" => {
            let mut a: Collection = parse(args)?;
            a.assets.sort_unstable();
            a.assets.dedup();
            edit(root, a.expected_revision, |c| {
                if let Some(id) = a.id {
                    let v = c
                        .collections
                        .iter_mut()
                        .find(|v| v.id == id)
                        .ok_or_else(|| error("Collection no longer exists."))?;
                    v.name = a.name;
                    v.assets = a.assets;
                    Ok(id)
                } else {
                    c.add_collection(a.name, a.assets)
                }
            })
        }
        "import_brand_kit" => {
            let a: Import = parse(args)?;
            absolute(&a.path)?;
            use std::io::Read;
            let mut bytes = Vec::new();
            std::fs::File::open(a.path)
                .map_err(|e| e.to_string())?
                .take((96 << 20) + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() > 96 << 20 {
                return Err("Brand kit exceeds 96 MiB.".into());
            }
            let b: BrandFile = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if !(1..=2).contains(&b.version) {
                return Err("Unsupported brand kit version.".into());
            }
            edit(root, a.expected_revision, |c| {
                let id = c.add_brand(b.name, "Geist".into(), b.colors)?;
                let brand = c.brands.iter_mut().find(|b| b.id == id).unwrap();
                brand.font = b.font;
                brand.typography = b.typography;
                brand.palettes = b.palettes;
                brand.fonts = b.fonts;
                Ok(id)
            })
        }
        "export_brand_kit" => {
            let a: Export = parse(args)?;
            absolute(&a.path)?;
            let c = library::load(root).map_err(|e| e.to_string())?;
            let brand = c
                .brands
                .iter()
                .find(|b| b.id == a.id)
                .ok_or("Brand kit no longer exists.")?;
            library::export_brand(brand, &a.path).map_err(|e| e.to_string())?;
            Ok(
                json!({"path":a.path,"logos_embedded":false,"note":"Native brand files contain typography roles, palettes and embedded font bytes; logo references remain local."}),
            )
        }
        "install_design_template" => {
            let a: Install = parse(args)?;
            absolute(&a.path)?;
            if a.path
                .extension()
                .and_then(|v| v.to_str())
                .is_some_and(|v| v.eq_ignore_ascii_case("emu"))
            {
                let project = emulsion_io::project::read(&a.path).map_err(|e| e.to_string())?;
                project.validate()?;
                let (catalog, id) = library::update(root, |c| {
                    let id = c.add_asset(a.path.clone(), AssetKind::Template)?;
                    c.assets.iter_mut().find(|v| v.id == id).unwrap().variants =
                        project.pages.iter().map(|p| p.meta.name.clone()).collect();
                    Ok(id)
                })
                .map_err(|e| e.to_string())?;
                return Ok(
                    json!({"revision":catalog.revision,"asset_id":id,"source_referenced":true}),
                );
            }
            let pack = emulsion_io::template_pack::read(&a.path).map_err(|e| e.to_string())?;
            if pack.manifest.kind != emulsion_io::template_pack::Kind::Design {
                return Err("Choose a design template pack, not a stencil pack.".into());
            }
            let (c, id) =
                emulsion_io::template_pack::install(root, pack).map_err(|e| e.to_string())?;
            Ok(
                json!({"revision":c.revision,"asset_id":id,"asset":c.assets.iter().find(|a|a.id==id)}),
            )
        }
        _ => Err("Unknown creative catalog tool.".into()),
    }
}
pub fn definitions() -> Vec<ToolDef> {
    let mut out = Vec::new();
    let revision = json!({"type":"integer","minimum":0,"description":"Current revision from get_creative_catalog; stale catalog writes are rejected atomically."});
    let id = json!({"type":"integer","minimum":1});
    let path = json!({"type":"string","minLength":1,"description":"Absolute local file path"});
    let label = json!({"type":"string","minLength":1,"maxLength":200});
    let mut add = |name: &str, description: &str, properties: Value, required: &[&str]| {
        out.push(ToolDef{name:name.into(),description:description.into(),input_schema:json!({"type":"object","additionalProperties":false,"properties":properties,"required":required})})
    };
    add(
        "get_creative_catalog",
        "Inspect native local creative assets, template packs, brand kits/logos and collections, with optimistic revision. Asset list is paginated; no network calls.",
        json!({"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":200,"default":100}}),
        &[],
    );
    add(
        "register_creative_asset",
        "Register an existing local image or logo reference. Does not copy, change, decode or place its source file.",
        json!({"expected_revision":revision,"path":path,"kind":{"type":"string","enum":["image","logo"]}}),
        &["expected_revision", "path", "kind"],
    );
    add(
        "update_creative_asset",
        "Update local asset metadata, including installed template names/tags. Source files are unchanged.",
        json!({"expected_revision":revision,"id":id,"name":label,"tags":{"type":"array","maxItems":50,"items":label},"attribution":{"type":"string","maxLength":4000},"license":{"type":"string","maxLength":4000}}),
        &["expected_revision", "id"],
    );
    for (name, description) in [
        (
            "remove_creative_asset",
            "Remove an asset/template reference and unlink it from kits/collections. Never deletes the original or installed file.",
        ),
        (
            "remove_brand_kit",
            "Remove a local brand kit; logo assets and source files remain.",
        ),
        (
            "remove_asset_collection",
            "Remove a collection; its assets and source files remain.",
        ),
    ] {
        add(
            name,
            description,
            json!({"expected_revision":revision,"id":id}),
            &["expected_revision", "id"],
        );
    }
    add(
        "set_brand_kit",
        "Create a kit, or replace an existing kit when id is supplied. Uses native validation. Logos must reference existing logo assets.",
        json!({"expected_revision":revision,"id":id,"name":label,"font":label,"colors":{"type":"array","minItems":1,"maxItems":32,"items":{"type":"array","minItems":4,"maxItems":4,"items":{"type":"integer","minimum":0,"maximum":255}}},"logos":{"type":"array","maxItems":50,"items":id},"typography":{"type":"object","maxProperties":64,"description":"Named typography roles: font,size,bold,italic,line_height,letter_spacing,optional RGBA color"},"palettes":{"type":"object","maxProperties":64,"additionalProperties":{"type":"array","minItems":1,"maxItems":32,"items":{"type":"array","minItems":4,"maxItems":4,"items":{"type":"integer","minimum":0,"maximum":255}}}}}),
        &["expected_revision", "name", "font", "colors"],
    );
    add(
        "set_asset_collection",
        "Create a collection, or replace its name/membership when id is supplied. Duplicate asset IDs are normalized.",
        json!({"expected_revision":revision,"id":id,"name":label,"assets":{"type":"array","maxItems":10000,"items":id}}),
        &["expected_revision", "name", "assets"],
    );
    add(
        "import_brand_kit",
        "Import a native v1/v2 brand JSON file with named typography, palettes and embedded font resources. Does not fetch fonts or logo URLs.",
        json!({"expected_revision":revision,"path":path}),
        &["expected_revision", "path"],
    );
    add(
        "export_brand_kit",
        "Write a native brand JSON file with name/font/palette, named roles and embedded font resources. Logo references are not embedded by this format.",
        json!({"id":id,"path":path}),
        &["id", "path"],
    );
    add(
        "install_design_template",
        "Validate and install a local .emutemplate pack, or register an editable .emu project as a local template. Content-addressed installation is idempotent; no network or source deletion.",
        json!({"path":path}),
        &["path"],
    );
    out.extend(extra::definitions());
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn creative_catalog_revision_references_and_removal_are_atomic() {
        let root =
            std::env::temp_dir().join(format!("emulsion-mcp-creative-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("logo.svg");
        std::fs::write(&path, b"<svg/>").unwrap();
        let first = run(
            &root,
            "register_creative_asset",
            &json!({"expected_revision":0,"path":path,"kind":"logo"}),
        )
        .unwrap();
        let id = first["result"].as_u64().unwrap();
        let before = library::load(&root).unwrap();
        assert!(
            run(
                &root,
                "set_brand_kit",
                &json!({"expected_revision":0,"name":"Brand","font":"Sans","colors":[[1,2,3,255]]})
            )
            .is_err()
        );
        assert!(run(&root,"set_brand_kit",&json!({"expected_revision":1,"name":"Brand","font":"Sans","colors":[[1,2,3,255]],"logos":[999]})).is_err());
        assert_eq!(library::load(&root).unwrap(), before);
        run(&root,"set_brand_kit",&json!({"expected_revision":1,"name":"Brand","font":"Sans","colors":[[1,2,3,255]],"logos":[id]})).unwrap();
        run(
            &root,
            "set_asset_collection",
            &json!({"expected_revision":2,"name":"Launch","assets":[id,id]}),
        )
        .unwrap();
        run(
            &root,
            "remove_creative_asset",
            &json!({"expected_revision":3,"id":id}),
        )
        .unwrap();
        let c = library::load(&root).unwrap();
        assert!(c.assets.is_empty());
        assert!(c.brands[0].logos.is_empty());
        assert!(c.collections[0].assets.is_empty());
        assert!(path.exists());
        assert!(run(&root, "get_creative_catalog", &json!({"limit":201})).is_err());
        assert!(
            run(
                &root,
                "remove_brand_kit",
                &json!({"expected_revision":4,"id":2,"force":true})
            )
            .is_err()
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod roundtrip_tests {
    use super::*;
    #[test]
    fn creative_catalog_native_brand_and_template_roundtrip() {
        let root = std::env::temp_dir().join(format!(
            "emulsion-mcp-creative-roundtrip-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let brand = run(
            &root,
            "set_brand_kit",
            &json!({"expected_revision":0,"name":"Studio","font":"Sans","colors":[[20,40,60,255]]}),
        )
        .unwrap()["result"]
            .as_u64()
            .unwrap();
        let exported = root.join("studio.brand.json");
        run(
            &root,
            "export_brand_kit",
            &json!({"id":brand,"path":exported}),
        )
        .unwrap();
        run(
            &root,
            "import_brand_kit",
            &json!({"expected_revision":1,"path":exported}),
        )
        .unwrap();
        assert_eq!(library::load(&root).unwrap().brands.len(), 2);
        let native = root.join("starter.emu");
        let session = emulsion_core::creation::CanvasSpec {
            kind: emulsion_core::creation::CanvasKind::Design,
            name: "Starter".into(),
            width: 120.,
            height: 80.,
            ..Default::default()
        }
        .create_project()
        .unwrap();
        emulsion_io::project::write(&session.snapshot().unwrap(), &native).unwrap();
        let before = std::fs::read(&native).unwrap();
        let installed = run(&root, "install_design_template", &json!({"path":native})).unwrap();
        assert!(installed["asset_id"].as_u64().is_some());
        let again = run(&root, "install_design_template", &json!({"path":native})).unwrap();
        assert_eq!(installed["asset_id"], again["asset_id"]);
        assert_eq!(std::fs::read(&native).unwrap(), before);
        assert_eq!(library::load(&root).unwrap().assets.len(), 1);
        let malformed = root.join("bad.emutemplate");
        std::fs::write(&malformed, b"not a pack").unwrap();
        let catalog = library::load(&root).unwrap();
        assert!(run(&root, "install_design_template", &json!({"path":malformed})).is_err());
        assert_eq!(library::load(&root).unwrap(), catalog);
        std::fs::remove_dir_all(root).unwrap();
    }
}
