//! Application-themed Library chrome; behavior remains in the shared Library commands.
use super::*;
use gpui_kit::component::{Disableable, Icon, Selectable};

pub(super) fn palette(cx: &App) -> theme::Palette {
    theme::palette(cx)
}

pub(super) fn heading(title: impl Into<SharedString>, cx: &App) -> impl IntoElement {
    div()
        .h(px(28.))
        .flex_none()
        .flex()
        .items_center()
        .px_2()
        .bg(palette(cx).panel)
        .border_t_1()
        .border_color(palette(cx).line)
        .text_size(px(11.))
        .text_color(palette(cx).muted)
        .child(title.into())
}

impl Workspace {
    pub(super) fn library_histogram_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let bins = self.batch.develop.rgb_histogram;
        let peak = bins.iter().flatten().copied().max().unwrap_or(1).max(1) as f32;
        let metadata = self
            .batch
            .develop
            .source
            .as_ref()
            .map(|source| {
                let m = &source.metadata;
                format!("{} · {} × {}", m.model, m.width, m.height)
            })
            .unwrap_or_else(|| "Select a photo".into());
        div()
            .id("library-histogram-panel")
            .test_support()
            .flex_none()
            .bg(p.panel)
            .child(heading("Histogram", cx))
            .child(
                div().px_2().py_1().child(
                    div()
                        .id("library-rgb-histogram")
                        .test_support()
                        .h(px(82.))
                        .bg(palette(cx).stage)
                        .border_1()
                        .border_color(p.line)
                        .child(
                            canvas(
                                |_, _, _| {},
                                move |bounds, _, window, _| {
                                    for (channel, color) in
                                        [0xe56565, 0x6fc785, 0x679bd7].into_iter().enumerate()
                                    {
                                        let mut path = PathBuilder::fill();
                                        let at = |bin: usize| {
                                            bounds.origin
                                                + point(
                                                    bounds.size.width * (bin as f32 / 31.),
                                                    bounds.size.height
                                                        * (1. - bins[channel][bin] as f32 / peak),
                                                )
                                        };
                                        path.move_to(bounds.bottom_left());
                                        for bin in 0..32 {
                                            path.line_to(at(bin));
                                        }
                                        path.line_to(bounds.bottom_right());
                                        path.close();
                                        if let Ok(path) = path.build() {
                                            window.paint_path(path, rgb(color).opacity(0.48));
                                        }
                                    }
                                },
                            )
                            .size_full(),
                        ),
                ),
            )
            .child(
                div()
                    .px_2()
                    .pb_1()
                    .text_size(px(10.))
                    .text_color(p.muted)
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(metadata),
            )
            .child(
                div().px_2().pb_1().child(
                    Checkbox::new("library-clipping")
                        .small()
                        .label("Clipping indicators")
                        .checked(self.batch.develop.clipping)
                        .on_change(cx.listener(|this, value, _, cx| {
                            this.batch.develop.clipping = *value;
                            this.invalidate_library_preview();
                            cx.notify();
                        })),
                ),
            )
            .into_any_element()
    }

    pub(super) fn library_editing_toolstrip(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut row = div()
            .id("library-editing-toolstrip")
            .test_support()
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .h(px(36.))
            .px_2()
            .bg(palette(cx).panel)
            .border_y_1()
            .border_color(palette(cx).line);
        for (tool, icon, title, section) in [
            (5usize, "crop", "Crop overlay · R", 1usize),
            (3, "pipette", "Heal / clone · Q", 5),
            (9, "blend", "Linear gradient · M", 5),
            (8, "circle", "Radial gradient · Shift+M", 5),
            (1, "brush", "Adjustment brush · K", 5),
            (7, "scan-line", "Guided transform", 1),
        ] {
            row = row.child(
                Button::new(("library-editing-tool", tool))
                    .icon(Icon::default().path(format!("icons/{icon}.svg")))
                    .small()
                    .ghost()
                    .selected(self.batch.develop.canvas_tool == tool)
                    .tooltip(title)
                    .accessibility_label(title)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.batch.develop.canvas_tool = if this.batch.develop.canvas_tool == tool {
                            0
                        } else {
                            tool
                        };
                        this.batch.develop.section = section;
                        this.batch.develop.slider_key = None;
                        this.invalidate_library_preview();
                        cx.notify();
                    })),
            );
        }
        row.into_any_element()
    }

    pub(super) fn library_develop_footer(&self, cx: &mut Context<Self>) -> AnyElement {
        let ready = self
            .batch
            .current
            .and_then(|i| self.batch.items.get(i))
            .is_some_and(|i| self.batch.develop.current_params(&i.path).is_some());
        div()
            .id("library-develop-footer")
            .test_support()
            .flex_none()
            .h(px(36.))
            .flex()
            .gap_1()
            .p_1()
            .bg(palette(cx).panel)
            .child(
                Button::new("library-footer-sync")
                    .label("Sync settings")
                    .small()
                    .outline()
                    .flex_1()
                    .disabled(!ready)
                    .on_click(cx.listener(|this, _, _, cx| this.library_save_develop(true, cx))),
            )
            .child(
                Button::new("library-save-all-drafts")
                    .label("Save all")
                    .small()
                    .outline()
                    .flex_1()
                    .disabled(!self.batch.develop.dirty() || self.batch.develop.saving)
                    .on_click(cx.listener(|this, _, _, cx| this.library_save_develop(false, cx))),
            )
            .child(
                Button::new("library-footer-reset")
                    .label("Reset")
                    .small()
                    .outline()
                    .flex_1()
                    .disabled(!ready || self.batch.develop.saving)
                    .on_click(
                        cx.listener(|this, _, _, cx| this.library_adjust(Default::default(), cx)),
                    ),
            )
            .into_any_element()
    }
}

