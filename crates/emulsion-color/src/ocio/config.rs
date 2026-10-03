//! OCIO config files (profile versions 1 and 2): roles, colour spaces,
//! displays and views, view transforms, looks, search paths and
//! environment variables.

use super::transform::{self, Transform, get, get_text, mapping, tag_name, text};
use super::{Error, Result};
use serde_yaml::Value;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Which reference a colour space converts to: scene-referred (v1 and most
/// v2 spaces) or display-referred (v2 `display_colorspaces`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reference {
    Scene,
    Display,
}

#[derive(Clone, Debug)]
pub struct ColorSpace {
    pub name: String,
    pub aliases: Vec<String>,
    pub family: String,
    pub encoding: String,
    pub description: String,
    pub isdata: bool,
    pub reference: Reference,
    pub to_reference: Option<Transform>,
    pub from_reference: Option<Transform>,
}

impl ColorSpace {
    /// Whether this space holds linear light (by its `encoding`, or a name
    /// such as "Linear …" or "ACEScg" when the config predates encodings).
    pub fn is_linear(&self) -> bool {
        if !self.encoding.is_empty() {
            return self.encoding.contains("linear");
        }
        let n = self.name.to_ascii_lowercase();
        n.contains("linear")
            || n.starts_with("lin_")
            || n.contains("acescg")
            || n.contains("aces2065")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub name: String,
    /// A plain view: convert to this colour space.
    pub colorspace: Option<String>,
    /// A v2 view: scene → display reference through this view transform,
    /// then to `display_colorspace`.
    pub view_transform: Option<String>,
    pub display_colorspace: Option<String>,
    pub looks: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Display {
    pub name: String,
    pub views: Vec<View>,
}

#[derive(Clone, Debug)]
pub struct ViewTransform {
    pub name: String,
    pub description: String,
    pub from_scene: Option<Transform>,
    pub to_scene: Option<Transform>,
    pub from_display: Option<Transform>,
    pub to_display: Option<Transform>,
}

#[derive(Clone, Debug)]
pub struct Look {
    pub name: String,
    pub process_space: String,
    pub description: String,
    pub transform: Option<Transform>,
    pub inverse_transform: Option<Transform>,
}

pub struct Config {
    pub version: u32,
    pub name: String,
    pub description: String,
    /// The directory relative search paths and LUTs resolve against.
    pub working_dir: PathBuf,
    pub search_paths: Vec<String>,
    /// Defaults for `$VAR` in paths; the process environment wins.
    pub environment: BTreeMap<String, String>,
    pub roles: BTreeMap<String, String>,
    pub colorspaces: Vec<ColorSpace>,
    pub displays: Vec<Display>,
    pub active_displays: Vec<String>,
    pub active_views: Vec<String>,
    pub inactive_colorspaces: Vec<String>,
    pub view_transforms: Vec<ViewTransform>,
    pub default_view_transform: Option<String>,
    pub looks: Vec<Look>,
    /// Ops of each LUT file read so far, by resolved path.
    pub(crate) files: Mutex<HashMap<PathBuf, Arc<Vec<super::ops::Op>>>>,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("name", &self.name)
            .field("version", &self.version)
            .field("colorspaces", &self.colorspaces.len())
            .field("displays", &self.displays.len())
            .finish()
    }
}

/// The token a v2 view uses for "the colour space named like the display".
pub const USE_DISPLAY_NAME: &str = "<USE_DISPLAY_NAME>";

fn list(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Tagged(t)) => list(Some(&t.value)),
        Some(Value::Sequence(items)) => items.iter().filter_map(text).collect(),
        Some(v) => text(v)
            .map(|s| {
                s.split(',')
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty())
                    .collect()
            })
            .unwrap_or_default(),
        None => Vec::new(),
    }
}

fn opt_transform(map: &serde_yaml::Mapping, key: &str, what: &str) -> Result<Option<Transform>> {
    get(map, key)
        .map(|v| transform::parse(v).map_err(|e| Error::Config(format!("{what}, {key}: {e}"))))
        .transpose()
}

