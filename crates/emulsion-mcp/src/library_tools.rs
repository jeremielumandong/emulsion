//! Live Library requests. The workspace owns selection, drafts and asynchronous jobs.
use crate::server::ToolDef;
use emulsion_core::raw::DevelopParams;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::PathBuf;

pub const NAMES: &[&str] = &[
    "merge_library_hdr",
    "stitch_library_panorama",
    "cancel_library_hdr",
    "library_profiles",
    "get_library",
    "get_library_preview",
    "import_library",
    "set_library_view",
    "select_library_photos",
    "edit_library_metadata",
    "library_collection",
    "library_catalog",
    "develop_library",
    "export_library",
    "open_library_photo",
    "cancel_library_export",
    "cancel_library_enhancement",
];
pub fn is_tool(name: &str) -> bool {
    NAMES.contains(&name)
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Page {
    #[serde(default)]
    pub offset: usize,
    pub limit: Option<usize>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Import {
    #[serde(default)]
    pub deduplicate: bool,
    pub folder: PathBuf,
}
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct View {
    pub canvas_tool: Option<String>,
    pub color_view: Option<emulsion_io::icc::PhotoView>,
    pub detail_region: Option<[f32; 2]>,
    pub fit_preview: Option<bool>,
    pub mask_overlay: Option<bool>,
    pub dust_visualization: Option<bool>,
    pub auto_advance: Option<bool>,
    pub panels_hidden: Option<bool>,
    pub filmstrip_hidden: Option<bool>,
    pub query: Option<String>,
    pub develop_section: Option<String>,
    pub collapse_stacks: Option<bool>,
    pub source: Option<Source>,
    pub collection: Option<u64>,
    pub minimum_rating: Option<u8>,
    pub flag: Option<Flag>,
    pub color_label: Option<u8>,
    pub raw_only: Option<bool>,
    pub clipping: Option<bool>,
    pub unedited: Option<bool>,
    pub sort: Option<Sort>,
    pub reverse: Option<bool>,
    pub mode: Option<Mode>,
    pub inspector: Option<Inspector>,
    /// Empty string clears the optional film recipe.
    pub recipe: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    All,
    Folder,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Flag {
    All,
    Picked,
    Rejected,
    Unflagged,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
    Filename,
    CaptureTime,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Loupe,
    PhotoCompare,
    Survey,
    Grid,
    List,
    Develop,
    Before,
    Compare,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Inspector {
    Develop,
    Info,
    Keywords,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub paths: Vec<PathBuf>,
    pub active: Option<PathBuf>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub paths: Vec<PathBuf>,
    pub rating: Option<u8>,
    pub flag: Option<Flag>,
    pub color_label: Option<u8>,
    pub keywords: Option<Vec<String>>,
    #[serde(default)]
    pub append_keywords: bool,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Collection {
    pub action: CollectionAction,
    pub name: Option<String>,
    pub id: Option<u64>,
    #[serde(default)]
    pub paths: Vec<PathBuf>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollectionAction {
    Create,
    Add,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Develop {
    pub guides: Option<Vec<[[f32; 2]; 2]>>,
    pub edits: Option<emulsion_core::develop_edits::LocalEdits>,
    pub action: DevelopAction,
    pub point: Option<[f32; 2]>,
    pub name: Option<String>,
    pub settings: Option<Value>,
    pub preset: Option<Preset>,
    pub path: Option<PathBuf>,
    pub group: Option<Group>,
}
#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DevelopAction {
    GuidedPerspective,
    LocalEdits,
    Adjust,
    Auto,
    Reset,
    AsShot,
    Undo,
    Reload,
    Save,
    Sync,
    Preset,
    SavePreset,
    LoadPreset,
    Snapshot,
    RestoreSnapshot,
    SubjectMask,
    SkyMask,
    AutoSky,
    AutoPerspective,
    DepthMap,
    Denoise,
    SuperResolution,
    MatchLens,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Preset {
    Neutral,
    Warm,
    BlackAndWhite,
    StrongContrast,
}
#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    All,
    WhiteBalance,
    Tone,
    Curve,
}
impl Group {
    pub fn raw(self) -> emulsion_io::raw_settings::RawSettingsGroup {
        use emulsion_io::raw_settings::RawSettingsGroup as G;
        match self {
            Self::All => G::All,
            Self::WhiteBalance => G::WhiteBalance,
            Self::Tone => G::Tone,
            Self::Curve => G::Curve,
        }
    }
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Export {
    pub settings: Option<emulsion_io::photo_export::OutputSettings>,
    pub out_dir: PathBuf,
    pub format: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogAction {
    pub action: String,
    #[serde(default)]
    pub paths: Vec<PathBuf>,
    pub name: Option<String>,
    pub path: Option<PathBuf>,
    pub rule: Option<emulsion_io::photo_catalog::SmartRule>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hdr {
    #[serde(skip)]
    pub panorama: bool,
    pub paths: Vec<PathBuf>,
    #[serde(default)]
    pub options: emulsion_io::photo_hdr::Options,
    pub output: Option<PathBuf>,
    #[serde(default)]
    pub preview: bool,
    #[serde(default)]
    pub overlay: bool,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profiles {
    pub action: String,
    pub digest: Option<[u8; 32]>,
    pub favorite: Option<bool>,
}
#[derive(Debug)]
pub enum Request {
    Hdr(Hdr),
    CancelHdr,
    Profiles(Profiles),
    State(Page),
    Preview,
    Import(Import),
    View(View),
    Select(Selection),
    Metadata(Metadata),
    Collection(Collection),
    Catalog(CatalogAction),
    Develop(Develop),
    Export(Export),
    Open,
    CancelExport,
    CancelEnhancement,
}
pub fn parse(name: &str, args: &Value) -> Result<Request, String> {
    fn from<T: serde::de::DeserializeOwned>(v: &Value) -> Result<T, String> {
        serde_json::from_value(v.clone()).map_err(|e| e.to_string())
    }
    fn empty(v: &Value) -> Result<(), String> {
        if v.as_object().is_some_and(|o| o.is_empty()) {
            Ok(())
        } else {
            Err("This tool takes an empty object".into())
        }
    }
    let r = match name {
        "cancel_library_hdr" => {
            empty(args)?;
            Request::CancelHdr
        }
        "merge_library_hdr" | "stitch_library_panorama" => {
            let mut v: Hdr = from(args)?;
            v.panorama = name == "stitch_library_panorama";
            if !(2..=9).contains(&v.paths.len()) || v.paths.iter().any(|p| !p.is_absolute()) {
                return Err("HDR needs 2–9 absolute source paths".into());
            }
            if (v.preview && v.output.is_some())
                || (!v.preview && v.output.as_ref().is_none_or(|p| !p.is_absolute()))
            {
                return Err(
                    "Full HDR merge requires an absolute new output path; previews take no output"
                        .into(),
                );
            }
            if let Some(ev) = &v.options.exposure_ev
                && (ev.len() != v.paths.len() || ev.iter().any(|e| !e.is_finite() || e.abs() > 40.))
            {
                return Err("Provide one finite exposure EV per source (within ±40)".into());
            }
            Request::Hdr(v)
        }
        "library_profiles" => {
            let v: Profiles = from(args)?;
            match v.action.as_str(){
                "list" if v.digest.is_none()&&v.favorite.is_none()=>{},
                "preview" if v.favorite.is_none()=>{},
                "favorite" if v.digest.is_some()&&v.favorite.is_some()=>{},
                _=>return Err("Use list, preview (optional digest), or favorite (digest and favorite required)".into()),
            }
            Request::Profiles(v)
        }
        "get_library" => Request::State(from(args)?),
        "get_library_preview" => {
            empty(args)?;
            Request::Preview
        }
        "import_library" => Request::Import(from(args)?),
        "set_library_view" => Request::View(from(args)?),
        "select_library_photos" => Request::Select(from(args)?),
        "edit_library_metadata" => Request::Metadata(from(args)?),
        "library_collection" => Request::Collection(from(args)?),
        "library_catalog" => Request::Catalog(from(args)?),
        "develop_library" => Request::Develop(from(args)?),
        "export_library" => Request::Export(from(args)?),
        "cancel_library_enhancement" => {
            empty(args)?;
            Request::CancelEnhancement
        }
        "cancel_library_export" => {
            empty(args)?;
            Request::CancelExport
        }
        "open_library_photo" => {
            empty(args)?;
            Request::Open
        }
        _ => return Err("Unknown Library tool".into()),
    };
    match &r {
        Request::State(p) if p.limit.is_some_and(|n| n == 0 || n > 200) => {
            return Err("limit must be 1–200".into());
        }
        Request::View(v) => {
            validate_labels(v.minimum_rating, v.color_label)?;
            if matches!(v.flag, Some(Flag::Unflagged)) {
                return Err("Library flag filter supports all, picked or rejected".into());
            }
            if v.collection.is_some() && matches!(v.source, Some(Source::Folder)) {
                return Err("A collection uses source=all".into());
            }
        }
        Request::Select(s) => validate_paths(&s.paths, true)?,
        Request::Metadata(m) => {
            validate_paths(&m.paths, false)?;
            validate_labels(m.rating, m.color_label)?;
            if matches!(m.flag, Some(Flag::All)) {
                return Err("Metadata flag must be picked, rejected or unflagged".into());
            }
            if m.rating.is_none()
                && m.flag.is_none()
                && m.color_label.is_none()
                && m.keywords.is_none()
            {
                return Err("Supply metadata to change".into());
            }
        }
        Request::Collection(c) => {
            validate_paths(&c.paths, true)?;
            match c.action {
                CollectionAction::Create
                    if c.name.as_ref().is_none_or(|n| n.trim().is_empty()) || c.id.is_some() =>
                {
                    return Err("create requires name and no id".into());
                }
                CollectionAction::Add
                    if c.id.is_none() || c.name.is_some() || c.paths.is_empty() =>
                {
                    return Err("add requires id and paths, without name".into());
                }
                _ => {}
            }
        }
        Request::Develop(d) => {
            use DevelopAction as A;
            if (d.action == A::GuidedPerspective) != d.guides.is_some() {
                return Err("guided_perspective requires guides".into());
            }
            if let Some(guides) = &d.guides
                && (!(2..=8).contains(&guides.len())
                    || guides
                        .iter()
                        .flatten()
                        .flatten()
                        .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v)))
            {
                return Err("Supply two to eight guides in normalized source coordinates".into());
            }
            if (d.action == A::LocalEdits) != d.edits.is_some() {
                return Err("local_edits requires edits".into());
            }
            if let Some(edits) = &d.edits {
                edits.validate().map_err(str::to_owned)?;
            }
            if (d.action == A::Adjust) != d.settings.is_some()
                || (d.action == A::Preset) != d.preset.is_some()
                || matches!(d.action, A::SavePreset | A::LoadPreset) != d.path.is_some()
                || d.group.is_some() && !matches!(d.action, A::Sync | A::Reset)
                || matches!(d.action, A::Snapshot | A::RestoreSnapshot) != d.name.is_some()
                || (d.action == A::SkyMask) != d.point.is_some()
                || d.point
                    .is_some_and(|p| p.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v)))
            {
                return Err("Arguments do not match the Develop action".into());
            }
            if let Some(settings) = &d.settings {
                patch(DevelopParams::default(), settings)?;
            }
        }
        Request::Export(e) if !matches!(e.format.as_str(), "jpg" | "png" | "tif" | "webp") => {
            return Err("Library export format must be jpg, png, tif or webp".into());
        }
        _ => {}
    }
    Ok(r)
}
fn validate_labels(rating: Option<u8>, label: Option<u8>) -> Result<(), String> {
    if rating.is_some_and(|v| v > 5) || label.is_some_and(|v| v > 5) {
        Err("rating and color_label must be 0–5".into())
    } else {
        Ok(())
    }
}
fn validate_paths(paths: &[PathBuf], empty: bool) -> Result<(), String> {
    if (!empty && paths.is_empty())
        || paths.len() > 10_000
        || paths.iter().any(|p| p.as_os_str().is_empty())
    {
        Err("Supply nonempty paths (maximum 10000)".into())
    } else {
        Ok(())
    }
}
pub fn patch(params: DevelopParams, patch: &Value) -> Result<DevelopParams, String> {
    let mut value = serde_json::to_value(params).map_err(|e| e.to_string())?;
    let object = patch
        .as_object()
        .filter(|o| !o.is_empty())
        .ok_or("settings must be a nonempty object")?;
    for (key, next) in object {
        if value.get(key).is_none() {
            return Err(format!("Unknown RAW setting '{key}'"));
        }
        value[key] = next.clone();
    }
    let result: DevelopParams = serde_json::from_value(value).map_err(|e| e.to_string())?;
    result.validate().map_err(|e| e.to_string())?;
    Ok(result)
}

pub fn definitions() -> Vec<ToolDef> {
    fn def(name: &str, description: &str, properties: Value, required: &[&str]) -> ToolDef {
        ToolDef {
            name: name.into(),
            description: description.into(),
            input_schema: json!({"type":"object","additionalProperties":false,"properties":properties,"required":required}),
        }
    }
    let paths = json!({"type":"array","maxItems":10000,"items":{"type":"string","minLength":1},"description":"Canonical absolute file paths returned by get_library"});
    let label = json!({"type":"integer","minimum":0,"maximum":5});
    let group = json!({"type":"string","enum":["all","white_balance","tone","curve"]});
    let settings = crate::raw_tools::definitions()
        .into_iter()
        .find(|d| d.name == "develop_raw")
        .unwrap()
        .input_schema["properties"]["settings"]
        .clone();
    vec![
        def(
            "cancel_library_hdr",
            "Cancel an active HDR or panorama merge or preview.",
            json!({}),
            &[],
        ),
        def(
            "library_profiles",
            "List installed DCP profiles/favorites, preview an actual profile on the active photo, or set a favorite. Apply a digest with develop_library settings.camera_profile.",
            json!({"action":{"type":"string","enum":["list","preview","favorite"]},"digest":{"type":["array","null"],"minItems":32,"maxItems":32,"items":{"type":"integer","minimum":0,"maximum":255}},"favorite":{"type":"boolean"}}),
            &["action"],
        ),
        def(
            "stitch_library_panorama",
            "Stitch 2–9 overlapping photos in capture order into a new planar panorama TIFF and add it to the Library. Uses saved Develop edits. Preview takes no output; full stitching requires a new absolute output path. cancel_library_hdr cancels either merge operation.",
            json!({"paths":{"type":"array","minItems":2,"maxItems":9,"items":{"type":"string"}},"preview":{"type":"boolean"},"output":{"type":"string"}}),
            &["paths"],
        ),
        def(
            "merge_library_hdr",
            "Merge 2–9 original bracketed exposures into a NEW RGB32 float TIFF and add it to the catalog. Existing Develop edits are ignored. Preview returns a reduced-resolution image without writing. Uses EXIF exposure or explicit relative EV; projective alignment with translation fallback and color-aware reference deghosting.",
            json!({"paths":{"type":"array","minItems":2,"maxItems":9,"items":{"type":"string"}},"output":{"type":"string"},"preview":{"type":"boolean"},"overlay":{"type":"boolean"},"options":{"type":"object","additionalProperties":false,"properties":{"align":{"type":"boolean"},"auto_tone":{"type":"boolean"},"deghost":{"type":"string","enum":["none","low","medium","high"]},"exposure_ev":{"type":["array","null"],"minItems":2,"maxItems":9,"items":{"type":"number","minimum":-40,"maximum":40}}}}}),
            &["paths"],
        ),
        def(
            "get_library",
            "Inspect the live Library: paginated visible files, selection, metadata, collections, filters, Develop settings, histogram, dirty/save/export state. No mutations. Use canonical paths for subsequent calls.",
            json!({"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":200,"default":100}}),
            &[],
        ),
        def(
            "get_library_preview",
            "Return the active Library photo as a PNG, including current Develop draft and optional recipe. Before mode shows as-shot; compare mode returns before and after images. Does not save or change selection.",
            json!({}),
            &[],
        ),
        def(
            "import_library",
            "Import supported images in one local folder (nonrecursive) into the catalog and display them. Originals remain in place. Waits for catalog persistence.",
            json!({"folder":{"type":"string","minLength":1},"deduplicate":{"type":"boolean","default":false}}),
            &["folder"],
        ),
        def(
            "set_library_view",
            "Patch Library search, filters, sorting, view, inspector or optional film recipe. source=all clears the collection; collection selects a catalog collection. Empty recipe clears it. Changes are reflected in the desktop Library.",
            json!({"canvas_tool":{"type":"string","enum":["none","brush","erase","heal","clone","crop","straighten","perspective","radial","linear","content_aware"]},"color_view":{"type":"object","additionalProperties":false,"properties":{"display":{"type":["string","null"]},"proof":{"type":["string","null"]},"gamut_warning":{"type":"boolean"}}},"detail_region":{"type":"array","minItems":2,"maxItems":2,"items":{"type":"number","minimum":0,"maximum":1}},"fit_preview":{"type":"boolean"},"mask_overlay":{"type":"boolean"},"dust_visualization":{"type":"boolean"},"auto_advance":{"type":"boolean"},"panels_hidden":{"type":"boolean"},"filmstrip_hidden":{"type":"boolean"},"query":{"type":"string"},"collapse_stacks":{"type":"boolean"},"develop_section":{"type":"string","enum":["basic","crop","curve","mixer","grading","masks","kelvin","history","enhance","detail","calibration","parametric"]},"source":{"type":"string","enum":["all","folder"]},"collection":{"type":"integer","minimum":1},"minimum_rating":label,"flag":{"type":"string","enum":["all","picked","rejected"]},"color_label":label,"raw_only":{"type":"boolean"},"clipping":{"type":"boolean"},"unedited":{"type":"boolean"},"sort":{"type":"string","enum":["filename","capture_time"]},"reverse":{"type":"boolean"},"mode":{"type":"string","enum":["grid","list","loupe","develop","before","compare","photo_compare","survey"]},"inspector":{"type":"string","enum":["develop","info","keywords"]},"recipe":{"type":"string"}}),
            &[],
        ),
        def(
            "select_library_photos",
            "Replace Library selection with visible canonical paths. Empty paths clears it. active must be selected; defaults to first selected. Select another active path to navigate the filmstrip.",
            json!({"paths":paths,"active":{"type":"string"}}),
            &["paths"],
        ),
        def(
            "edit_library_metadata",
            "Persist ratings, pick/unflag/reject, color labels (0 none,1 red,2 yellow,3 green,4 blue,5 purple), and keywords for explicit Library paths. Keywords replace by default or append when append_keywords=true. Pick and reject are mutually exclusive.",
            json!({"paths":paths,"rating":label,"flag":{"type":"string","enum":["picked","rejected","unflagged"]},"color_label":label,"keywords":{"type":"array","items":{"type":"string"}},"append_keywords":{"type":"boolean"}}),
            &["paths"],
        ),
        def(
            "library_collection",
            "Create a named collection (optionally empty) or add explicit Library paths to a collection ID. Catalog writes are atomic; source files are unchanged.",
            json!({"action":{"type":"string","enum":["create","add"]},"name":{"type":"string"},"id":{"type":"integer","minimum":1},"paths":paths}),
            &["action"],
        ),
        def(
            "library_catalog",
            "Photo catalog operations: smart_collection creates a live rule; stack/unstack use canonical paths; virtual_copy creates an independent recipe reference; backup/restore_backup use .emulibrary for a portable archive including photo originals, sidecars, presets and masks, or .json for catalog references; relink verifies identical original bytes and rewrites virtual references with their histories; import_lightroom reads .lrcat references, ratings, collections and supported readable histories, or an Adobe companion handoff JSON; import_presets installs XMP/lrtemplate/JSON/ZIP preset packs with compatibility warnings; save_export_preset/load_export_preset use a JSON path for the visible output settings.",
            json!({"action":{"type":"string","enum":["maintain_cache","relink_root","create_proxy","import_profile","undo_metadata","smart_collection","stack","unstack","virtual_copy","backup","relink","import_lightroom","restore_backup","import_presets","save_export_preset","load_export_preset"]},"paths":paths,"name":{"type":"string"},"path":{"type":"string"},"rule":{"type":"object","additionalProperties":false,"properties":{"minimum_rating":label,"color_label":label,"flagged":{"type":"boolean"},"rejected":{"type":"boolean"},"raw_only":{"type":"boolean"},"keyword":{"type":"string","maxLength":200}}}}),
            &["action"],
        ),
        def(
            "develop_library",
            "Operate on the active Library photo using the same drafts, undo and sidecars as the UI. adjust patches settings; auto/reset/as_shot/undo/preset/load_preset save immediately and report failures. sync copies group to selected photos. save flushes all drafts. reload explicitly discards active unsaved draft. Built-in preset names: neutral,warm,black_and_white,strong_contrast. Imports Emulsion JSON, Lightroom XMP and legacy lrtemplate presets with a compatibility report. snapshot/restore_snapshot use a name; match_lens resolves a measured Lensfun profile; subject_mask/sky_mask create local bitmap masks (sky_mask requires point); auto_sky uses local semantic sky segmentation; auto_perspective estimates level and perspective from image lines; denoise/super_resolution create new rendered derivatives. sensor_noise_reduction is a RAW-only pre-demosaic control in settings. point_curves contains composite/red/green/blue control points. Sampled camera WB cannot be synced across Library files. RAW highlights is recovery: positive darkens, negative brightens; UI slider uses the opposite sign.",
            json!({"guides":{"type":"array","minItems":2,"maxItems":8,"items":{"type":"array","minItems":2,"maxItems":2,"items":{"type":"array","minItems":2,"maxItems":2,"items":{"type":"number","minimum":0,"maximum":1}}}},"edits":local_edits_schema(),"action":{"type":"string","enum":["guided_perspective","local_edits","adjust","auto","reset","as_shot","undo","reload","save","sync","preset","save_preset","load_preset","snapshot","restore_snapshot","subject_mask","sky_mask","auto_sky","auto_perspective","depth_map","denoise","super_resolution","match_lens"]},"point":{"type":"array","minItems":2,"maxItems":2,"items":{"type":"number","minimum":0,"maximum":1},"description":"Normalized point in the untransformed source sky"},"name":{"type":"string","minLength":1,"maxLength":200},"settings":settings,"preset":{"type":"string","enum":["neutral","warm","black_and_white","strong_contrast"]},"path":{"type":"string","minLength":1},"group":group}),
            &["action"],
        ),
        def(
            "export_library",
            "Export selected Library photos using saved edits and the selected recipe to jpg/png/tif/webp. settings controls resizing, JPEG quality, 8/16-bit output, sharpening and an image watermark. Rejects dirty drafts. Waits for completion and reports partial failures. Existing files and originals are preserved. Source metadata is omitted by default; metadata selects copyright, camera or camera_and_location retention. Serial numbers and maker notes are always excluded. Optional publish sends versioned exports to WebDAV without replacing existing remote files.",
            json!({"out_dir":{"type":"string","minLength":1},"format":{"type":"string","enum":["jpg","png","tif","webp"]},"settings":{"type":"object","additionalProperties":false,"properties":{"publish":{"type":["object","null"],"additionalProperties":false,"required":["url"],"properties":{"url":{"type":"string"},"authorization_env":{"type":["string","null"]}}},"color_space":{"type":"string","enum":["srgb","adobe_rgb","pro_photo"]},"long_edge":{"type":"integer","minimum":0},"metadata":{"type":"string","enum":["none","copyright","camera","camera_and_location"]},"jpeg_quality":{"type":"integer","minimum":1,"maximum":100},"depth":{"type":"integer","enum":[0,8,16]},"sharpening":{"type":"number","minimum":0,"maximum":1},"watermark":{"type":["string","null"]},"watermark_opacity":{"type":"number","minimum":0,"maximum":1},"watermark_width":{"type":"number","minimum":0.01,"maximum":1}}}}),
            &["out_dir", "format"],
        ),
        def(
            "cancel_library_enhancement",
            "Cancel active local model inference; already completed derivative files are preserved.",
            json!({}),
            &[],
        ),
        def(
            "cancel_library_export",
            "Stop the Library export queue. An already encoding image may still finish; completed outputs are preserved. Inspect get_library for progress.",
            json!({}),
            &[],
        ),
        def(
            "open_library_photo",
            "Open the active saved Library image in Photo. Rejects unsaved RAW drafts. Returns the opened document ID; RAW Photo tools operate through that document's assistant relay.",
            json!({}),
            &[],
        ),
    ]
}

/// Encode a rendered Library snapshot using MCP's standard image content shape.
pub fn png_content(width: u32, height: u32, rgba: &[u8]) -> Result<Value, String> {
    use base64::Engine as _;
    let png = emulsion_io::export::png8(width, height, rgba).map_err(|e| e.to_string())?;
    Ok(
        json!({"type":"image","mimeType":"image/png","data":base64::engine::general_purpose::STANDARD.encode(png)}),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn guided_perspective_requires_bounded_source_guides() {
        use serde_json::json;
        assert!(super::parse("develop_library", &json!({
            "action":"guided_perspective", "guides":[[[0.2,0.1],[0.3,0.9]],[[0.8,0.1],[0.7,0.9]]]
        })).is_ok());
        for args in [
            json!({"action":"guided_perspective"}),
            json!({"action":"guided_perspective","guides":[]}),
            json!({"action":"guided_perspective","guides":[[[0.,0.],[2.,1.]],[[0.,0.],[1.,1.]]]}),
            json!({"action":"auto","guides":[]}),
        ] {
            assert!(super::parse("develop_library", &args).is_err());
        }
    }
    use super::*;
    #[test]
    fn schemas_and_read_only_registration_cover_library_tools() {
        let defs = definitions();
        assert_eq!(defs.len(), NAMES.len());
        for name in NAMES {
            assert!(defs.iter().any(|d| d.name == *name));
        }
        for name in ["get_library", "get_library_preview"] {
            assert!(crate::tools::READ_ONLY.contains(&name));
        }
    }
    #[test]
    fn schemas_expose_every_persisted_raw_setting() {
        let raw = crate::raw_tools::definitions()
            .into_iter()
            .find(|d| d.name == "develop_raw")
            .unwrap();
        let library = definitions()
            .into_iter()
            .find(|d| d.name == "develop_library")
            .unwrap();
        let values = serde_json::to_value(DevelopParams::default()).unwrap();
        for key in values.as_object().unwrap().keys() {
            assert!(
                raw.input_schema["properties"]["settings"]["properties"]
                    .get(key)
                    .is_some(),
                "Photo schema missing {key}"
            );
            assert!(
                library.input_schema["properties"]["settings"]["properties"]
                    .get(key)
                    .is_some(),
                "Library schema missing {key}"
            );
        }
    }
    #[test]
    fn strict_requests_reject_silent_noops_and_invalid_metadata() {
        for (name, args) in [
            ("edit_library_metadata", json!({"paths":["a"],"rating":6})),
            ("edit_library_metadata", json!({"paths":["a"]})),
            ("set_library_view", json!({"flag":"unflagged"})),
            ("develop_library", json!({"action":"save","settings":{}})),
            (
                "develop_library",
                json!({"action":"adjust","settings":{"typo":1}}),
            ),
            ("get_library", json!({"limit":0})),
            ("get_library_preview", json!({"path":"x"})),
        ] {
            assert!(parse(name, &args).is_err(), "{name} {args}");
        }
        assert!(
            parse(
                "develop_library",
                &json!({"action":"adjust","settings":{"whites":0.3,"noise_reduction":0.2}})
            )
            .is_ok()
        );
        assert!(
            parse(
                "library_collection",
                &json!({"action":"create","name":"Empty"})
            )
            .is_ok()
        );
    }
    #[test]
    fn expanded_develop_and_output_controls_are_reachable_through_mcp() {
        for action in ["auto_sky", "auto_perspective"] {
            assert!(parse("develop_library", &json!({"action":action})).is_ok());
            assert!(
                parse(
                    "develop_library",
                    &json!({"action":action,"point":[0.5,0.5]})
                )
                .is_err()
            );
        }
        let params = patch(
            DevelopParams::default(),
            &json!({
                "point_curves":[[],[[0.,1.],[0.4,0.3],[1.,0.]],[],[]],
                "sensor_noise_reduction":0.5
            }),
        )
        .unwrap();
        assert_eq!(params.point_curves[1].len, 3);
        assert_eq!(params.sensor_noise_reduction, 0.5);
        assert!(
            patch(
                params,
                &json!({"point_curves":[[],[[0.8,0.],[0.2,1.]],[],[]]})
            )
            .is_err()
        );
        let def = definitions()
            .into_iter()
            .find(|d| d.name == "export_library")
            .unwrap();
        let settings =
            serde_json::to_value(emulsion_io::photo_export::OutputSettings::default()).unwrap();
        for key in settings.as_object().unwrap().keys() {
            assert!(
                def.input_schema["properties"]["settings"]["properties"]
                    .get(key)
                    .is_some(),
                "Output schema missing {key}"
            );
        }
        assert!(parse("export_library", &json!({"out_dir":"out","format":"tif","settings":{"metadata":"camera","publish":{"url":"https://example.com/photos","authorization_env":"EMULSION_DAV_AUTH"}}})).is_ok());
    }
    #[test]
    fn partial_settings_preserve_existing_values_and_validate_all_controls() {
        let old = DevelopParams {
            exposure: 1.2,
            temperature: 0.3,
            ..Default::default()
        };
        let next = patch(old, &json!({"clarity":0.2,"vignette":-0.4})).unwrap();
        assert_eq!(next.exposure, old.exposure);
        assert_eq!(next.temperature, old.temperature);
        assert!(patch(old, &json!({"exposure":6})).is_err());
    }
}

fn local_edits_schema() -> Value {
    let unit = json!({"type":"number","minimum":0,"maximum":1});
    let signed = json!({"type":"number","minimum":-1,"maximum":1});
    let point = json!({"type":"array","minItems":2,"maxItems":2,"items":unit});
    let shape = json!({"type":"object","description":"Brush: points/radius/feather; radial: center/radius(two axes)/feather; linear: start/end; luminance: range/feather; color: rgb/tolerance/feather; bitmap: digest/inverted. Coordinates refer to oriented source before crop.","required":["type"],"properties":{"type":{"type":"string","enum":["brush","radial","linear","luminance","color","bitmap"]},"points":{"type":"array","minItems":1,"maxItems":4096,"items":point},"center":point,"start":point,"end":point,"range":point,"radius":{"oneOf":[unit,point]},"feather":unit,"rgb":{"type":"array","minItems":3,"maxItems":3,"items":unit},"tolerance":unit,"digest":{"type":"array","minItems":32,"maxItems":32,"items":{"type":"integer","minimum":0,"maximum":255}},"inverted":{"type":"boolean"}}});
    let id = json!({"type":"integer","minimum":1});
    json!({"type":"object","additionalProperties":false,"required":["version","masks","spots"],"properties":{"version":{"type":"integer","enum":[1]},"masks":{"type":"array","maxItems":64,"items":{"type":"object","additionalProperties":false,"required":["id","name","enabled","components","exposure","contrast","saturation","temperature","tint"],"properties":{"id":id,"name":{"type":"string","minLength":1,"maxLength":200},"enabled":{"type":"boolean"},"components":{"type":"array","minItems":1,"maxItems":64,"items":{"type":"object","additionalProperties":false,"required":["operation","shape"],"properties":{"operation":{"type":"string","enum":["add","subtract","intersect"]},"shape":shape}}},"exposure":{"type":"number","minimum":-5,"maximum":5},"contrast":signed,"saturation":signed,"temperature":signed,"tint":signed}}},"spots":{"type":"array","maxItems":256,"items":{"type":"object","additionalProperties":false,"required":["id","source","target","radius","feather","opacity","mode"],"properties":{"id":id,"source":point,"target":point,"stroke":{"type":"array","maxItems":512,"items":point},"radius":unit,"feather":unit,"opacity":unit,"mode":{"type":"string","enum":["heal","clone","content_aware"]}}}}}})
}

#[cfg(test)]
mod hdr_profile_tests {
    use super::*;
    #[test]
    fn hdr_and_profile_requests_require_explicit_valid_operations() {
        assert!(parse("merge_library_hdr",&json!({"paths":["/a.dng","/b.dng"],"preview":true,"options":{"exposure_ev":[-2,2]}})).is_ok());
        for args in [
            json!({"paths":["/a.dng","/b.dng"]}),
            json!({"paths":["/a.dng","/b.dng"],"preview":true,"output":"/out.tif"}),
            json!({"paths":["/a.dng","/b.dng"],"preview":true,"options":{"exposure_ev":[0]}}),
            json!({"paths":["/a.dng","/b.dng"],"preview":true,"options":{"deghost":"invalid"}}),
        ] {
            assert!(parse("merge_library_hdr", &args).is_err());
        }
        assert!(parse("library_profiles", &json!({"action":"list"})).is_ok());
        assert!(
            parse(
                "library_profiles",
                &json!({"action":"favorite","digest":vec![0;32]})
            )
            .is_err()
        );
        assert!(
            parse(
                "library_profiles",
                &json!({"action":"preview","favorite":true})
            )
            .is_err()
        );
        assert!(parse("cancel_library_hdr", &json!({})).is_ok());
        assert!(parse("cancel_library_hdr", &json!({"typo":true})).is_err());
    }
}
