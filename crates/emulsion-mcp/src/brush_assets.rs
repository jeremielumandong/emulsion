//! Brush assets and previews share the canvas renderer and durable catalog store.
use crate::server::{ToolDef, ToolResult};
use emulsion_io::brush_library::{self as store, Catalog, ExportScope};
use emulsion_raster::{
    Raster, color,
    paint::{Brush, DualBlend, textures},
    preview::{self, PreviewMode, StrokeSample},
};
use serde_json::{Value, json};
use std::{
    io::{Cursor, Read},
    path::{Path, PathBuf},
    sync::Arc,
};

pub(crate) const NAMES: &[&str] = &[
    "brush_source",
    "import_brushes",
    "export_brushes",
    "preview_brush",
];
pub(crate) const READ_ONLY: &[&str] = &["preview_brush"];

pub(crate) fn definitions() -> Vec<ToolDef> {
    let specs = [
        (
            "brush_source",
            "Edit a saved brush's immutable image source. Requires current catalog expected_revision. Import PNG/JPEG (32 MiB, 4096 px limit), invert, rotate 90 degrees, seamless mirror, generate diamond/paper, or clear. Originals/reset points retain their sources.",
            json!({"brush_id":{"type":"string"},"expected_revision":{"type":"integer","minimum":0},"component":{"enum":["primary","secondary"],"default":"primary"},"source":{"enum":["shape","grain"]},"action":{"enum":["import","invert","rotate","seamless","diamond","paper","clear"]},"path":{"type":"string"}}),
            vec!["brush_id", "expected_revision", "source", "action"],
        ),
        (
            "import_brushes",
            "Inspect or apply .embrushes, .brush, .brushset, .brushlibrary or sampled-tip ABR imports. Defaults to dry_run, with conversion warnings and added definitions; no persistent writes in dry-run. Applying requires dry_run=false and expected_revision. Native packages retain library/set hierarchy. Dry-run IDs are temporary; apply the same paths and use the IDs returned by the committed import.",
            json!({"paths":{"type":"array","items":{"type":"string"},"minItems":1,"maxItems":32},"target_set":{"type":"string"},"dry_run":{"type":"boolean","default":true},"expected_revision":{"type":"integer","minimum":0}}),
            vec!["paths", "target_set"],
        ),
        (
            "export_brushes",
            "Export a self-contained native .embrushes package with current/original/reset sources and dual settings. Select brush_ids or scope=set/library with scope_id. Refuses to overwrite an existing file unless overwrite=true.",
            json!({"path":{"type":"string"},"scope":{"enum":["brushes","set","library"],"default":"brushes"},"scope_id":{"type":"string"},"brush_ids":{"type":"array","items":{"type":"string"}},"overwrite":{"type":"boolean","default":false}}),
            vec!["path"],
        ),
        (
            "preview_brush",
            "Render a deterministic standalone PNG using the canvas engine, without catalog/document writes. Optional settings and secondary_settings recursively override saved settings for this preview; secondary_settings=null disables a saved secondary component. Samples use pixel x/y, optional monotonic time_ms (omitted timestamps advance 16 ms), optional pressure 0..1 and tilt [x,y] degrees -90..90. RGBA colors are sRGB bytes. Smudge needs variation in the background; background_path can supply it.",
            json!({"brush_id":{"type":"string"},"settings":{"type":"object"},"secondary_settings":{"type":["object","null"]},"combine_mode":{"enum":["Normal","Multiply","Screen"]},"width":{"type":"integer","minimum":16,"maximum":1024,"default":480},"height":{"type":"integer","minimum":16,"maximum":1024,"default":360},"seed":{"type":"integer","minimum":0,"default":1},"mode":{"enum":["paint","smudge","erase"],"default":"paint"},"color":{"type":"array","items":{"type":"integer","minimum":0,"maximum":255},"minItems":4,"maxItems":4},"background":{"type":"array","items":{"type":"integer","minimum":0,"maximum":255},"minItems":4,"maxItems":4},"background_path":{"type":"string"},"samples":{"type":"array","minItems":1,"maxItems":2000,"items":{"type":"object","properties":{"x":{"type":"number"},"y":{"type":"number"},"time_ms":{"type":"number","minimum":0},"pressure":{"type":"number","minimum":0,"maximum":1},"tilt":{"type":"array","minItems":2,"maxItems":2,"items":{"type":"number","minimum":-90,"maximum":90}}},"required":["x","y"],"additionalProperties":false}}}),
            vec![],
        ),
    ];
    specs.into_iter().map(|(name,description,properties,required)| ToolDef { name:name.into(),description:description.into(),input_schema:json!({"type":"object","properties":properties,"required":required,"additionalProperties":false}) }).collect()
}
fn string<'a>(args: &'a Value, key: &str) -> anyhow::Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("{key} must be a nonempty string"))
}
fn optional_string<'a>(args: &'a Value, key: &str, default: &'a str) -> anyhow::Result<&'a str> {
    if args.get(key).is_some() {
        string(args, key)
    } else {
        Ok(default)
    }
}
fn boolean(args: &Value, key: &str, default: bool) -> anyhow::Result<bool> {
    args.get(key).map_or(Ok(default), |v| {
        v.as_bool()
            .ok_or_else(|| anyhow::anyhow!("{key} must be boolean"))
    })
}
fn revision(args: &Value, catalog: &Catalog) -> anyhow::Result<u64> {
    let rev = args
        .get("expected_revision")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow::anyhow!("expected_revision is required"))?;
    anyhow::ensure!(
        rev == catalog.revision,
        "catalog revision conflict: expected {rev}, current {}",
        catalog.revision
    );
    Ok(rev)
}
pub(crate) fn execute(name: &str, args: &Value) -> ToolResult {
    match run(name, args) {
        Ok(result) => result,
        Err(e) => ToolResult::error(e.to_string()),
    }
}
fn run(name: &str, args: &Value) -> anyhow::Result<ToolResult> {
    let definition = definitions()
        .into_iter()
        .find(|d| d.name == name)
        .ok_or_else(|| anyhow::anyhow!("Unknown brush asset tool"))?;
    let object = args
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("arguments must be an object"))?;
    for key in object.keys() {
        anyhow::ensure!(
            definition.input_schema["properties"].get(key).is_some(),
            "Unknown argument: {key}"
        );
    }
    let catalog = store::load()?;
    match name {
        "brush_source" => source(args, catalog),
        "import_brushes" => import(args, catalog),
        "export_brushes" => export(args, &catalog),
        "preview_brush" => render_preview(args, &catalog),
        _ => unreachable!(),
    }
}
fn decode(path: &Path) -> anyhow::Result<image::DynamicImage> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(32 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() <= 32 * 1024 * 1024,
        "Source image exceeds 32 MiB"
    );
    let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    anyhow::ensure!(
        matches!(
            reader.format(),
            Some(image::ImageFormat::Png | image::ImageFormat::Jpeg)
        ),
        "Source must be PNG or JPEG"
    );
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    Ok(reader.decode()?)
}
// Same source transformations as Brush Studio; operate on registry coverage,
// preserving alpha-derived shape semantics rather than flattening transparency.
fn mirror(source: image::GrayImage) -> image::GrayImage {
    let largest = source.width().max(source.height());
    let source = if largest > 2048 {
        let scale = 2048.0 / largest as f32;
        image::imageops::resize(
            &source,
            (source.width() as f32 * scale).round().max(1.0) as u32,
            (source.height() as f32 * scale).round().max(1.0) as u32,
            image::imageops::FilterType::Triangle,
        )
    } else {
        source
    };
    let (w, h) = source.dimensions();
    image::GrayImage::from_fn(w * 2, h * 2, |x, y| {
        *source.get_pixel(
            if x < w { x } else { w * 2 - x - 1 },
            if y < h { y } else { h * 2 - y - 1 },
        )
    })
}
fn transform(id: u32, action: &str) -> anyhow::Result<image::GrayImage> {
    if action == "diamond" {
        return Ok(image::GrayImage::from_fn(128, 128, |x, y| {
            let d = ((x as f32 + 0.5) / 64.0 - 1.0).abs() + ((y as f32 + 0.5) / 64.0 - 1.0).abs();
            image::Luma([((1.0 - d).clamp(0.0, 1.0) * 255.0).round() as u8])
        }));
    }
    if action == "paper" {
        let noise = image::GrayImage::from_fn(128, 128, |x, y| {
            let mut hash = x.wrapping_mul(374_761_393) ^ y.wrapping_mul(668_265_263) ^ 0xA3B1_5577;
            hash = (hash ^ (hash >> 13)).wrapping_mul(1_274_126_177);
            image::Luma([40 + ((hash ^ (hash >> 16)) % 216) as u8])
        });
        return Ok(mirror(image::imageops::blur(&noise, 0.65)));
    }
    let texture =
        textures::get(id).ok_or_else(|| anyhow::anyhow!("Import or generate a source first"))?;
    let (w, h) = (texture.width(), texture.height());
    anyhow::ensure!(w <= 4096 && h <= 4096, "Source exceeds 4096 pixels");
    let mut pixels = image::GrayImage::from_fn(w, h, |x, y| {
        image::Luma([
            (texture.sample((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32) * 255.0)
                .round() as u8,
        ])
    });
    match action {
        "invert" => {
            image::imageops::invert(&mut pixels);
            Ok(pixels)
        }
        "rotate" => Ok(image::imageops::rotate90(&pixels)),
        "seamless" => Ok(mirror(pixels)),
        _ => anyhow::bail!("Unknown source action: {action}"),
    }
}
fn source(args: &Value, catalog: Catalog) -> anyhow::Result<ToolResult> {
    source_at(args, catalog, &store::root())
}
fn source_at(args: &Value, mut catalog: Catalog, root: &Path) -> anyhow::Result<ToolResult> {
    let expected = revision(args, &catalog)?;
    let id = string(args, "brush_id")?;
    let action = string(args, "action")?;
    let secondary = match optional_string(args, "component", "primary")? {
        "primary" => false,
        "secondary" => true,
        _ => anyhow::bail!("component must be primary or secondary"),
    };
    let grain = match string(args, "source")? {
        "grain" => true,
        "shape" => false,
        _ => anyhow::bail!("source must be shape or grain"),
    };
    let definition = catalog
        .brush_mut(id)
        .ok_or_else(|| anyhow::anyhow!("Unknown brush_id"))?;
    let brush = if secondary {
        definition
            .secondary
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Brush has no secondary component"))?
    } else {
        &mut definition.brush
    };
    let (hash, handle) = if action == "clear" {
        (None, 0)
    } else {
        let image = if action == "import" {
            decode(Path::new(string(args, "path")?))?
        } else {
            image::DynamicImage::ImageLuma8(transform(
                if grain { brush.grain_tex } else { brush.tip },
                action,
            )?)
        };
        let mut png = Cursor::new(Vec::new());
        image.write_to(&mut png, image::ImageFormat::Png)?;
        let (hash, handle) = store::store_texture_asset_to(root, &png.into_inner())?;
        (Some(hash), handle)
    };
    if grain {
        brush.grain_tex = handle;
        if handle != 0 {
            brush.grain_strength = 1.0;
        }
    } else {
        brush.tip = handle;
    }
    match (secondary, grain) {
        (false, false) => definition.shape_asset = hash,
        (false, true) => definition.grain_asset = hash,
        (true, false) => definition.secondary_shape_asset = hash,
        (true, true) => definition.secondary_grain_asset = hash,
    }
    let saved = store::commit_to(root, expected, &catalog)?;
    Ok(ToolResult::text(
        json!({"revision":saved.revision,"brush":saved.brush(id)}).to_string(),
    ))
}
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> anyhow::Result<Self> {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("{e}"))?;
        let path = std::env::temp_dir().join(format!(
            "emulsion-brush-import-{}",
            bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
        ));
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn import(args: &Value, catalog: Catalog) -> anyhow::Result<ToolResult> {
    import_at(args, catalog, &store::root())
}
fn import_at(args: &Value, mut catalog: Catalog, root: &Path) -> anyhow::Result<ToolResult> {
    let dry = boolean(args, "dry_run", true)?;
    let expected = if dry {
        catalog.revision
    } else {
        revision(args, &catalog)?
    };
    let paths = args
        .get("paths")
        .and_then(Value::as_array)
        .filter(|p| !p.is_empty() && p.len() <= 32)
        .ok_or_else(|| anyhow::anyhow!("paths must contain 1..32 paths"))?
        .iter()
        .map(|v| {
            v.as_str()
                .map(PathBuf::from)
                .ok_or_else(|| anyhow::anyhow!("paths must be strings"))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let set = string(args, "target_set")?;
    let scratch = if dry { Some(Scratch::new()?) } else { None };
    let report = if let Some(scratch) = &scratch {
        store::import_paths_to(&scratch.0, &mut catalog, &paths, set)?
    } else {
        store::import_paths_to(root, &mut catalog, &paths, set)?
    };
    if !dry {
        catalog = store::commit_to(root, expected, &catalog)?;
    }
    Ok(ToolResult::text(json!({"dry_run":dry,"revision":catalog.revision,"warnings":report.warnings,"added":report.added.iter().filter_map(|id|catalog.brush(id)).collect::<Vec<_>>(),"libraries":catalog.libraries,"sets":catalog.sets}).to_string()))
}
fn export(args: &Value, catalog: &Catalog) -> anyhow::Result<ToolResult> {
    export_at(args, catalog, &store::root())
}
fn export_at(args: &Value, catalog: &Catalog, root: &Path) -> anyhow::Result<ToolResult> {
    let path = Path::new(string(args, "path")?);
    anyhow::ensure!(
        path.extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("embrushes")),
        "Export path must end in .embrushes"
    );
    let overwrite = boolean(args, "overwrite", false)?;
    anyhow::ensure!(
        !path.exists() || overwrite,
        "Destination exists; set overwrite=true to replace it"
    );
    let scope = match optional_string(args, "scope", "brushes")? {
        "brushes" => ExportScope::Brushes,
        "set" => ExportScope::Set(string(args, "scope_id")?.into()),
        "library" => ExportScope::Library(string(args, "scope_id")?.into()),
        _ => anyhow::bail!("scope must be brushes, set or library"),
    };
    let ids = if let Some(v) = args.get("brush_ids") {
        v.as_array()
            .ok_or_else(|| anyhow::anyhow!("brush_ids must be an array"))?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| anyhow::anyhow!("brush_ids must be strings"))
            })
            .collect::<anyhow::Result<Vec<_>>>()?
    } else {
        Vec::new()
    };
    anyhow::ensure!(
        scope != ExportScope::Brushes || !ids.is_empty(),
        "brush_ids must not be empty for brushes scope"
    );
    store::export_package_scoped_from(root, path, catalog, &ids, scope)?;
    Ok(ToolResult::text(
        json!({"path":path,"revision":catalog.revision}).to_string(),
    ))
}
fn rgba(args: &Value, key: &str, default: [u8; 4]) -> anyhow::Result<[u8; 4]> {
    let Some(value) = args.get(key) else {
        return Ok(default);
    };
    let values = value
        .as_array()
        .filter(|v| v.len() == 4)
        .ok_or_else(|| anyhow::anyhow!("{key} must be four sRGB bytes"))?;
    let mut result = [0; 4];
    for (out, v) in result.iter_mut().zip(values) {
        *out = v
            .as_u64()
            .and_then(|v| u8::try_from(v).ok())
            .ok_or_else(|| anyhow::anyhow!("{key} must be four sRGB bytes"))?;
    }
    Ok(result)
}
fn tool_error(error: ToolResult) -> anyhow::Error {
    anyhow::anyhow!(
        "{}",
        error
            .content
            .first()
            .and_then(|v| v["text"].as_str())
            .unwrap_or("Brush operation failed")
    )
}
fn samples(args: &Value, w: u32, h: u32) -> anyhow::Result<Vec<StrokeSample>> {
    let Some(values) = args.get("samples") else {
        return Ok(preview::sample_stroke(w, h));
    };
    let points = crate::exec::parse_brush_samples(values).map_err(tool_error)?;
    anyhow::ensure!(
        !points.is_empty() && points.len() <= 2000,
        "samples must contain 1..2000 points"
    );
    anyhow::ensure!(
        points
            .iter()
            .all(|s| s.x >= 0.0 && s.x <= w as f32 && s.y >= 0.0 && s.y <= h as f32),
        "sample coordinates must be inside preview"
    );
    Ok(points)
}
fn render_preview(args: &Value, catalog: &Catalog) -> anyhow::Result<ToolResult> {
    let definition = if args.get("brush_id").is_some() {
        Some(
            catalog
                .brush(string(args, "brush_id")?)
                .ok_or_else(|| anyhow::anyhow!("Unknown brush_id"))?,
        )
    } else {
        None
    };
    let mut brush = definition.map_or(Brush::default(), |b| b.brush);
    let mut secondary = definition.and_then(|b| b.secondary);
    if let Some(settings) = args.get("settings") {
        brush = crate::exec::apply_brush_settings(brush, settings).map_err(tool_error)?;
    }
    if let Some(settings) = args.get("secondary_settings") {
        secondary = if settings.is_null() {
            None
        } else {
            Some(
                crate::exec::apply_brush_settings(secondary.unwrap_or_default(), settings)
                    .map_err(tool_error)?,
            )
        };
    }
    let blend = if let Some(v) = args.get("combine_mode") {
        anyhow::ensure!(
            secondary.is_some(),
            "combine_mode requires a secondary brush"
        );
        crate::exec::parse_dual_blend(v).map_err(tool_error)?
    } else {
        definition.map_or(DualBlend::Normal, |b| b.combine_mode)
    };
    let dimension = |key: &str, default: u32| -> anyhow::Result<u32> {
        args.get(key).map_or(Ok(default), |v| {
            v.as_u64()
                .filter(|v| (16..=1024).contains(v))
                .map(|v| v as u32)
                .ok_or_else(|| anyhow::anyhow!("{key} must be 16..1024"))
        })
    };
    let (w, h) = (dimension("width", 480)?, dimension("height", 360)?);
    let color = rgba(args, "color", [25, 45, 90, 255])?;
    let background = rgba(args, "background", [245, 245, 245, 255])?;
    let image = if let Some(path) = args.get("background_path") {
        let path = path
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("background_path must be a string"))?;
        decode(Path::new(path))?
            .resize_exact(w, h, image::imageops::FilterType::Triangle)
            .to_rgba8()
    } else {
        image::RgbaImage::from_pixel(w, h, image::Rgba(background))
    };
    let base = Arc::new(Raster::from_srgba8(w, h, image.as_raw()));
    let mode = match optional_string(args, "mode", "paint")? {
        "paint" => PreviewMode::Paint(color::srgba8_to_premul(color)),
        "smudge" => PreviewMode::Smudge,
        "erase" => PreviewMode::Erase,
        _ => anyhow::bail!("mode must be paint, smudge or erase"),
    };
    let seed = args.get("seed").map_or(Ok(1), |v| {
        v.as_u64()
            .ok_or_else(|| anyhow::anyhow!("seed must be a nonnegative integer"))
    })?;
    let points = samples(args, w, h)?;
    let raster = if let Some(secondary) = secondary {
        preview::render_dual_stroke(base, brush, secondary, blend, mode, &points, seed)
    } else {
        preview::render_stroke(base, brush, mode, &points, seed)
    };
    let image = image::RgbaImage::from_raw(w, h, raster.to_srgba8()).expect("raster dimensions");
    let png = crate::preview::png_block(&image).map_err(|e| anyhow::anyhow!("{:?}", e.content))?;
    let mut result=ToolResult::text(json!({"width":w,"height":h,"seed":seed,"samples":points.len(),"settings":brush,"secondary":secondary,"combine_mode":blend,"revision":catalog.revision}).to_string());
    result.content.push(png);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Scratch, Catalog, String) {
        let dir = Scratch::new().unwrap();
        let mut catalog = store::load_from(&dir.0).unwrap();
        let set = catalog
            .create_set(store::USER_LIBRARY, "MCP assets")
            .unwrap();
        let id = catalog
            .add_brush(&set, "Asset test", Brush::default())
            .unwrap();
        let catalog = store::commit_to(&dir.0, catalog.revision, &catalog).unwrap();
        (dir, catalog, id)
    }
    #[test]
    fn source_edits_are_revision_checked_and_preserve_reset_assets() {
        let (dir, catalog, id) = fixture();
        source_at(&json!({"brush_id":id,"expected_revision":catalog.revision,"source":"shape","action":"diamond"}),catalog,&dir.0).unwrap();
        let mut saved = store::load_from(&dir.0).unwrap();
        let original = saved.brush(&id).unwrap().shape_asset.clone();
        saved.create_reset_point(&id).unwrap();
        let saved = store::commit_to(&dir.0, saved.revision, &saved).unwrap();
        assert!(
            source_at(
                &json!({"brush_id":id,"expected_revision":0,"source":"shape","action":"clear"}),
                saved.clone(),
                &dir.0
            )
            .is_err()
        );
        source_at(&json!({"brush_id":id,"expected_revision":saved.revision,"source":"shape","action":"invert"}),saved,&dir.0).unwrap();
        let mut saved = store::load_from(&dir.0).unwrap();
        assert_ne!(saved.brush(&id).unwrap().shape_asset, original);
        assert_eq!(saved.brush(&id).unwrap().reset_point_shape_asset, original);
        saved.reset_brush(&id).unwrap();
        assert_eq!(saved.brush(&id).unwrap().shape_asset, original);
    }
    #[test]
    fn secondary_source_does_not_modify_primary() {
        let (dir, mut catalog, id) = fixture();
        catalog.brush_mut(&id).unwrap().secondary = Some(Brush::default());
        let catalog = store::commit_to(&dir.0, catalog.revision, &catalog).unwrap();
        source_at(&json!({"brush_id":id,"expected_revision":catalog.revision,"component":"secondary","source":"grain","action":"paper"}),catalog,&dir.0).unwrap();
        let saved = store::load_from(&dir.0).unwrap();
        let brush = saved.brush(&id).unwrap();
        assert_eq!(brush.brush.grain_tex, 0);
        assert_ne!(brush.secondary.unwrap().grain_tex, 0);
        assert!(brush.secondary_grain_asset.is_some());
    }
    #[test]
    fn package_export_preserves_hierarchy_and_dry_run_does_not_commit() {
        let (dir, mut catalog, id) = fixture();
        let library = catalog.create_library("Export library").unwrap();
        let set = catalog.create_set(&library, "Export set").unwrap();
        catalog.move_brush(&id, &set, 0).unwrap();
        let catalog = store::commit_to(&dir.0, catalog.revision, &catalog).unwrap();
        let package = dir.0.join("test.embrushes");
        export_at(
            &json!({"path":package,"scope":"library","scope_id":library}),
            &catalog,
            &dir.0,
        )
        .unwrap();
        assert!(
            export_at(
                &json!({"path":package,"scope":"library","scope_id":library}),
                &catalog,
                &dir.0
            )
            .is_err()
        );
        let other = Scratch::new().unwrap();
        let target = store::load_from(&other.0).unwrap();
        let revision = target.revision;
        let result = import_at(
            &json!({"paths":[package],"target_set":store::USER_SET}),
            target.clone(),
            &other.0,
        )
        .unwrap();
        let result: Value =
            serde_json::from_str(result.content[0]["text"].as_str().unwrap()).unwrap();
        assert!(result["dry_run"].as_bool().unwrap());
        assert_eq!(store::load_from(&other.0).unwrap(), target);
        import_at(&json!({"paths":[package],"target_set":store::USER_SET,"dry_run":false,"expected_revision":revision}),target,&other.0).unwrap();
        let imported = store::load_from(&other.0).unwrap();
        assert!(
            imported
                .libraries
                .iter()
                .any(|l| l.name == "Export library")
        );
        assert!(imported.sets.iter().any(|s| s.name == "Export set"));
    }
    #[test]
    fn malformed_import_leaves_catalog_unchanged() {
        let (dir, catalog, _) = fixture();
        let file = dir.0.join("broken.embrushes");
        std::fs::write(&file, b"not a zip").unwrap();
        assert!(import_at(&json!({"paths":[file],"target_set":store::USER_SET,"dry_run":false,"expected_revision":catalog.revision}),catalog.clone(),&dir.0).is_err());
        assert_eq!(store::load_from(&dir.0).unwrap(), catalog);
    }
    #[test]
    fn preview_is_deterministic_and_accepts_tilt_time_and_secondary_drafts() {
        let (_, catalog, id) = fixture();
        let args = json!({"brush_id":id,"width":128,"height":64,"seed":42,"settings":{"size":12,"advanced":{"shape":{"rotation_jitter":0.5}}},"secondary_settings":{"size":5},"combine_mode":"Screen","samples":[{"x":12,"y":20,"pressure":0.2,"tilt":[12,25],"time_ms":0},{"x":100,"y":40,"pressure":1,"tilt":[20,10],"time_ms":20}]});
        let a = render_preview(&args, &catalog).unwrap();
        let b = render_preview(&args, &catalog).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.content[1]["mimeType"], "image/png");
        let mut bad = args;
        bad["settings"]["unknown"] = json!(1);
        assert!(render_preview(&bad, &catalog).is_err());
    }
    #[test]
    fn preview_secondary_can_be_disabled_and_combine_requires_secondary() {
        let (_, mut catalog, id) = fixture();
        catalog.brush_mut(&id).unwrap().secondary = Some(Brush::default());
        let result = render_preview(
            &json!({"brush_id":id,"width":32,"height":32,"secondary_settings":null}),
            &catalog,
        )
        .unwrap();
        let metadata: Value =
            serde_json::from_str(result.content[0]["text"].as_str().unwrap()).unwrap();
        assert!(metadata["secondary"].is_null());
        assert!(
            render_preview(
                &json!({"brush_id":id,"secondary_settings":null,"combine_mode":"Normal"}),
                &catalog
            )
            .is_err()
        );
    }
    #[test]
    fn source_decoder_rejects_invalid_images_and_mirror_edges_match() {
        let dir = Scratch::new().unwrap();
        let file = dir.0.join("invalid.png");
        std::fs::write(&file, b"invalid").unwrap();
        assert!(decode(&file).is_err());
        let source = image::GrayImage::from_raw(2, 2, vec![0, 64, 128, 255]).unwrap();
        let tile = mirror(source);
        assert_eq!(tile.get_pixel(0, 0), tile.get_pixel(3, 0));
        assert_eq!(tile.get_pixel(1, 0), tile.get_pixel(1, 3));
        assert_eq!(
            transform(0, "paper").unwrap(),
            transform(0, "paper").unwrap()
        );
    }
}
