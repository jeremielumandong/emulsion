//! Settings › Color management: ICC (as always) or OpenColorIO with a
//! config (built-in, `$OCIO` or a file), the default and project working
//! colour spaces, the display, view and look the canvas and player show,
//! and the colour space exports are written in. Every change is checked
//! against the config before it is kept.
use crate::file_prompt::FilePrompts;
use crate::settings_storyboard::matches;
use crate::theme::Palette;
use crate::widgets::{chip, mono};
use crate::workspace::Workspace;
use emulsion_io::color_management::{self, ColorManagement, ConfigSource};
use gpui_kit::*;
use std::cell::RefCell;

thread_local! {
    /// The last refusal, shown under the section until the next change.
    static NOTE: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn change(cx: &mut App, edit: impl FnOnce(&mut ColorManagement)) {
    let note = color_management::update(edit).err();
    NOTE.with(|n| *n.borrow_mut() = note);
    cx.refresh_windows();
}

impl Workspace {
    fn project_working(&self, cx: &App) -> Option<Option<String>> {
        let editor = self.editor.as_ref()?.read(cx);
        let board = editor.editor.storyboard()?;
        Some(board.working_colorspace.clone())
    }

    fn choose_ocio_config(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_open_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose an OpenColorIO config".into()),
        });
        cx.spawn_in(window, async move |_, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            cx.update(|_, cx| {
                change(cx, |c| {
                    c.switch_config(ConfigSource::File(path));
                    c.ocio = true;
                })
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn color_settings(
        &mut self,
        p: &Palette,
        query: &str,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let words = "Color colour management OpenColorIO OCIO ACES ICC working colour space display view look export config";
        if !matches(query, words) {
            return None;
        }
        let saved = color_management::current();
        let project = self.project_working(cx);
        let row = |label: &'static str, content: Div| {
            div()
                .flex()
                .items_start()
                .gap(px(10.))
                .child(mono(label, 10., p.muted).w(px(170.)).flex_none().pt(px(4.)))
                .child(content.flex().flex_wrap().gap(px(6.)).max_w(px(760.)))
        };
        let pick =
            |id: String, label: String, on: bool, edit: Box<dyn Fn(&mut ColorManagement)>| {
                chip(SharedString::from(id), label, on, p)
                    .test_support()
                    .on_click(move |_, _, cx| change(cx, |c| edit(c)))
            };
        let mut rows: Vec<AnyElement> = Vec::new();
        rows.push(
            row(
                "colour management",
                div()
                    .child(pick(
                        "settings-color-icc".into(),
                        "ICC (OpenColorIO off)".into(),
                        !saved.ocio,
                        Box::new(|c| c.ocio = false),
                    ))
                    .child(pick(
                        "settings-color-ocio".into(),
                        "OpenColorIO".into(),
                        saved.ocio,
                        Box::new(|c| c.ocio = true),
                    )),
            )
            .into_any_element(),
        );
        let resolved = saved.resolve(project.clone().flatten().as_deref());
        if saved.ocio {
            let env = std::env::var("OCIO").ok().filter(|v| !v.is_empty());
            let file = match &saved.config {
                ConfigSource::File(path) => Some(path.display().to_string()),
                _ => None,
            };
            rows.push(
                row(
                    "config",
                    div()
                        .child(pick(
                            "settings-color-config-builtin".into(),
                            "Built-in ACES".into(),
                            saved.config == ConfigSource::Builtin,
                            Box::new(|c| c.switch_config(ConfigSource::Builtin)),
                        ))
                        .child(pick(
                            "settings-color-config-env".into(),
                            match &env {
                                Some(path) => format!("$OCIO · {path}"),
                                None => "$OCIO (not set)".into(),
                            },
                            saved.config == ConfigSource::Environment,
                            Box::new(|c| c.switch_config(ConfigSource::Environment)),
                        ))
                        .child(
                            chip(
                                "settings-color-config-file",
                                file.map_or("File…".to_string(), |f| format!("File · {f}")),
                                matches!(saved.config, ConfigSource::File(_)),
                                p,
                            )
                            .test_support()
                            .on_click(cx.listener(
                                |this, _, window, cx| this.choose_ocio_config(window, cx),
                            )),
                        ),
                )
                .into_any_element(),
            );
            if let Ok(config) = color_management::load_config(&saved.config) {
                let current = resolved.as_ref().ok().and_then(Option::as_ref);
                let display = current.map(|r| r.display.clone()).unwrap_or_default();
                let view = current.map(|r| r.view.clone()).unwrap_or_default();
                let mut displays = div();
                for d in config.active_displays() {
                    let name = d.name.clone();
                    displays = displays.child(pick(
                        format!("settings-color-display-{name}"),
                        name.clone(),
                        name == display,
                        Box::new(move |c| {
                            c.display = Some(name.clone());
                            c.view = None;
                        }),
                    ));
                }
                rows.push(row("display", displays).into_any_element());
                let mut views = div();
                for v in config.active_views(&display) {
                    let name = v.name.clone();
                    views = views.child(pick(
                        format!("settings-color-view-{name}"),
                        name.clone(),
                        name == view,
                        Box::new(move |c| c.view = Some(name.clone())),
                    ));
                }
                rows.push(row("view", views).into_any_element());
                let mut looks = div()
                    .child(pick(
                        "settings-color-look-view".into(),
                        "The view's own".into(),
                        saved.look.is_none(),
                        Box::new(|c| c.look = None),
                    ))
                    .child(pick(
                        "settings-color-look-none".into(),
                        "None".into(),
                        saved.look.as_deref() == Some(""),
                        Box::new(|c| c.look = Some(String::new())),
                    ));
                for l in &config.looks {
                    let name = l.name.clone();
                    looks = looks.child(pick(
                        format!("settings-color-look-{name}"),
                        name.clone(),
                        saved.look.as_deref() == Some(name.as_str()),
                        Box::new(move |c| c.look = Some(name.clone())),
                    ));
                }
                rows.push(row("look", looks).into_any_element());
                let spaces: Vec<String> = config
                    .active_colorspaces()
                    .iter()
                    .filter(|c| {
                        c.reference == color_management::ocio::Reference::Scene && !c.isdata
                    })
                    .map(|c| c.name.clone())
                    .collect();
                let default_working = current.map(|r| r.working.clone());
                let mut working = div().child(pick(
                    "settings-color-working-role".into(),
                    "From the config's roles".into(),
                    saved.working.is_none(),
                    Box::new(|c| c.working = None),
                ));
                for name in &spaces {
                    let n = name.clone();
                    working = working.child(pick(
                        format!("settings-color-working-{name}"),
                        name.clone(),
                        saved.working.as_deref() == Some(name.as_str()),
                        Box::new(move |c| c.working = Some(n.clone())),
                    ));
                }
                rows.push(row("working colour space", working).into_any_element());
                if let Some(own) = project {
                    let editor = self.editor.clone();
                    let mut chips = div().child(
                        chip(
                            "settings-color-project-default",
                            "Use the default",
                            own.is_none(),
                            p,
                        )
                        .test_support()
                        .on_click({
                            let editor = editor.clone();
                            move |_, _, cx| set_project_working(&editor, None, cx)
                        }),
                    );
                    for name in &spaces {
                        let n = name.clone();
                        let editor = editor.clone();
                        chips = chips.child(
                            chip(
                                SharedString::from(format!("settings-color-project-{name}")),
                                name.clone(),
                                own.as_deref() == Some(name.as_str()),
                                p,
                            )
                            .test_support()
                            .on_click(move |_, _, cx| {
                                set_project_working(&editor, Some(n.clone()), cx)
                            }),
                        );
                    }
                    rows.push(row("this storyboard", chips).into_any_element());
                }
                let mut export = div().child(pick(
                    "settings-color-export-view".into(),
                    "As displayed (display and view)".into(),
                    saved.export_colorspace.is_none(),
                    Box::new(|c| c.export_colorspace = None),
                ));
                for cs in config.active_colorspaces() {
                    if cs.isdata {
                        continue;
                    }
                    let name = cs.name.clone();
                    export = export.child(pick(
                        format!("settings-color-export-{name}"),
                        name.clone(),
                        saved.export_colorspace.as_deref() == Some(name.as_str()),
                        Box::new(move |c| c.export_colorspace = Some(name.clone())),
                    ));
                }
                rows.push(row("exports in", export).into_any_element());
                if let Some(working) = default_working {
                    rows.push(
                        mono(
                            format!("Pixels are read as {working}; the canvas and player show {display} / {view}."),
                            10.5,
                            p.ink,
                        )
                        .into_any_element(),
                    );
                }
            }
        }
        let problem = match &resolved {
            Err(e) => Some(e.clone()),
            Ok(_) => NOTE.with(|n| n.borrow().clone()),
        };
        Some(
            div()
                .id("settings-color")
                .test_support()
                .flex()
                .flex_col()
                .gap(px(10.))
                .px(px(40.))
                .py(px(24.))
                .border_b_1()
                .border_color(p.line)
                .child(
                    div()
                        .text_size(px(17.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Color management"),
                )
                .child(
                    div()
                        .max_w(px(640.))
                        .text_size(px(13.))
                        .text_color(p.muted)
                        .child("ICC profiles convert imported pictures to sRGB, as always. With OpenColorIO on, the canvas, the Stage and the animatic player show pixels through an OCIO display and view (ACES by default), and image, movie and PDF exports are written in the display's colours or a colour space you choose. Turn it off to go back to ICC behaviour exactly."),
                )
                .children(rows)
                .children(problem.map(|message| {
                    div()
                        .id("settings-color-message")
                        .test_support()
                        .child(mono(message, 10.5, p.accent))
                }))
                .into_any_element(),
        )
    }
}

fn set_project_working(
    editor: &Option<Entity<crate::editor::EditorView>>,
    name: Option<String>,
    cx: &mut App,
) {
    let Some(editor) = editor else {
        return;
    };
    editor.update(cx, |e, cx| {
        e.edit_board(
            |b| {
                b.working_colorspace = name;
                Ok(())
            },
            cx,
        );
    });
    cx.refresh_windows();
}

#[cfg(test)]
mod tests {
    use crate::actions;
    use crate::tests::open;
    use ::core::prelude::v1::test;
    use emulsion_core::Document;
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::*;

    #[gpui_kit::test]
    fn the_color_section_is_found_and_starts_with_icc(cx: &mut TestAppContext) {
        let (_ws, cx) = open(cx, Document::new(32, 32));
        cx.simulate_resize(size(px(1600.), px(4000.)));
        cx.update(|window, _| window.activate_window());
        cx.update(|window, cx| window.dispatch_action(Box::new(actions::ShowSettings), cx));
        cx.run_until_parked();
        cx.update(|window, cx| window.render_frame(cx));
        let present = |cx: &mut VisualTestContext, id: &'static str| {
            cx.update(|window, _| window.try_find(id).is_some())
        };
        assert!(present(cx, "settings-color-section"));
        // Off by default: only the mode choice shows, and nothing is
        // reported wrong.
        assert!(present(cx, "settings-color-icc"));
        assert!(present(cx, "settings-color-ocio"));
        assert!(!present(cx, "settings-color-config-builtin"));
        assert!(!present(cx, "settings-color-message"));
        cx.update(|window, cx| window.click("settings-search", cx));
        cx.simulate_input("opencolorio");
        cx.run_until_parked();
        cx.update(|window, cx| window.render_frame(cx));
        assert!(present(cx, "settings-color-section"));
        assert!(!present(cx, "settings-storyboard-section"));
    }
}
