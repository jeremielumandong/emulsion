//! Purpose first, then a visual family; layouts and palettes stay in preview.
use super::*;
use emulsion_core::design::invitations::{self, FamilyId, Occasion, Selection};

fn occasion_label(occasion: Occasion) -> String {
    match occasion {
        Occasion::Wedding => t!("design.invitation.wedding"),
        Occasion::Birthday => t!("design.invitation.birthday"),
    }
    .into_owned()
}

impl EditorView {
    pub(super) fn invitation_purpose_controls(
        &mut self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id("design-invitation-purposes")
            .test_support()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(t!("design.invitation.purpose")),
            )
            .child(
                div().flex().flex_wrap().gap_1().children(
                    [None, Some(Occasion::Wedding), Some(Occasion::Birthday)]
                        .into_iter()
                        .enumerate()
                        .map(|(index, occasion)| {
                            let label = occasion.map_or_else(
                                || t!("design.invitation.more_templates").into_owned(),
                                occasion_label,
                            );
                            Button::new(("design-invitation-purpose", index))
                                .label(label)
                                .xsmall()
                                .outline()
                                .selected(self.design_ui.invitation_occasion == occasion)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.design_ui.invitation_occasion = occasion;
                                    this.design_ui.scroll.set_offset(point(px(0.), px(0.)));
                                    if let Some(search) = &this.design_ui.search {
                                        search.update(cx, |s, cx| s.set_value("", window, cx));
                                    }
                                    cx.notify();
                                }))
                        }),
                ),
            )
    }

    fn load_invitation_previews(&mut self, cx: &mut Context<Self>) {
        if self.design_ui.invitation_previews_loading {
            return;
        }
        let Some(occasion) = self.design_ui.invitation_occasion else {
            return;
        };
        let missing: Vec<FamilyId> = invitations::families(occasion)
            .map(|family| family.id)
            .filter(|id| !self.design_ui.invitation_previews.contains_key(id))
            .collect();
        if missing.is_empty() {
            return;
        }
        self.design_ui.invitation_previews_loading = true;
        cx.spawn(async move |this, cx| {
            let images = cx
                .background_spawn(async move {
                    missing
                        .into_iter()
                        .filter_map(|id| {
                            let doc = Selection::for_family(id).create_primary(360, 504).ok()?;
                            let (w, h, bytes) = super::super::history::doc_thumb(&doc, 280);
                            Some((id, Arc::new(viewport::bgra_image(w, h, bytes))))
                        })
                        .collect::<HashMap<_, _>>()
                })
                .await;
            this.update(cx, |this, cx| {
                this.design_ui.invitation_previews_loading = false;
                if this.visible {
                    this.design_ui.invitation_previews.extend(images);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn invitation_family_cards(
        &mut self,
        query: &str,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        self.load_invitation_previews(cx);
        let occasion = self
            .design_ui
            .invitation_occasion
            .unwrap_or(Occasion::Wedding);
        let matching: Vec<_> = invitations::families(occasion)
            .filter(|family| {
                let searchable = format!(
                    "{} {} {} {} {} {}",
                    occasion.label(),
                    occasion_label(occasion),
                    family.label,
                    family.description,
                    family
                        .variants
                        .iter()
                        .map(|variant| variant.label)
                        .collect::<Vec<_>>()
                        .join(" "),
                    family
                        .palettes
                        .iter()
                        .map(|palette| palette.label)
                        .collect::<Vec<_>>()
                        .join(" ")
                );
                searchable.to_lowercase().contains(query)
            })
            .collect();
        div()
            .id("design-invitation-families")
            .test_support()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(p.muted)
                    .child(t!("design.invitation.choose_style")),
            )
            .children(matching.iter().enumerate().map(|(index, family)| {
                let selection = Selection::for_family(family.id);
                let preview = self.design_ui.invitation_previews.get(&family.id).cloned();
                Button::new(("design-invitation-family", index))
                    .accessibility_label(family.label)
                    .tooltip(family.description)
                    .outline()
                    .p_0()
                    .w_full()
                    .h(px(232.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .w_full()
                            .min_w_0()
                            .gap_2()
                            .child(
                                div()
                                    .w_full()
                                    .h(px(174.))
                                    .flex_none()
                                    .overflow_hidden()
                                    .bg(p.soft_bg)
                                    .when_some(preview, |d, image| {
                                        d.child(
                                            img(image)
                                                .w_full()
                                                .h(px(174.))
                                                .aspect_ratio(228. / 174.)
                                                .object_fit(ObjectFit::Contain)
                                                .id(("design-invitation-thumbnail", index))
                                                .test_support(),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(family.label),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .pb_2()
                                    .text_size(px(10.))
                                    .text_color(p.muted)
                                    .child(t!("design.invitation.variant_count")),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.preview_invitation_family(selection, window, cx);
                    }))
            }))
            .when(matching.is_empty(), |d| {
                d.child(
                    div()
                        .text_size(px(12.))
                        .child(t!("design.invitation.no_matches")),
                )
            })
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(p.muted)
                    .child(t!("design.invitation.personalize_hint")),
            )
    }
}

#[cfg(test)]
#[path = "design_invitation_cache_tests.rs"]
mod tests;
