//! The conventional Layers panel footer: link, layer style, mask, fill or
//! adjustment layer, group, new layer and delete.
use super::layer_menu::item;
use super::*;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenu};
use gpui_kit::component::{Disableable, Sizable};

/// The fx menu, in the conventional order: (effect key, catalog key).
const EFFECTS: [(&str, &str); 10] = [
    ("drop_shadow", "editor.layers_footer.drop_shadow"),
    ("inner_shadow", "editor.layers_footer.inner_shadow"),
    ("outer_glow", "editor.layers_footer.outer_glow"),
    ("inner_glow", "editor.layers_footer.inner_glow"),
    ("bevel_emboss", "editor.layers_footer.bevel_emboss"),
    ("satin", "editor.layers_footer.satin"),
    ("color_overlay", "editor.layers_footer.color_overlay"),
    ("gradient_overlay", "editor.layers_footer.gradient_overlay"),
    ("pattern_overlay", "editor.layers_footer.pattern_overlay"),
    ("stroke", "editor.layers_footer.stroke"),
];

/// The fill or adjustment menu below Solid Color, one slice per section:
/// (adjustment key, catalog key).
const ADJUSTMENTS: [&[(&str, &str)]; 4] = [
    &[
        ("levels", "editor.layers_footer.levels"),
        ("curves", "editor.layers_footer.curves"),
        ("color_balance", "editor.layers_footer.color_balance"),
        (
            "brightness_contrast",
            "editor.layers_footer.brightness_contrast",
        ),
        ("exposure", "editor.layers_footer.exposure"),
        ("vibrance", "editor.layers_footer.vibrance"),
    ],
    &[
        ("hue_saturation", "editor.layers_footer.hue_saturation"),
        ("selective_color", "editor.layers_footer.selective_color"),
        ("black_and_white", "editor.layers_footer.black_and_white"),
        ("gradient_map", "editor.layers_footer.gradient_map"),
        ("photo_filter", "editor.layers_footer.photo_filter"),
        ("white_balance", "editor.layers_footer.white_balance"),
    ],
    &[
        ("invert", "editor.layers_footer.invert"),
        ("threshold", "editor.layers_footer.threshold"),
        ("posterize", "editor.layers_footer.posterize"),
    ],
    &[
        ("grain", "editor.layers_footer.grain"),
        ("vignette", "editor.layers_footer.vignette"),
    ],
];

/// Lucide drawings take their colour from the element, not the button.
fn footer_icon(glyph: &'static str, enabled: bool, p: &Palette) -> Svg {
    rail::tool_icon(glyph)
        .size_4()
        .text_color(if enabled { p.ink } else { p.muted })
}

fn adjustment(key: &str) -> Option<Adjustment> {
    Adjustment::catalogue().into_iter().find(|a| a.key() == key)
}

