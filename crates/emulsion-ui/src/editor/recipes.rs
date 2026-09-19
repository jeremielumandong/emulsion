//! The Recipes panel: film recipes to apply with one click, import from
//! the clipboard, save and delete.

use super::*;
use emulsion_recipes::store::{self, Origin};
use emulsion_recipes::{Recipe, import};

#[derive(Default)]
pub(crate) struct RecipeState {
    pub open: bool,
    /// Tag filter, if any.
    pub tag: Option<String>,
    cache: Option<Vec<(Recipe, Origin)>>,
}

/// Where saved recipes live.
pub fn recipes_dir() -> PathBuf {
    emulsion_io::recent::data_dir().join("recipes")
}

impl EditorView {
    pub fn toggle_recipes(&mut self, cx: &mut Context<Self>) {
        self.recipes.open = !self.recipes.open;
        if self.recipes.open {
            self.recipes.cache = Some(store::list(&recipes_dir()));
        }
        cx.notify();
    }

    fn recipe_list(&mut self) -> Vec<(Recipe, Origin)> {
        self.recipes
            .cache
            .get_or_insert_with(|| store::list(&recipes_dir()))
            .clone()
    }

    /// Apply a recipe above the selected node, as one undo step.
    pub fn apply_recipe(&mut self, recipe: &Recipe, cx: &mut Context<Self>) {
        let compiled = match emulsion_recipes::compile(recipe) {
            Ok(c) => c,
            Err(e) => {
                self.set_status(format!("Could not apply {}: {e}", recipe.name), true, cx);
                return;
            }
        };
        let slot = self.insertion_slot();
        match store::add_to(&mut self.editor, compiled, slot) {
            Ok(gid) => {
                self.selected = Some(gid);
                self.set_status(
                    format!(
                        "Applied {}. Open the group to tune each stage.",
                        recipe.name
                    ),
                    false,
                    cx,
                );
                self.after_change(cx);
            }
            Err(e) => self.set_status(e.to_string(), true, cx),
        }
    }

    /// Read a pasted recipe block or TOML from the clipboard and save it.
    pub fn import_recipe_from_clipboard(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) else {
            self.set_status("Copy a recipe's settings first, then import.", false, cx);
            return;
        };
        let (recipe, unknown) = match Recipe::from_toml(&text) {
            Ok(r) => (r, Vec::new()),
            Err(_) => import::parse_text(&text),
        };
        if let Err(e) = recipe.validate() {
            self.set_status(format!("That does not read as a recipe: {e}"), true, cx);
            return;
        }
        match store::save(&recipes_dir(), &recipe) {
            Ok(_) => {
                self.recipes.cache = None;
                let note = if unknown.is_empty() {
                    String::new()
                } else {
                    format!(
                        " · {} line{} not understood",
                        unknown.len(),
                        if unknown.len() == 1 { "" } else { "s" }
                    )
                };
                self.set_status(format!("Saved recipe {}{note}", recipe.name), false, cx);
                self.apply_recipe(&recipe, cx);
            }
            Err(e) => self.set_status(format!("Could not save the recipe: {e}"), true, cx),
        }
    }

    fn delete_recipe(&mut self, name: &str, cx: &mut Context<Self>) {
        if store::delete(&recipes_dir(), name) {
            self.recipes.cache = None;
            self.set_status(format!("Deleted recipe {name}"), false, cx);
        }
        cx.notify();
    }

    pub(crate) fn recipes_view(
        &mut self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.recipes.open {
            return None;
        }
        let all = self.recipe_list();
        let mut tags: Vec<String> = all.iter().flat_map(|(r, _)| r.tags.clone()).collect();
        tags.sort();
        tags.dedup();
        let tag = self.recipes.tag.clone();
        let mut header = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
            .child(label("Film recipes", p));
        header = header.child(
            chip("rc-all", "all", tag.is_none(), p).on_click(cx.listener(|this, _, _, cx| {
                this.recipes.tag = None;
                cx.notify();
            })),
        );
        for (i, t) in tags.iter().enumerate() {
            let on = tag.as_deref() == Some(t);
            let t2 = t.clone();
            header = header.child(chip(("rc-tag", i), t.clone(), on, p).on_click(cx.listener(
                move |this, _, _, cx| {
                    this.recipes.tag = Some(t2.clone());
                    cx.notify();
                },
            )));
        }
        let mut rows = div().flex().flex_col().gap(px(4.));
        for (i, (r, origin)) in all.iter().enumerate() {
            if let Some(t) = &tag
                && !r.tags.contains(t)
            {
                continue;
            }
            let look = emulsion_recipes::looks::find(&r.film_simulation)
                .map(|l| l.label)
                .unwrap_or("LUT");
            let recipe = r.clone();
            let mut row = div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    button(("rc-apply", i), r.name.clone(), false, p)
                        .py(px(3.))
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.apply_recipe(&recipe, cx)),
                        ),
                )
                .child(
                    mono(
                        format!(
                            "{look} · {}",
                            if r.author.is_empty() {
                                "unknown"
                            } else {
                                r.author.as_str()
                            }
                        ),
                        9.5,
                        p.muted,
                    )
                    .flex_1()
                    .overflow_hidden()
                    .whitespace_nowrap(),
                );
            if let Origin::Saved(_) = origin {
                let name = r.name.clone();
                row = row
                    .child(chip(("rc-del", i), "×", false, p).on_click(
                        cx.listener(move |this, _, _, cx| this.delete_recipe(&name, cx)),
                    ));
            }
            rows = rows.child(row);
        }
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .px(px(15.))
                .py(px(10.))
                .border_b_1()
                .border_color(p.line)
                .bg(p.panel)
                .child(header)
                .child(rows)
                .child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .child(
                            button("rc-import", "Import from clipboard", true, p)
                                .py(px(4.))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.import_recipe_from_clipboard(cx)
                                })),
                        )
                        .child(mono(
                            "paste a Fuji X Weekly block or a .recipe.toml",
                            9.5,
                            p.muted,
                        )),
                )
                .into_any_element(),
        )
    }
}
