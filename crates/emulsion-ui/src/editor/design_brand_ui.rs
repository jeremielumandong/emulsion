//! Portable brand typography, palette targeting and native asset folders.
use super::*;
use emulsion_core::design_brand_assets::{self as brand, ColorTarget, TypographyRole};
use emulsion_io::creative_library::Brand;
use gpui_kit::component::{
    Sizable, WindowExt,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    menu::{DropdownMenu, PopupMenuItem},
};
use std::cell::Cell;
fn rgba_text(c: [u8; 4]) -> String {
    format!("#{:02X}{:02X}{:02X}{:02X}", c[0], c[1], c[2], c[3])
}
pub(super) fn parse_rgba(value: &str) -> Option<[u8; 4]> {
    let value = value.trim().trim_start_matches('#');
    let n = u32::from_str_radix(value, 16).ok()?;
    match value.len() {
        6 => Some([(n >> 16) as u8, (n >> 8) as u8, n as u8, 255]),
        8 => Some(n.to_be_bytes()),
        _ => None,
    }
}
fn field(label: &'static str, input: &Entity<InputState>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(label)
        .child(Input::new(input).id(label))
}
impl EditorView {
    pub(super) fn brand_extended_controls(
        &self,
        brand: &Brand,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = brand.id;
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                Button::new(("brand-embed-font", id))
                    .label("Import portable font…")
                    .small()
                    .outline()
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.import_brand_font(Some(id), cx)),
                    ),
            )
            .child(
                Button::new(("brand-role-new", id))
                    .label("Add typography role…")
                    .small()
                    .ghost()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.edit_typography_role(id, None, window, cx)
                    })),
            )
            .children(brand.typography.keys().cloned().map(|name| {
                let apply = name.clone();
                let edit = name.clone();
                let remove = name.clone();
                let owner = cx.weak_entity();
                div()
                    .flex()
                    .gap_1()
                    .child(
                        Button::new((ElementId::from("brand-role-apply"), format!("{id}-{name}")))
                            .label(name)
                            .small()
                            .outline()
                            .flex_1()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.apply_typography_role(id, &apply, cx)
                            })),
                    )
                    .child(
                        Button::new((ElementId::from("brand-role-menu"), format!("{id}-{edit}")))
                            .label("…")
                            .small()
                            .ghost()
                            .dropdown_menu(move |menu, _, _| {
                                let a = owner.clone();
                                let b = owner.clone();
                                let edit = edit.clone();
                                let remove = remove.clone();
                                menu.item(PopupMenuItem::new("Edit role…").on_click(
                                    move |_, window, cx| {
                                        a.update(cx, |this, cx| {
                                            this.edit_typography_role(
                                                id,
                                                Some(edit.clone()),
                                                window,
                                                cx,
                                            )
                                        })
                                        .ok();
                                    },
                                ))
                                .item(
                                    PopupMenuItem::new("Remove role").on_click(move |_, _, cx| {
                                        let name = remove.clone();
                                        b.update(cx, |this, cx| {
                                            this.catalog_edit(
                                                move |c| {
                                                    if let Some(b) =
                                                        c.brands.iter_mut().find(|b| b.id == id)
                                                    {
                                                        b.typography.remove(&name);
                                                    }
                                                    Ok(())
                                                },
                                                cx,
                                            )
                                        })
                                        .ok();
                                    }),
                                )
                            }),
                    )
            }))
            .child(
                Button::new(("brand-palette-new", id))
                    .label("New palette / extract selection…")
                    .small()
                    .ghost()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.edit_brand_palette(id, None, window, cx)
                    })),
            )
            .children(brand.palettes.iter().map(|(name, colors)| {
                let colors = colors.clone();
                let edit = name.clone();
                let remove = name.clone();
                let owner = cx.weak_entity();
                div()
                    .flex()
                    .gap_1()
                    .child(
                        Button::new((
                            ElementId::from("brand-palette-apply"),
                            format!("{id}-{name}"),
                        ))
                        .label(name.clone())
                        .small()
                        .outline()
                        .flex_1()
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.palette_apply_dialog(colors.clone(), window, cx)
                            },
                        )),
                    )
                    .child(
                        Button::new((
                            ElementId::from("brand-palette-menu"),
                            format!("{id}-{name}"),
                        ))
                        .label("…")
                        .small()
                        .ghost()
                        .dropdown_menu(move |menu, _, _| {
                            let a = owner.clone();
                            let b = owner.clone();
                            let edit = edit.clone();
                            let remove = remove.clone();
                            menu.item(PopupMenuItem::new("Edit palette…").on_click(
                                move |_, window, cx| {
                                    a.update(cx, |this, cx| {
                                        this.edit_brand_palette(id, Some(edit.clone()), window, cx)
                                    })
                                    .ok();
                                },
                            ))
                            .item(
                                PopupMenuItem::new("Remove palette").on_click(move |_, _, cx| {
                                    let name = remove.clone();
                                    b.update(cx, |this, cx| {
                                        this.catalog_edit(
                                            move |c| {
                                                if let Some(b) =
                                                    c.brands.iter_mut().find(|b| b.id == id)
                                                {
                                                    b.palettes.remove(&name);
                                                }
                                                Ok(())
                                            },
                                            cx,
                                        )
                                    })
                                    .ok();
                                }),
                            )
                        }),
                    )
            }))
            .child(
                Button::new(("brand-target-color", id))
                    .label("Apply palette color to fill / stroke / text…")
                    .small()
                    .ghost()
                    .on_click({
                        let colors = brand.colors.clone();
                        cx.listener(move |this, _, window, cx| {
                            this.palette_apply_dialog(colors.clone(), window, cx)
                        })
                    }),
            )
            .children(brand.fonts.values().map(|font| {
                div().text_size(px(11.)).child(format!(
                    "Embedded: {} · {} KiB",
                    font.family(),
                    font.bytes().len().div_ceil(1024)
                ))
            }))
            .into_any_element()
    }
    pub(super) fn apply_typography_role(&mut self, id: u64, name: &str, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let Some(kit) = self
            .creative
            .catalog
            .brands
            .iter()
            .find(|b| b.id == id)
            .cloned()
        else {
            return;
        };
        let Some(role) = kit.typography.get(name) else {
            return;
        };
        let ids = self.selected_layer_roots();
        match brand::apply_role(&mut self.editor, &ids, role, &kit.fonts) {
            Ok(()) => self.after_change(cx),
            Err(e) => self.set_status(e, true, cx),
        }
    }
    pub(super) fn import_brand_font(&mut self, brand: Option<u64>, cx: &mut Context<Self>) {
        let ids = self.selected_layer_roots();
        let ticket = self.edit_ticket();
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose a local TTF or OTF font to embed".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.first().cloned() else {
                return;
            };
            let result = cx
                .background_spawn(async move {
                    use std::io::Read;
                    let mut bytes = Vec::new();
                    std::fs::File::open(path)
                        .map_err(|e| e.to_string())?
                        .take((emulsion_core::design_fonts::MAX_FONT_BYTES + 1) as u64)
                        .read_to_end(&mut bytes)
                        .map_err(|e| e.to_string())?;
                    emulsion_core::design_fonts::EmbeddedFont::from_bytes(bytes)
                })
                .await;
            this.update(cx, |this, cx| {
                let font = match result {
                    Ok(f) => f,
                    Err(e) => {
                        this.set_status(e, true, cx);
                        return;
                    }
                };
                if let Some(id) = brand {
                    this.catalog_edit(
                        move |c| {
                            let brand =
                                c.brands.iter_mut().find(|b| b.id == id).ok_or_else(|| {
                                    emulsion_io::IoError::Manifest("Brand no longer exists.".into())
                                })?;
                            brand.font = font.alias().into();
                            brand.fonts.insert(font.alias().into(), font);
                            Ok(())
                        },
                        cx,
                    );
                } else if !this.edit_is_current(ticket) {
                    this.set_status(
                        "Document changed while loading the font. Choose it again.",
                        true,
                        cx,
                    );
                } else {
                    match emulsion_core::design_fonts::embed(&mut this.editor, &ids, font) {
                        Ok(()) => this.after_change(cx),
                        Err(e) => this.set_status(e, true, cx),
                    }
                }
            })
            .ok();
        })
        .detach();
    }
    pub(super) fn edit_typography_role(
        &mut self,
        id: u64,
        old: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(kit) = self
            .creative
            .catalog
            .brands
            .iter()
            .find(|b| b.id == id)
            .cloned()
        else {
            return;
        };
        let role = old
            .as_ref()
            .and_then(|name| kit.typography.get(name))
            .cloned()
            .or_else(|| {
                self.selected
                    .and_then(|id| TypographyRole::sample(&self.editor.doc, id).ok())
            })
            .unwrap_or(TypographyRole {
                font: kit.font.clone(),
                ..Default::default()
            });
        let family = kit
            .fonts
            .get(&role.font)
            .map(|f| f.family().to_owned())
            .unwrap_or(role.font.clone());
        let fields = [
            old.clone().unwrap_or("Heading".into()),
            family,
            role.size.to_string(),
            role.line_height.to_string(),
            role.letter_spacing.to_string(),
            role.color.map(rgba_text).unwrap_or_default(),
        ]
        .map(|v| cx.new(|cx| InputState::new(window, cx).default_value(v)));
        let bold = Rc::new(Cell::new(role.bold));
        let italic = Rc::new(Cell::new(role.italic));
        let owner = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let inputs = fields.clone();
            let owner = owner.clone();
            let old = old.clone();
            let fonts = kit.fonts.clone();
            let bold = bold.clone();
            let italic = italic.clone();
            let b = bold.clone();
            let i = italic.clone();
            dialog
                .title("Typography role")
                .width(px(440.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .children(
                            [
                                "Role name",
                                "Font family",
                                "Size · px",
                                "Line height",
                                "Letter spacing · px",
                                "Text color · #RRGGBBAA, blank keeps existing",
                            ]
                            .into_iter()
                            .zip(&fields)
                            .map(|(label, input)| field(label, input)),
                        )
                        .child(
                            Checkbox::new("brand-role-bold")
                                .label("Bold")
                                .checked(bold.get())
                                .on_click(move |value, _, _| b.set(*value)),
                        )
                        .child(
                            Checkbox::new("brand-role-italic")
                                .label("Italic")
                                .checked(italic.get())
                                .on_click(move |value, _, _| i.set(*value)),
                        ),
                )
                .footer(crate::widgets::form_dialog_footer("Save role"))
                .on_ok(move |_, _, cx| {
                    let v = inputs
                        .each_ref()
                        .map(|i| i.read(cx).value().trim().to_string());
                    let parsed = (|| {
                        let color = if v[5].is_empty() {
                            None
                        } else {
                            Some(parse_rgba(&v[5]).ok_or("Enter a six/eight-digit hex color")?)
                        };
                        let font = fonts
                            .values()
                            .find(|f| f.family() == v[1])
                            .map(|f| f.alias().to_owned())
                            .unwrap_or(v[1].clone());
                        let role = TypographyRole {
                            font,
                            size: v[2].parse().map_err(|_| "Invalid size")?,
                            line_height: v[3].parse().map_err(|_| "Invalid line height")?,
                            letter_spacing: v[4].parse().map_err(|_| "Invalid spacing")?,
                            color,
                            bold: bold.get(),
                            italic: italic.get(),
                        };
                        role.validate()?;
                        if v[0].is_empty() || v[0].chars().count() > 200 {
                            return Err("Choose a role name".into());
                        }
                        Ok::<_, String>(role)
                    })();
                    let role = match parsed {
                        Ok(role) => role,
                        Err(e) => {
                            owner
                                .update(cx, |this, cx| this.set_status(e, true, cx))
                                .ok();
                            return false;
                        }
                    };
                    let name = v[0].clone();
                    let old = old.clone();
                    owner
                        .update(cx, |this, cx| {
                            let font = this.editor.doc.design.fonts.get(&role.font).cloned();
                            this.catalog_edit(
                                move |c| {
                                    let b = c.brands.iter_mut().find(|b| b.id == id).ok_or_else(
                                        || {
                                            emulsion_io::IoError::Manifest(
                                                "Brand no longer exists".into(),
                                            )
                                        },
                                    )?;
                                    if let Some(old) = old {
                                        b.typography.remove(&old);
                                    }
                                    if let Some(font) = font {
                                        b.fonts.insert(font.alias().into(), font);
                                    }
                                    b.typography.insert(name, role);
                                    Ok(())
                                },
                                cx,
                            )
                        })
                        .is_ok()
                })
        });
    }
    pub(super) fn edit_brand_palette(
        &mut self,
        id: u64,
        old: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(kit) = self.creative.catalog.brands.iter().find(|b| b.id == id) else {
            return;
        };
        let colors = old
            .as_ref()
            .and_then(|n| kit.palettes.get(n))
            .cloned()
            .or_else(|| {
                brand::extract_colors(&self.editor.doc, &self.selected_layer_roots())
                    .ok()
                    .filter(|v| !v.is_empty())
            })
            .unwrap_or_else(|| kit.colors.clone());
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(old.clone().unwrap_or("Selection palette".into()))
        });
        let colors = cx.new(|cx| {
            InputState::new(window, cx).default_value(
                colors
                    .into_iter()
                    .map(rgba_text)
                    .collect::<Vec<_>>()
                    .join(", "),
            )
        });
        let owner = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let name = name.clone();
            let colors = colors.clone();
            let owner = owner.clone();
            let old = old.clone();
            dialog
                .title("Palette collection")
                .width(px(460.))
                .child(field("Palette name", &name))
                .child(field(
                    "Colors · #RRGGBB or #RRGGBBAA, comma separated",
                    &colors,
                ))
                .footer(crate::widgets::form_dialog_footer("Save palette"))
                .on_ok(move |_, _, cx| {
                    let name = name.read(cx).value().trim().to_owned();
                    let colors = colors
                        .read(cx)
                        .value()
                        .split(',')
                        .map(parse_rgba)
                        .collect::<Option<Vec<_>>>();
                    let Some(colors) = colors.filter(|v| !v.is_empty() && v.len() <= 32) else {
                        return false;
                    };
                    if name.is_empty() || name.chars().count() > 200 {
                        return false;
                    }
                    let old = old.clone();
                    owner
                        .update(cx, |this, cx| {
                            this.catalog_edit(
                                move |c| {
                                    let b = c.brands.iter_mut().find(|b| b.id == id).ok_or_else(
                                        || {
                                            emulsion_io::IoError::Manifest(
                                                "Brand no longer exists".into(),
                                            )
                                        },
                                    )?;
                                    if let Some(old) = old {
                                        b.palettes.remove(&old);
                                    }
                                    b.palettes.insert(name, colors);
                                    Ok(())
                                },
                                cx,
                            )
                        })
                        .is_ok()
                })
        });
    }
    pub(super) fn palette_apply_dialog(
        &mut self,
        colors: Vec<[u8; 4]>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let ids = self.selected_layer_roots();
        let ticket = self.edit_ticket();
        let color = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(rgba_text(colors.first().copied().unwrap_or([0, 0, 0, 255])))
        });
        let target = Rc::new(Cell::new(ColorTarget::Fill));
        let owner = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let owner = owner.clone();
            let color = color.clone();
            let target = target.clone();
            let apply_target = target.clone();
            let ids = ids.clone();
            let swatches = colors
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    let value = *value;
                    let input = color.clone();
                    Button::new(("brand-palette-swatch", index))
                        .label(rgba_text(value))
                        .small()
                        .on_click(move |_, window, cx| {
                            input.update(cx, |state, cx| {
                                state.set_value(rgba_text(value), window, cx)
                            })
                        })
                })
                .collect::<Vec<_>>();
            dialog
                .title("Apply palette color")
                .width(px(440.))
                .child(div().flex().flex_wrap().gap_1().children(swatches))
                .child(field(
                    "Color · alpha 00 is transparent, FF is opaque",
                    &color,
                ))
                .child(
                    Button::new("brand-color-target")
                        .label(format!("Target: {:?}", target.get()))
                        .small()
                        .outline()
                        .dropdown_menu(move |mut menu, _, _| {
                            for (label, value) in [
                                ("Fill", ColorTarget::Fill),
                                ("Stroke", ColorTarget::Stroke),
                                ("Text", ColorTarget::Text),
                                ("All paints", ColorTarget::All),
                            ] {
                                let state = target.clone();
                                menu = menu.item(PopupMenuItem::new(label).on_click(
                                    move |_, _, cx| {
                                        state.set(value);
                                        cx.refresh_windows();
                                    },
                                ));
                            }
                            menu
                        }),
                )
                .footer(crate::widgets::form_dialog_footer("Apply color"))
                .on_ok(move |_, _, cx| {
                    let Some(color) = parse_rgba(&color.read(cx).value()) else {
                        return false;
                    };
                    owner
                        .update(cx, |this, cx| {
                            if !this.edit_is_current(ticket) {
                                return false;
                            }
                            match brand::apply_color(
                                &mut this.editor,
                                &ids,
                                color,
                                apply_target.get(),
                            ) {
                                Ok(()) => {
                                    this.after_change(cx);
                                    true
                                }
                                Err(e) => {
                                    this.set_status(e, true, cx);
                                    false
                                }
                            }
                        })
                        .unwrap_or(false)
                })
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    use gpui_kit::test::TestWindowExt;
    #[gpui_kit::test]
    fn brand_palette_native_dialog_targets_fill_transparency_and_one_undo(cx: &mut TestAppContext) {
        let (ws, cx) = crate::tests::open(cx, Document::new(400, 300));
        cx.simulate_resize(size(px(1400.), px(1000.)));
        let (view, id, before) = cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.install_project(
                    emulsion_core::project::ProjectEditor::new_project(
                        emulsion_core::project::ProjectKind::Design,
                        Document::new(400, 300),
                    )
                    .unwrap(),
                    "Brand".into(),
                    window,
                    cx,
                )
            });
            let view = ws.read(cx).editor.clone().unwrap();
            let (id, before) = view.update(cx, |v, cx| {
                let id = v
                    .editor
                    .execute(Command::AddNode {
                        node: Box::new(
                            emulsion_core::design::Element::Rectangle
                                .node((400, 300), [10, 20, 30, 255]),
                        ),
                        slot: emulsion_core::command::Slot::TOP,
                    })
                    .unwrap()
                    .unwrap();
                v.set_layer_selection(vec![id], Some(id));
                let before = v.editor.doc.clone();
                v.palette_apply_dialog(vec![[220, 60, 40, 255]], window, cx);
                (id, before)
            });
            (view, id, before)
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.click("Color · alpha 00 is transparent, FF is opaque", cx));
        cx.simulate_keystrokes("ctrl-a");
        cx.simulate_input("invalid");
        cx.update(|window, cx| window.click("ok", cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert_eq!(view.read(cx).editor.doc, before);
            assert!(window.find("ok").visible());
            window.click("Color · alpha 00 is transparent, FF is opaque", cx);
        });
        cx.simulate_keystrokes("ctrl-a");
        cx.simulate_input("#10203000");
        cx.update(|window, cx| window.click("ok", cx));
        cx.run_until_parked();
        cx.update(|_,cx|view.update(cx,|v,_|{assert!(matches!(&v.editor.doc.node(id).unwrap().kind,NodeKind::Path{style,..}if style.fill==Some([16,32,48,0])));assert!(v.editor.undo());assert_eq!(v.editor.doc,before);}));
    }
}