impl EditorView {
    pub(super) fn layers_footer(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let ready = self.layer_menu_ready();
        let ids = self.selected_layer_ids();
        let structural = ready
            && !ids.is_empty()
            && ids
                .iter()
                .flat_map(|id| self.editor.doc.subtree(*id))
                .all(|id| self.editor.doc.locked_ancestor(id).is_none());
        let link = self.can_link_layers(true);
        let unlink = self.can_link_layers(false);
        let single = ids.len() == 1;
        let editor = cx.entity().downgrade();
        let adjust_editor = editor.clone();
        let accent = p.accent;
        div()
            .id("layers-footer")
            .flex()
            .flex_none()
            .items_center()
            .justify_end()
            .gap(px(2.))
            .pt(px(4.))
            .mt(px(2.))
            .border_t_1()
            .border_color(p.line)
            .child(
                Button::new("layers-link")
                    .xsmall()
                    .ghost()
                    .child(footer_icon("link", link || unlink, p))
                    .accessibility_label(t!("editor.layers_footer.link"))
                    .tooltip(if unlink && !link {
                        t!("editor.layers_footer.unlink")
                    } else {
                        t!("editor.layers_footer.link")
                    })
                    .disabled(!link && !unlink)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_layer_links(link, cx);
                    })),
            )
            .child(
                Button::new("layers-fx")
                    .xsmall()
                    .ghost()
                    .child(
                        div()
                            .italic()
                            .font_weight(FontWeight::BOLD)
                            .text_size(px(12.))
                            .child("fx"),
                    )
                    .accessibility_label(t!("editor.layers_footer.add_style"))
                    .tooltip(t!("editor.layers_footer.add_style"))
                    .disabled(!(ready && single))
                    .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _, cx| {
                        let Some(editor) = editor.upgrade() else {
                            return menu;
                        };
                        effects_menu(menu, &editor, cx)
                    }),
            )
            .child(self.layer_mask_button(cx))
            .child(
                Button::new("layers-adjust")
                    .xsmall()
                    .ghost()
                    .child(footer_icon("contrast", ready, p))
                    .accessibility_label(t!("editor.layers_footer.new_adjustment"))
                    .tooltip(t!("editor.layers_footer.new_adjustment"))
                    .disabled(!ready)
                    .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _, cx| {
                        let Some(editor) = adjust_editor.upgrade() else {
                            return menu;
                        };
                        adjustment_menu(menu, &editor, cx)
                    }),
            )
            .child(
                Button::new("layers-group")
                    .xsmall()
                    .ghost()
                    .icon(IconName::Folder)
                    .accessibility_label(t!("editor.layers_footer.new_group"))
                    .tooltip(t!("editor.layers_footer.new_group"))
                    .disabled(!ready)
                    .on_click(cx.listener(|this, _, _, cx| this.new_empty_group(cx))),
            )
            .child(
                Button::new("layers-new-vector")
                    .xsmall()
                    .ghost()
                    .child(footer_icon("pencil", ready, p))
                    .accessibility_label("Create a new vector layer")
                    .tooltip("Create a new vector layer: editable pencil strokes")
                    .disabled(!ready)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_text_field(cx);
                        this.new_vector_layer(cx);
                    })),
            )
            .child(
                // Dropping a layer here duplicates it, the conventional behaviour.
                div()
                    .id("layers-new-drop")
                    .rounded_sm()
                    .drag_over::<DraggedNode>(move |s, _, _, _| s.bg(accent.opacity(0.25)))
                    .on_drop(cx.listener(|this, d: &DraggedNode, _, cx| {
                        if !this.layer_is_selected(d.id) {
                            this.set_layer_selection(vec![d.id], Some(d.id));
                        }
                        this.duplicate_selected(cx);
                    }))
                    .child(
                        Button::new("layers-new")
                            .xsmall()
                            .ghost()
                            .child(footer_icon("file-plus", ready, p))
                            .accessibility_label(t!("editor.layers_footer.new_layer"))
                            .tooltip(t!("editor.layers_footer.new_layer"))
                            .disabled(!ready)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.close_text_field(cx);
                                this.new_empty_layer(cx);
                            })),
                    ),
            )
            .child(
                // Dropping a layer here deletes it.
                div()
                    .id("layers-delete-drop")
                    .rounded_sm()
                    .drag_over::<DraggedNode>(move |s, _, _, _| s.bg(accent.opacity(0.25)))
                    .on_drop(cx.listener(|this, d: &DraggedNode, _, cx| {
                        if !this.layer_is_selected(d.id) {
                            this.set_layer_selection(vec![d.id], Some(d.id));
                        }
                        this.delete_selected(cx);
                    }))
                    .child(
                        Button::new("layers-delete")
                            .xsmall()
                            .ghost()
                            .child(footer_icon("trash", structural, p))
                            .accessibility_label(t!("editor.layers_footer.delete"))
                            .tooltip(t!("editor.layers_footer.delete"))
                            .disabled(!structural)
                            .on_click(cx.listener(|this, _, _, cx| this.delete_selected(cx))),
                    ),
            )
            .into_any_element()
    }

    fn new_empty_group(&mut self, cx: &mut Context<Self>) {
        self.close_text_field(cx);
        let n = self
            .editor
            .doc
            .nodes
            .iter()
            .filter(|n| n.is_group())
            .count()
            + 1;
        self.add_node(Node::group(0, format!("Group {n}")), cx);
    }
}

fn effects_menu(
    menu: PopupMenu,
    editor: &Entity<EditorView>,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let Some(id) = editor.read(cx).selected else {
        return menu;
    };
    let mut menu = menu
        .item(item(
            editor,
            t!("editor.layer_menu.blending_options"),
            true,
            move |e, window, cx| e.open_blending_options(id, window, cx),
        ))
        .separator();
    for (key, label) in EFFECTS {
        menu = menu.item(item(editor, t!(label), true, move |e, window, cx| {
            e.open_layer_effect_kind(id, key, window, cx)
        }));
    }
    menu
}

fn adjustment_menu(
    menu: PopupMenu,
    editor: &Entity<EditorView>,
    _: &mut Context<PopupMenu>,
) -> PopupMenu {
    let mut menu = menu.item(item(
        editor,
        t!("editor.layers_footer.solid_color"),
        true,
        |e, _, cx| {
            let n = e
                .editor
                .doc
                .nodes
                .iter()
                .filter(|n| matches!(n.kind, NodeKind::Fill { .. }))
                .count()
                + 1;
            e.add_node(
                Node::new(
                    0,
                    format!("Color Fill {n}"),
                    NodeKind::Fill { rgba: e.tools.fg },
                ),
                cx,
            );
        },
    ));
    for section in ADJUSTMENTS {
        menu = menu.separator();
        for &(key, label) in section {
            let Some(adjust) = adjustment(key) else {
                continue;
            };
            menu = menu.item(item(editor, t!(label), true, move |e, _, cx| {
                e.add_node(Node::adjust(0, adjust.clone()), cx);
            }));
        }
    }
    menu.item(item(
        editor,
        t!("editor.layers_footer.color_lookup"),
        true,
        |e, _, cx| e.import_lut(None, cx),
    ))
    .separator()
    .item(item(
        editor,
        t!("editor.layers_footer.remove_background"),
        true,
        |e, _, cx| e.remove_background(cx),
    ))
    .item(item(
        editor,
        t!("editor.layers_footer.depth_map"),
        true,
        |e, _, cx| e.depth_layer(cx),
    ))
    .item(item(
        editor,
        t!("editor.layers_footer.restore_faces"),
        true,
        |e, _, cx| e.restore_faces(cx),
    ))
    .item(item(
        editor,
        t!(
            "editor.layers_footer.upscale",
            factor = emulsion_ai::upscale::factor()
        ),
        true,
        |e, _, cx| e.ai_upscale(cx),
    ))
}