impl Workspace {
    pub(super) fn library_folder_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        use emulsion_io::creative_library::AssetKind;
        let revision = self.batch.library.catalog.revision;
        if self
            .batch
            .library
            .folder_rows
            .as_ref()
            .is_none_or(|(r, _)| *r != revision)
        {
            let mut folders = std::collections::BTreeMap::<PathBuf, usize>::new();
            for asset in &self.batch.library.catalog.assets {
                if asset.kind == AssetKind::Image
                    && let Some(parent) = asset.path.parent()
                {
                    *folders.entry(parent.to_owned()).or_default() += 1;
                }
            }
            self.batch.library.folder_rows =
                Some((revision, Arc::new(folders.into_iter().collect())));
        }
        let folders = self.batch.library.folder_rows.as_ref().unwrap().1.clone();
        let count = folders.len();
        let list = uniform_list(
            "library-folder-rows",
            count,
            cx.processor(move |_, range: Range<usize>, _, cx| {
                range
                    .map(|i| {
                        let (path, count) = &folders[i];
                        let label = format!(
                            "{} · {count}",
                            path.file_name()
                                .unwrap_or(path.as_os_str())
                                .to_string_lossy()
                        );
                        let path = path.clone();
                        Button::new(("library-folder-row", i))
                            .label(label)
                            .tooltip(path.display().to_string())
                            .small()
                            .ghost()
                            .w_full()
                            .h(px(26.))
                            .justify_start()
                            .rounded_none()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.batch.library.collection = None;
                                this.batch.library.source_paths = Some(
                                    this.batch
                                        .library
                                        .catalog
                                        .assets
                                        .iter()
                                        .filter(|a| {
                                            a.kind == AssetKind::Image
                                                && a.path.parent() == Some(path.as_path())
                                        })
                                        .map(|a| a.path.clone())
                                        .collect(),
                                );
                                this.batch.folder = Some(path.clone());
                                this.library_show(cx);
                            }))
                            .into_any_element()
                    })
                    .collect()
            }),
        )
        .w_full()
        .h(px((count as f32 * 26.).min(130.)));
        div()
            .id("library-folders-panel")
            .test_support()
            .flex()
            .flex_col()
            .child(heading("Folders", cx))
            .when(count > 0, |d| d.child(list))
            .when(count == 0, |d| {
                d.child(
                    div()
                        .p_2()
                        .text_size(px(10.))
                        .text_color(palette(cx).muted)
                        .child("Imported folders appear here"),
                )
            })
            .into_any_element()
    }
}