fn parse_view(value: &Value) -> Result<View> {
    let map = mapping(value).ok_or_else(|| Error::Config("a view must be a mapping".into()))?;
    let name = get_text(map, "name").ok_or_else(|| Error::Config("a view has no name".into()))?;
    let view = View {
        colorspace: get_text(map, "colorspace"),
        view_transform: get_text(map, "view_transform"),
        display_colorspace: get_text(map, "display_colorspace"),
        looks: get_text(map, "looks")
            .or_else(|| get_text(map, "look"))
            .unwrap_or_default(),
        description: get_text(map, "description").unwrap_or_default(),
        name,
    };
    if view.colorspace.is_none() && view.view_transform.is_none() {
        return Err(Error::Config(format!(
            "view “{}” names no colour space",
            view.name
        )));
    }
    Ok(view)
}

fn parse_colorspace(value: &Value, reference: Reference, version: u32) -> Result<ColorSpace> {
    let map =
        mapping(value).ok_or_else(|| Error::Config("a colour space must be a mapping".into()))?;
    let name =
        get_text(map, "name").ok_or_else(|| Error::Config("a colour space has no name".into()))?;
    let what = format!("colour space “{name}”");
    let (to_key, from_key) = match reference {
        Reference::Display => ("to_display_reference", "from_display_reference"),
        Reference::Scene => ("to_scene_reference", "from_scene_reference"),
    };
    let mut to = opt_transform(map, to_key, &what)?;
    let mut from = opt_transform(map, from_key, &what)?;
    if reference == Reference::Scene || version < 2 {
        if to.is_none() {
            to = opt_transform(map, "to_reference", &what)?;
        }
        if from.is_none() {
            from = opt_transform(map, "from_reference", &what)?;
        }
    }
    Ok(ColorSpace {
        aliases: list(get(map, "aliases")),
        family: get_text(map, "family").unwrap_or_default(),
        encoding: get_text(map, "encoding").unwrap_or_default(),
        description: get_text(map, "description")
            .unwrap_or_default()
            .trim()
            .to_string(),
        isdata: get(map, "isdata")
            .and_then(transform::boolean)
            .unwrap_or(false),
        reference,
        to_reference: to,
        from_reference: from,
        name,
    })
}

