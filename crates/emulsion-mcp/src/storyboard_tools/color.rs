//! Colour management: read the ICC/OpenColorIO settings with the config's
//! colour spaces, displays, views and looks, and change them, including the
//! storyboard's own working colour space (one Undo step).
use super::def;
use crate::ToolDef;
use emulsion_core::project::ProjectEditor;
use emulsion_core::storyboard::Storyboard;
use emulsion_io::color_management::{self, ColorManagement, ConfigSource};
use serde_json::{Value, json};

pub(super) fn definitions() -> Vec<ToolDef> {
    let name = |what: &str| json!({"type":["string","null"],"maxLength":200,"description":format!("{what} from describe_color_management; null for the default.")});
    vec![
        def(
            "describe_color_management",
            "Read colour management: whether OpenColorIO is on (otherwise ICC, where imports are converted to sRGB), the config (built-in ACES, $OCIO or a file) with its colour spaces (family, encoding, scene or display reference), displays and their views, looks and roles; the default working colour space and this storyboard's own; the display, view and look the canvas and player show; the colour space exports are written in; and what these resolve to, with any error.",
            json!({}),
            &[],
        ),
        def(
            "set_ocio_config",
            "Change colour management. `enabled` turns OpenColorIO on or off (off keeps ICC behaviour exactly). `config` is \"builtin\", \"env\" ($OCIO) or an absolute path to a .ocio file. `working_colorspace` is the default for documents without their own; `project_working_colorspace` sets this storyboard's (one Undo step, null clears it). `display`, `view` and `look` choose what the canvas, Stage and player show (look \"\" turns the view's looks off, null uses them). `export_colorspace` is the colour space image, movie and PDF exports are written in (null: as displayed). Every name is checked against the config; nothing changes if one is wrong.",
            json!({
                "enabled":{"type":"boolean"},
                "config":{"type":"string","minLength":1,"maxLength":4096},
                "working_colorspace":name("Colour space"),
                "project_working_colorspace":name("Colour space"),
                "display":name("Display"),
                "view":name("View"),
                "look":{"type":["string","null"],"maxLength":400,"description":"Look name(s), comma-separated, \"\" for none, null for the view's own."},
                "export_colorspace":name("Colour space")
            }),
            &[],
        ),
    ]
}

/// Apply the settings arguments to `c`.
fn apply(c: &mut ColorManagement, args: &Value) -> Result<(), String> {
    if let Some(on) = args.get("enabled").and_then(Value::as_bool) {
        c.ocio = on;
    }
    if let Some(config) = args.get("config").and_then(Value::as_str) {
        c.switch_config(match config {
            "builtin" | "built-in" => ConfigSource::Builtin,
            "env" | "$OCIO" => ConfigSource::Environment,
            path => {
                let path = std::path::PathBuf::from(path);
                if !path.is_absolute() {
                    return Err("config must be \"builtin\", \"env\" or an absolute path".into());
                }
                ConfigSource::File(path)
            }
        });
    }
    let text = |key: &str| -> Option<Option<String>> {
        args.get(key).map(|v| v.as_str().map(str::to_string))
    };
    if let Some(v) = text("working_colorspace") {
        c.working = v;
    }
    if let Some(v) = text("display") {
        c.display = v;
        if args.get("view").is_none() {
            c.view = None;
        }
    }
    if let Some(v) = text("view") {
        c.view = v;
    }
    if let Some(v) = text("look") {
        c.look = v;
    }
    if let Some(v) = text("export_colorspace") {
        c.export_colorspace = v;
    }
    Ok(())
}

pub(super) fn run(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    name: &str,
    args: &Value,
) -> Option<Result<Value, String>> {
    let result = match name {
        "describe_color_management" => Ok(color_management::describe(
            &color_management::current(),
            board.working_colorspace.as_deref(),
        )),
        "set_ocio_config" => set(editor, board, args),
        _ => return None,
    };
    Some(result)
}

fn set(editor: &mut ProjectEditor, board: &Storyboard, args: &Value) -> Result<Value, String> {
    let mut next = color_management::current();
    apply(&mut next, args)?;
    let project = match args.get("project_working_colorspace") {
        Some(v) => v.as_str().map(str::to_string),
        None => board.working_colorspace.clone(),
    };
    // Check everything before changing anything.
    next.resolve(project.as_deref())?;
    if project != board.working_colorspace {
        if let Some(name) = &project {
            // The project's space must exist even while OpenColorIO is off.
            let config = color_management::load_config(&next.config)?;
            if config.colorspace(name).is_none() {
                return Err(format!(
                    "The colour space “{name}” is not in the OpenColorIO config"
                ));
            }
        }
        editor.edit_storyboard(|b| {
            b.working_colorspace = project.clone();
            Ok(())
        })?;
    }
    let saved = color_management::update(|c| *c = next)?;
    Ok(color_management::describe(&saved, project.as_deref()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_change_the_settings_and_are_checked() {
        let mut c = ColorManagement::default();
        apply(
            &mut c,
            &json!({"enabled":true,"display":"sRGB - Display","view":"Un-tone-mapped","export_colorspace":"ACEScct"}),
        )
        .unwrap();
        assert!(c.ocio);
        assert_eq!(c.view.as_deref(), Some("Un-tone-mapped"));
        let r = c.resolve(Some("ACEScg")).unwrap().unwrap();
        assert_eq!(
            (r.working.as_str(), r.export_label()),
            ("ACEScg", "ACEScct".to_string())
        );
        apply(&mut c, &json!({"display":"Rec.1886 Rec.709 - Display"})).unwrap();
        assert_eq!(c.view, None, "a new display starts at its default view");
        apply(&mut c, &json!({"look":"", "export_colorspace":null})).unwrap();
        assert_eq!(
            (c.look.as_deref(), c.export_colorspace.as_deref()),
            (Some(""), None)
        );
        assert!(apply(&mut c, &json!({"config":"relative.ocio"})).is_err());
        apply(&mut c, &json!({"view":"Nope"})).unwrap();
        assert!(c.resolve(None).unwrap_err().contains("Nope"));
    }

    #[test]
    fn the_project_keeps_its_working_space_with_undo() {
        let mut e = super::super::tests::board();
        let board = e.storyboard().unwrap().clone();
        // The process-wide settings start off (nothing is read from disk in
        // tests); only the project's colour space is changed here.
        let out = set(
            &mut e,
            &board,
            &json!({"project_working_colorspace":"ACEScct"}),
        )
        .unwrap();
        assert_eq!(out["project_working_colorspace"], "ACEScct");
        assert_eq!(
            e.storyboard().unwrap().working_colorspace.as_deref(),
            Some("ACEScct")
        );
        let board = e.storyboard().unwrap().clone();
        assert!(
            set(
                &mut e,
                &board,
                &json!({"project_working_colorspace":"Nope"})
            )
            .is_err()
        );
        assert_eq!(
            e.storyboard().unwrap().working_colorspace.as_deref(),
            Some("ACEScct")
        );
        let described = run(&mut e, &board, "describe_color_management", &json!({}))
            .unwrap()
            .unwrap();
        assert_eq!(described["project_working_colorspace"], "ACEScct");
        assert!(
            described["displays"]
                .as_array()
                .is_some_and(|d| !d.is_empty())
        );
        e.undo();
        assert_eq!(e.storyboard().unwrap().working_colorspace, None);
    }
}
