//! Photoshop-style Layers panel footer: link, layer style, mask, fill or
//! adjustment layer, group, new layer and delete.
use super::layer_menu::item;
use super::*;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenu};
use gpui_kit::component::{Disableable, Sizable};

/// The fx menu, in Photoshop's order.
const EFFECTS: [(&str, &str); 10] = [
    ("drop_shadow", "Drop Shadow…"),
    ("inner_shadow", "Inner Shadow…"),
    ("outer_glow", "Outer Glow…"),
    ("inner_glow", "Inner Glow…"),
    ("bevel_emboss", "Bevel and Emboss…"),
    ("satin", "Satin…"),
    ("color_overlay", "Color Overlay…"),
    ("gradient_overlay", "Gradient Overlay…"),
    ("pattern_overlay", "Pattern Overlay…"),
    ("stroke", "Stroke…"),
];

/// The fill or adjustment menu below Solid Color, one slice per section.
const ADJUSTMENTS: [&[(&str, &str)]; 4] = [
    &[
        ("levels", "Levels…"),
        ("curves", "Curves…"),
        ("color_balance", "Color Balance…"),
        ("brightness_contrast", "Brightness/Contrast…"),
        ("exposure", "Exposure…"),
        ("vibrance", "Vibrance…"),
    ],
    &[
        ("hue_saturation", "Hue/Saturation…"),
        ("selective_color", "Selective Color…"),
        ("black_and_white", "Black & White…"),
        ("gradient_map", "Gradient Map…"),
        ("photo_filter", "Photo Filter…"),
        ("white_balance", "White Balance…"),
    ],
    &[
        ("invert", "Invert"),
        ("threshold", "Threshold…"),
        ("posterize", "Posterize…"),
    ],
    &[("grain", "Grain…"), ("vignette", "Vignette…")],
];

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
                    .child(rail::tool_icon("link").size_4())
                    .accessibility_label("Link layers")
                    .tooltip(if unlink && !link {
                        "Unlink layers"
                    } else {
                        "Link layers"
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
                    .accessibility_label("Add a layer style")
                    .tooltip("Add a layer style")
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
                    .child(rail::tool_icon("contrast").size_4())
                    .accessibility_label("Create new fill or adjustment layer")
                    .tooltip("Create new fill or adjustment layer")
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
                    .accessibility_label("Create a new group")
                    .tooltip("Create a new group")
                    .disabled(!ready)
                    .on_click(cx.listener(|this, _, _, cx| this.new_empty_group(cx))),
            )
            .child(
                // Dropping a layer here duplicates it, as in Photoshop.
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
                            .child(rail::tool_icon("file-plus").size_4())
                            .accessibility_label("Create a new layer")
                            .tooltip("Create a new layer")
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
                            .icon(IconName::Trash)
                            .accessibility_label("Delete layer")
                            .tooltip("Delete layer")
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
            "Blending Options…",
            true,
            move |e, window, cx| e.open_blending_options(id, window, cx),
        ))
        .separator();
    for (key, label) in EFFECTS {
        menu = menu.item(item(editor, label, true, move |e, window, cx| {
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
    let mut menu = menu.item(item(editor, "Solid Color…", true, |e, _, cx| {
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
    }));
    for section in ADJUSTMENTS {
        menu = menu.separator();
        for &(key, label) in section {
            let Some(adjust) = adjustment(key) else {
                continue;
            };
            menu = menu.item(item(editor, label, true, move |e, _, cx| {
                e.add_node(Node::adjust(0, adjust.clone()), cx);
            }));
        }
    }
    menu.item(item(editor, "Color Lookup (.cube)…", true, |e, _, cx| {
        e.import_lut(None, cx)
    }))
    .separator()
    .item(item(editor, "Remove Background (AI)", true, |e, _, cx| {
        e.remove_background(cx)
    }))
    .item(item(editor, "Depth Map (AI)", true, |e, _, cx| {
        e.depth_layer(cx)
    }))
    .item(item(editor, "Restore Faces (AI)", true, |e, _, cx| {
        e.restore_faces(cx)
    }))
    .item(item(
        editor,
        format!("Upscale ×{} (AI)", emulsion_ai::upscale::factor()),
        true,
        |e, _, cx| e.ai_upscale(cx),
    ))
}