impl Config {
    /// Parse config YAML; relative paths resolve against `working_dir`.
    pub fn parse(yaml: &str, working_dir: &Path) -> Result<Config> {
        let doc = transform::parse_yaml(yaml)?;
        let root =
            mapping(&doc).ok_or_else(|| Error::Config("the config is not a mapping".into()))?;
        let version_text = get_text(root, "ocio_profile_version")
            .ok_or_else(|| Error::Config("missing ocio_profile_version".into()))?;
        let version: u32 = version_text
            .split('.')
            .next()
            .and_then(|v| v.trim().parse().ok())
            .ok_or_else(|| Error::Config(format!("bad ocio_profile_version “{version_text}”")))?;
        if !(1..=2).contains(&version) {
            return Err(Error::Unsupported(format!(
                "config profile version {version_text}"
            )));
        }
        let environment = get(root, "environment")
            .and_then(mapping)
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| Some((text(k)?, text(v).unwrap_or_default())))
                    .collect()
            })
            .unwrap_or_default();
        let search_paths = match get(root, "search_path").or_else(|| get(root, "resource_path")) {
            Some(Value::Sequence(items)) => items.iter().filter_map(text).collect(),
            Some(v) => split_search_path(&text(v).unwrap_or_default()),
            None => Vec::new(),
        };
        let roles = get(root, "roles")
            .and_then(mapping)
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| Some((text(k)?.to_ascii_lowercase(), text(v)?)))
                    .collect()
            })
            .unwrap_or_default();
        let mut colorspaces = Vec::new();
        for (key, reference) in [
            ("colorspaces", Reference::Scene),
            ("display_colorspaces", Reference::Display),
        ] {
            if let Some(Value::Sequence(items)) = get(root, key) {
                for item in items {
                    colorspaces.push(parse_colorspace(item, reference, version)?);
                }
            }
        }
        let mut shared = Vec::new();
        if let Some(Value::Sequence(items)) = get(root, "shared_views") {
            for item in items {
                shared.push(parse_view(item)?);
            }
        }
        let mut displays = Vec::new();
        if let Some(m) = get(root, "displays").and_then(mapping) {
            for (name, views) in m {
                let name = text(name).unwrap_or_default();
                let mut out = Vec::new();
                if let Value::Sequence(items) = views {
                    for item in items {
                        let tag = match item {
                            Value::Tagged(t) => tag_name(&t.tag),
                            _ => "View".into(),
                        };
                        if tag == "Views" {
                            for wanted in list(Some(item)) {
                                let view = shared.iter().find(|v| v.name == wanted).ok_or_else(|| {
                                    Error::Config(format!(
                                        "display “{name}” uses shared view “{wanted}”, which is not defined"
                                    ))
                                })?;
                                out.push(view.clone());
                            }
                        } else {
                            out.push(parse_view(item)?);
                        }
                    }
                }
                displays.push(Display { name, views: out });
            }
        }
        let mut view_transforms = Vec::new();
        if let Some(Value::Sequence(items)) = get(root, "view_transforms") {
            for item in items {
                let map = mapping(item)
                    .ok_or_else(|| Error::Config("a view transform must be a mapping".into()))?;
                let name = get_text(map, "name")
                    .ok_or_else(|| Error::Config("a view transform has no name".into()))?;
                let what = format!("view transform “{name}”");
                view_transforms.push(ViewTransform {
                    description: get_text(map, "description").unwrap_or_default(),
                    from_scene: opt_transform(map, "from_scene_reference", &what)?
                        .or(opt_transform(map, "from_reference", &what)?),
                    to_scene: opt_transform(map, "to_scene_reference", &what)?.or(opt_transform(
                        map,
                        "to_reference",
                        &what,
                    )?),
                    from_display: opt_transform(map, "from_display_reference", &what)?,
                    to_display: opt_transform(map, "to_display_reference", &what)?,
                    name,
                });
            }
        }
        let mut looks = Vec::new();
        if let Some(Value::Sequence(items)) = get(root, "looks") {
            for item in items {
                let map = mapping(item)
                    .ok_or_else(|| Error::Config("a look must be a mapping".into()))?;
                let name = get_text(map, "name")
                    .ok_or_else(|| Error::Config("a look has no name".into()))?;
                let what = format!("look “{name}”");
                looks.push(Look {
                    process_space: get_text(map, "process_space")
                        .ok_or_else(|| Error::Config(format!("{what} has no process_space")))?,
                    description: get_text(map, "description").unwrap_or_default(),
                    transform: opt_transform(map, "transform", &what)?,
                    inverse_transform: opt_transform(map, "inverse_transform", &what)?,
                    name,
                });
            }
        }
        let config = Config {
            version,
            name: get_text(root, "name").unwrap_or_default(),
            description: get_text(root, "description")
                .unwrap_or_default()
                .trim()
                .to_string(),
            working_dir: working_dir.to_path_buf(),
            search_paths,
            environment,
            roles,
            colorspaces,
            displays,
            active_displays: list(get(root, "active_displays")),
            active_views: list(get(root, "active_views")),
            inactive_colorspaces: list(get(root, "inactive_colorspaces")),
            view_transforms,
            default_view_transform: get_text(root, "default_view_transform"),
            looks,
            files: Mutex::new(HashMap::new()),
        };
        config.check()?;
        Ok(config)
    }

    /// Read a config file; its directory is the working directory.
    pub fn from_file(path: &Path) -> Result<Config> {
        let meta = std::fs::metadata(path)
            .map_err(|e| Error::Config(format!("{}: {e}", path.display())))?;
        if meta.len() > 16 * 1024 * 1024 {
            return Err(Error::Config(format!(
                "{}: larger than 16 MiB",
                path.display()
            )));
        }
        let yaml = std::fs::read_to_string(path)
            .map_err(|e| Error::Config(format!("{}: {e}", path.display())))?;
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        Config::parse(&yaml, &dir).map_err(|e| match e {
            Error::Config(m) => Error::Config(format!("{}: {m}", path.display())),
            other => other,
        })
    }

    /// The config `$OCIO` names, if it is set.
    pub fn from_env() -> Option<Result<Config>> {
        let path = std::env::var_os("OCIO").filter(|p| !p.is_empty())?;
        Some(Config::from_file(Path::new(&path)))
    }

    /// Emulsion's built-in config (see [`super::BUILTIN_CONFIG`]).
    pub fn builtin() -> Config {
        Config::parse(super::BUILTIN_CONFIG, Path::new(""))
            .expect("the built-in OCIO config parses")
    }

    /// References that must resolve for the config to be usable.
    fn check(&self) -> Result<()> {
        for (role, cs) in &self.roles {
            if self.colorspace(cs).is_none() {
                return Err(Error::Config(format!(
                    "role “{role}” names colour space “{cs}”, which is not defined"
                )));
            }
        }
        for d in &self.displays {
            for v in &d.views {
                if let Some(vt) = &v.view_transform
                    && self.view_transform(vt).is_none()
                {
                    return Err(Error::Config(format!(
                        "view “{}” uses view transform “{vt}”, which is not defined",
                        v.name
                    )));
                }
            }
        }
        Ok(())
    }

    /// A colour space by name, alias or role (names ignore case).
    pub fn colorspace(&self, name: &str) -> Option<&ColorSpace> {
        let by_name = |n: &str| {
            self.colorspaces
                .iter()
                .find(|c| c.name == n)
                .or_else(|| {
                    self.colorspaces
                        .iter()
                        .find(|c| c.name.eq_ignore_ascii_case(n))
                })
                .or_else(|| {
                    self.colorspaces
                        .iter()
                        .find(|c| c.aliases.iter().any(|a| a.eq_ignore_ascii_case(n)))
                })
        };
        by_name(name).or_else(|| {
            self.roles
                .get(&name.to_ascii_lowercase())
                .and_then(|cs| by_name(cs))
        })
    }

    pub fn role(&self, role: &str) -> Option<&str> {
        self.roles
            .get(&role.to_ascii_lowercase())
            .map(String::as_str)
    }

    pub fn view_transform(&self, name: &str) -> Option<&ViewTransform> {
        self.view_transforms
            .iter()
            .find(|v| v.name.eq_ignore_ascii_case(name))
    }

    /// The view transform that links scene- and display-referred spaces.
    pub fn default_view_transform(&self) -> Option<&ViewTransform> {
        self.default_view_transform
            .as_deref()
            .and_then(|n| self.view_transform(n))
            .or_else(|| {
                self.view_transforms
                    .iter()
                    .find(|v| v.from_scene.is_some() || v.to_scene.is_some())
            })
    }

    pub fn look(&self, name: &str) -> Option<&Look> {
        self.looks
            .iter()
            .find(|l| l.name.eq_ignore_ascii_case(name))
    }

    /// Colour spaces a person should be offered: active ones, scene-referred
    /// first.
    pub fn active_colorspaces(&self) -> Vec<&ColorSpace> {
        let inactive = env_list("OCIO_INACTIVE_COLORSPACES")
            .unwrap_or_else(|| self.inactive_colorspaces.clone());
        self.colorspaces
            .iter()
            .filter(|c| !inactive.iter().any(|i| i.eq_ignore_ascii_case(&c.name)))
            .collect()
    }

    /// Displays in the order a person should see them (honouring
    /// `active_displays` and `$OCIO_ACTIVE_DISPLAYS`).
    pub fn active_displays(&self) -> Vec<&Display> {
        let active =
            env_list("OCIO_ACTIVE_DISPLAYS").unwrap_or_else(|| self.active_displays.clone());
        ordered(&self.displays, &active, |d| &d.name)
    }

    pub fn display(&self, name: &str) -> Option<&Display> {
        self.displays
            .iter()
            .find(|d| d.name.eq_ignore_ascii_case(name))
    }

    /// The views of `display` to offer (honouring `active_views`).
    pub fn active_views(&self, display: &str) -> Vec<&View> {
        let Some(d) = self.display(display) else {
            return Vec::new();
        };
        let active = env_list("OCIO_ACTIVE_VIEWS").unwrap_or_else(|| self.active_views.clone());
        let views = ordered(&d.views, &active, |v| &v.name);
        if views.is_empty() {
            d.views.iter().collect()
        } else {
            views
        }
    }

    pub fn default_display(&self) -> Option<&Display> {
        self.active_displays()
            .into_iter()
            .next()
            .or(self.displays.first())
    }

    pub fn default_view(&self, display: &str) -> Option<&View> {
        self.active_views(display).into_iter().next()
    }

    /// `$NAME` and `${NAME}` from the process environment, falling back to
    /// the config's `environment` defaults.
    pub fn expand(&self, s: &str) -> String {
        let mut out = String::new();
        let mut rest = s;
        while let Some(i) = rest.find('$') {
            out.push_str(&rest[..i]);
            let after = &rest[i + 1..];
            let (name, len) = if let Some(braced) = after.strip_prefix('{') {
                match braced.find('}') {
                    Some(end) => (&braced[..end], end + 2),
                    None => ("", 0),
                }
            } else {
                let end = after
                    .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .unwrap_or(after.len());
                (&after[..end], end)
            };
            if name.is_empty() {
                out.push('$');
                rest = after;
                continue;
            }
            let value = std::env::var(name)
                .ok()
                .or_else(|| self.environment.get(name).cloned());
            match value {
                Some(v) => out.push_str(&v),
                None => out.push_str(&rest[i..i + 1 + len]),
            }
            rest = &after[len..];
        }
        out.push_str(rest);
        out
    }

    /// Where a FileTransform's `src` is: absolute, or the first search path
    /// directory that has it.
    pub fn resolve_file(&self, src: &str) -> Result<PathBuf> {
        let src = self.expand(src);
        let path = Path::new(&src);
        if path.is_absolute() {
            return if path.is_file() {
                Ok(path.to_path_buf())
            } else {
                Err(Error::NotFound(format!(
                    "LUT file {} does not exist",
                    path.display()
                )))
            };
        }
        let dirs: Vec<PathBuf> = if self.search_paths.is_empty() {
            vec![self.working_dir.clone()]
        } else {
            self.search_paths
                .iter()
                .map(|p| {
                    let p = PathBuf::from(self.expand(p));
                    if p.is_absolute() {
                        p
                    } else {
                        self.working_dir.join(p)
                    }
                })
                .collect()
        };
        dirs.iter()
            .map(|d| d.join(path))
            .find(|p| p.is_file())
            .ok_or_else(|| {
                Error::NotFound(format!(
                    "LUT file “{src}” is not in the search path ({})",
                    dirs.iter()
                        .map(|d| d.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })
    }
}

fn env_list(var: &str) -> Option<Vec<String>> {
    let v = std::env::var(var).ok().filter(|v| !v.trim().is_empty())?;
    Some(
        v.split([',', ':'])
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
    )
}

fn ordered<'a, T>(items: &'a [T], active: &[String], name: impl Fn(&T) -> &String) -> Vec<&'a T> {
    if active.is_empty() {
        return items.iter().collect();
    }
    active
        .iter()
        .filter_map(|a| items.iter().find(|i| name(i).eq_ignore_ascii_case(a)))
        .collect()
}

/// `a:b:c`, keeping Windows drive letters (`C:/luts`) whole.
fn split_search_path(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for part in s.split(':') {
        if let Some(last) = out.last_mut()
            && last.len() == 1
            && last.chars().all(|c| c.is_ascii_alphabetic())
            && (part.starts_with('/') || part.starts_with('\\'))
        {
            last.push(':');
            last.push_str(part);
            continue;
        }
        out.push(part.to_string());
    }
    out.retain(|p| !p.trim().is_empty());
    out
}
