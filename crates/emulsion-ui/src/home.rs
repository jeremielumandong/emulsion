//! The Home screen: a headline, new/open, and recent files.

use crate::theme::{self, Palette};
use crate::viewport::bgra_image;
use crate::workspace::Workspace;
use emulsion_core::{Command, Document, Node, NodeKind};
use emulsion_io::recent;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::{Disableable, IconName, Selectable, Sizable};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

pub(crate) struct GalleryThumbnail {
    requested_width: u32,
    image: Option<Arc<RenderImage>>,
}

fn thumbnail_width(viewport_width: f32, scale_factor: f32) -> u32 {
    let pixels = (viewport_width / 4.0 * scale_factor).ceil() as u32;
    // Bucket resize requests to avoid rebuilding on every single-pixel drag.
    pixels.div_ceil(128).saturating_mul(128).clamp(128, 2048)
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum HomeFilter {
    #[default]
    All,
    Unfinished,
    Today,
    Starred,
}

#[derive(Default)]
pub(crate) struct HomeState {
    search: Option<(Entity<InputState>, Subscription)>,
    rows: bool,
    folder: Option<PathBuf>,
    filter: HomeFilter,
    selected: Option<PathBuf>,
    checked: HashSet<PathBuf>,
}

fn path_id(prefix: &'static str, path: &Path) -> ElementId {
    (ElementId::from(prefix), path.to_string_lossy().into_owned()).into()
}

fn file_name(path: &Path) -> String {
    path.file_stem()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn file_kind(path: &Path) -> String {
    path.extension()
        .map(|kind| kind.to_string_lossy().to_uppercase())
        .unwrap_or_default()
}

fn control(id: impl Into<ElementId>, label: impl Into<SharedString>, p: &Palette) -> Button {
    Button::new(id)
        .label(label)
        .xsmall()
        .rounded_none()
        .bg(p.soft_bg)
        .border_color(p.line)
        .text_color(p.ink)
}

fn app_icon() -> Arc<Image> {
    static ICON: OnceLock<Arc<Image>> = OnceLock::new();
    ICON.get_or_init(|| {
        Arc::new(Image::from_bytes(
            ImageFormat::Png,
            include_bytes!("../../../assets/icons/emulsion.png").to_vec(),
        ))
    })
    .clone()
}

#[cfg(test)]
// Kept beside the small Home state helpers so the interaction fixtures can
// use their private path/filter utilities without widening production APIs.
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use crate::app_state::{AppSettings, Capabilities, CliStatus};
    use core::prelude::v1::test;
    use emulsion_io::settings::Settings;
    use gpui_kit::component::Root;
    use gpui_kit::test::TestWindowExt;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn browser(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_reduce_motion(true);
            theme::install(cx);
            crate::actions::bind(cx);
            cx.set_global(AppSettings(Settings::default()));
            cx.set_global(Capabilities {
                cli: CliStatus::Missing,
            });
        });
        let slot = Rc::new(RefCell::new(None));
        let installed = slot.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let workspace = cx.new(|cx| Workspace::new(window, cx));
            *installed.borrow_mut() = Some(workspace.clone());
            Root::new(workspace, window, cx)
        });
        let workspace = slot.borrow().clone().unwrap();
        cx.simulate_resize(size(px(1280.), px(800.)));
        // Let Workspace's asynchronous disk load finish before installing the
        // deterministic recent list used by these tests.
        cx.run_until_parked();
        cx.update(|_, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.splash = false;
                workspace.recovered.clear();
                workspace.recents = ["photos/Portrait.png", "prints/Poster.ora"]
                    .into_iter()
                    .enumerate()
                    .map(|(i, path)| recent::Recent {
                        path: path.into(),
                        opened: recent::now().saturating_sub(i as u64 * 172800),
                        summary: "3 layers".into(),
                    })
                    .collect();
                workspace.thumbs.clear();
                for entry in &workspace.recents {
                    workspace.thumbs.insert(
                        entry.path.clone(),
                        GalleryThumbnail {
                            requested_width: 2048,
                            image: None,
                        },
                    );
                }
                cx.notify();
            });
        });
        cx.run_until_parked();
        (workspace, cx)
    }

    #[gpui_kit::test]
    fn returning_home_keeps_open_actions_reachable(cx: &mut TestAppContext) {
        let (workspace, cx) = browser(cx);
        for route in ["tab", "action", "menu", "close"] {
            cx.update(|window, cx| {
                cx.global_mut::<AppSettings>().0.compact_chrome = route == "menu";
                workspace.update(cx, |workspace, cx| {
                    workspace.install(
                        Document::new(64, 64),
                        None,
                        None,
                        None,
                        "Focus regression".into(),
                        window,
                        cx,
                    );
                });
            });
            cx.run_until_parked();
            match route {
                "tab" => cx.update(|window, cx| window.click("tab-home", cx)),
                "action" => cx.update(|window, cx| {
                    window.dispatch_action(Box::new(crate::actions::ShowHome), cx)
                }),
                "menu" => {
                    cx.update(|window, cx| window.click("compact-app-menu", cx));
                    cx.run_until_parked();
                    cx.update(|window, cx| window.within("popup-menu").click(4usize, cx));
                }
                "close" => cx.update(|window, cx| {
                    workspace.update(cx, |workspace, cx| workspace.close_tab(0, window, cx));
                }),
                _ => unreachable!(),
            }
            cx.run_until_parked();
            cx.update(|window, cx| {
                assert_eq!(workspace.read(cx).screen, crate::workspace::Screen::Home);
                window.click("home-import-files", cx);
            });
            cx.run_until_parked();
            assert!(cx.did_prompt_for_paths(), "Open after {route}");
            cx.simulate_path_prompt_response(|_| None);
            cx.run_until_parked();
            cx.update(|window, cx| window.press("ctrl-o", cx));
            cx.run_until_parked();
            assert!(cx.did_prompt_for_paths(), "keyboard Open after {route}");
            cx.simulate_path_prompt_response(|_| None);
            cx.update(|window, cx| {
                workspace.update(cx, |workspace, cx| {
                    if !workspace.tabs.is_empty() {
                        workspace.close_tab(0, window, cx);
                    }
                });
            });
            cx.run_until_parked();
        }
    }

    #[gpui_kit::test]
    fn home_search_folder_filters_and_row_selection_use_real_recent_paths(cx: &mut TestAppContext) {
        let (workspace, cx) = browser(cx);
        cx.update(|window, cx| window.click("home-filter-today", cx));
        cx.run_until_parked();
        cx.update(|_, cx| assert_eq!(workspace.read(cx).visible_recents(cx).len(), 1));
        cx.update(|window, cx| window.click("home-filter-all", cx));
        cx.update(|window, cx| window.click(path_id("home-folder", Path::new("prints")), cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert_eq!(
                workspace.read(cx).visible_recents(cx)[0].path,
                Path::new("prints/Poster.ora")
            )
        });
        cx.update(|window, cx| window.click("home-folder-all", cx));
        cx.update(|window, cx| {
            let input = workspace
                .read(cx)
                .home_state
                .search
                .as_ref()
                .unwrap()
                .0
                .clone();
            input.update(cx, |input, cx| input.set_value("portrait", window, cx));
        });
        cx.run_until_parked();
        cx.update(|_, cx| assert_eq!(workspace.read(cx).visible_recents(cx).len(), 1));
        cx.update(|window, cx| window.click("compact-app-menu", cx));
        cx.run_until_parked();
        cx.update(|window, cx| window.within("popup-menu").click(11usize, cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.click(path_id("home-recent", Path::new("photos/Portrait.png")), cx)
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            let workspace = workspace.read(cx);
            assert!(workspace.home_state.rows);
            assert_eq!(
                workspace.home_state.selected.as_deref(),
                Some(Path::new("photos/Portrait.png"))
            );
            assert_eq!(workspace.screen, crate::workspace::Screen::Home);
        });
    }

    #[gpui_kit::test]
    fn home_checked_files_transfer_to_batch_without_opening_an_editor(cx: &mut TestAppContext) {
        let (workspace, cx) = browser(cx);
        cx.update(|window, cx| {
            window.click(path_id("home-check", Path::new("photos/Portrait.png")), cx)
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.batch_home_selection(cx);
                assert_eq!(workspace.screen, crate::workspace::Screen::Batch);
                assert_eq!(workspace.batch.items.len(), 1);
                assert_eq!(
                    workspace.batch.items[0].path,
                    Path::new("photos/Portrait.png")
                );
                assert!(workspace.batch.items[0].selected);
                assert!(workspace.home_state.checked.is_empty());
                assert!(workspace.editor.is_none());
                // Avoid rendering Batch with deliberately nonexistent image
                // paths; its production thumbnail loader retries failures.
                workspace.screen = crate::workspace::Screen::Home;
            });
        });
    }

    #[gpui_kit::test]
    fn home_layout_keeps_presets_visible_and_scopes_gallery_scrolling(cx: &mut TestAppContext) {
        let (_, cx) = browser(cx);
        for (width, height) in [(3840., 2160.), (1280., 800.), (800., 600.)] {
            cx.simulate_resize(size(px(width), px(height)));
            cx.run_until_parked();
            cx.update(|window, _| {
                let library = window.find("home-library").bounds();
                let main = window.find("home-main").bounds();
                let scroll = window.find("home-scroll").bounds();
                let presets = window.find("home-presets").bounds();
                assert_eq!(library.right(), main.left());
                assert!(scroll.bottom() <= presets.top());
                assert!(presets.bottom() <= px(height));
                assert!(scroll.size.height > px(200.));
                if width >= 1280. {
                    assert_eq!(main.right(), window.find("home-inspector").bounds().left());
                } else {
                    assert!(window.try_find("home-inspector").is_none());
                }
            });
        }
    }
}

impl Workspace {
    pub(crate) fn home_uses_rows(&self) -> bool {
        self.home_state.rows
    }

    pub(crate) fn set_home_rows(&mut self, rows: bool, cx: &mut Context<Self>) {
        self.home_state.rows = rows;
        cx.notify();
    }

    /// Drop a file from the recent list without touching the file.
    pub fn remove_recent(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        self.recents = emulsion_io::recent::remove(path);
        self.home_state.checked.remove(path);
        if self.home_state.selected.as_deref() == Some(path) {
            self.home_state.selected = None;
        }
        self.invalidate_thumbnail(path);
        cx.notify();
    }

    pub(crate) fn invalidate_thumbnail(&mut self, path: &std::path::Path) {
        self.thumbs.remove(path);
        // An earlier read must not overwrite a newly saved document's preview.
        self.thumbs_loading.remove(path);
    }

    fn load_thumbs(&mut self, width: u32, cx: &mut Context<Self>) {
        for r in self.recents.clone() {
            if self
                .thumbs
                .get(&r.path)
                .is_some_and(|thumb| thumb.requested_width >= width)
                || self.thumbs_loading.contains_key(&r.path)
            {
                continue;
            }
            // Full saved composites can be large. Limit simultaneous decodes.
            if self.thumbs_loading.len() >= 2 {
                break;
            }
            self.thumb_generation = self.thumb_generation.wrapping_add(1);
            let generation = self.thumb_generation;
            self.thumbs_loading.insert(r.path.clone(), generation);
            let path = r.path.clone();
            cx.spawn(async move |this, cx| {
                let p = path.clone();
                let result = cx
                    .background_spawn(async move {
                        emulsion_io::thumb::thumbnail_cover(&p, width, width * 3 / 4).map(
                            |(w, h, mut rgba)| {
                                for px in rgba.as_chunks_mut::<4>().0 {
                                    px.swap(0, 2);
                                }
                                (w, h, rgba)
                            },
                        )
                    })
                    .await;
                this.update(cx, |this, cx| {
                    if this.thumbs_loading.get(&path) != Some(&generation) {
                        return;
                    }
                    this.thumbs_loading.remove(&path);
                    let previous = this.thumbs.remove(&path).and_then(|thumb| thumb.image);
                    let image = result
                        .ok()
                        .map(|(w, h, bgra)| Arc::new(bgra_image(w, h, bgra)))
                        .or(previous);
                    this.thumbs.insert(
                        path,
                        GalleryThumbnail {
                            requested_width: width,
                            image,
                        },
                    );
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    fn ensure_home_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.home_state.search.is_none() {
            let input =
                cx.new(|cx| InputState::new(window, cx).placeholder("Search work and folders…"));
            let subscription = cx.subscribe(&input, |_, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            });
            self.home_state.search = Some((input, subscription));
        }
    }

    fn unfinished(&self, path: &Path, cx: &App) -> bool {
        self.tabs.iter().any(|tab| {
            let editor = tab.read(cx);
            (editor.editor.path.as_deref() == Some(path) || editor.source.as_deref() == Some(path))
                && editor.editor.is_modified()
        })
    }

    fn visible_recents(&self, cx: &App) -> Vec<recent::Recent> {
        let query = self
            .home_state
            .search
            .as_ref()
            .map(|(input, _)| input.read(cx).value().to_lowercase())
            .unwrap_or_default();
        let query = query.trim();
        let stars = &crate::app_state::settings(cx).starred_files;
        let now = recent::now();
        self.recents
            .iter()
            .filter(|entry| {
                let folder = self
                    .home_state
                    .folder
                    .as_ref()
                    .is_none_or(|folder| entry.path.parent() == Some(folder.as_path()));
                let search =
                    query.is_empty() || entry.path.to_string_lossy().to_lowercase().contains(query);
                let filter = match self.home_state.filter {
                    HomeFilter::All => true,
                    HomeFilter::Unfinished => self.unfinished(&entry.path, cx),
                    HomeFilter::Today => now.saturating_sub(entry.opened) < 86_400,
                    HomeFilter::Starred => stars.contains(&entry.path),
                };
                folder && search && filter
            })
            .cloned()
            .collect()
    }

    fn selected_recent(&self, cx: &App) -> Option<recent::Recent> {
        let visible = self.visible_recents(cx);
        visible
            .iter()
            .find(|entry| Some(&entry.path) == self.home_state.selected.as_ref())
            .cloned()
            .or_else(|| visible.into_iter().next())
    }

    pub(crate) fn home_header(
        &mut self,
        navigation: AnyElement,
        theme_controls: AnyElement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.ensure_home_search(window, cx);
        let p = theme::palette(cx);
        let input = self.home_state.search.as_ref().unwrap().0.clone();
        let filters = [
            ("home-filter-all", "All", HomeFilter::All),
            (
                "home-filter-unfinished",
                "Unfinished",
                HomeFilter::Unfinished,
            ),
            ("home-filter-today", "Today", HomeFilter::Today),
            ("home-filter-starred", "Starred", HomeFilter::Starred),
        ]
        .into_iter()
        .map(|(id, label, filter)| {
            control(id, label, &p)
                .selected(self.home_state.filter == filter)
                .tooltip(if filter == HomeFilter::Today {
                    "Opened in the last 24 hours"
                } else if filter == HomeFilter::Unfinished {
                    "Open documents with unsaved changes"
                } else {
                    label
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.home_state.filter = filter;
                    cx.notify();
                }))
        });
        div()
            .id("home-header")
            .test_support()
            .flex()
            .items_center()
            .w_full()
            .min_w_0()
            .h(rems(2.25))
            .px_2()
            .gap_2()
            .child(
                div()
                    .id("home-brand")
                    .test_support()
                    .flex()
                    .items_center()
                    .gap_2()
                    .flex_none()
                    .child(
                        img(app_icon())
                            .size(rems(1.25))
                            .object_fit(ObjectFit::Contain),
                    )
                    .text_size(rems(0.813))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Emulsion"),
            )
            .child(
                div()
                    .id("home-header-filters")
                    .test_support()
                    .flex()
                    .items_center()
                    .gap_1()
                    .children(filters),
            )
            .child(
                div()
                    .id("home-window-drag")
                    .test_support()
                    .flex_1()
                    .min_w(rems(3.))
                    .h_full()
                    .window_control_area(WindowControlArea::Drag),
            )
            .child(
                Popover::new("home-header-search")
                    .trigger(
                        Button::new("home-header-search-button")
                            .icon(IconName::Search)
                            .tooltip("Search recent files")
                            .xsmall()
                            .ghost()
                            .rounded_none(),
                    )
                    .content(move |_, _, _| {
                        div()
                            .id("home-search-container")
                            .test_support()
                            .w(rems(18.75))
                            .p_1()
                            .child(Input::new(&input).small())
                            .into_any_element()
                    }),
            )
            .child(theme_controls)
            .child(div().flex().items_center().child(navigation))
            .into_any_element()
    }

    pub(crate) fn home(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.ensure_home_search(window, cx);
        self.load_thumbs(
            thumbnail_width(
                f32::from(window.viewport_size().width),
                window.scale_factor(),
            ),
            cx,
        );
        let p = theme::palette(cx);
        let width = f32::from(window.viewport_size().width) / f32::from(window.rem_size());
        let inspector = width >= 64.;
        let sidebar_width = if width < 48. { 9. } else { 11.875 };
        let center_width = (width - sidebar_width - if inspector { 15.625 } else { 0. }).max(12.);
        let columns = (center_width / 11.125).floor().clamp(1., 6.) as u16;
        let visible = self.visible_recents(cx);
        let selected = self.selected_recent(cx);
        let cells = visible
            .iter()
            .map(|entry| {
                self.home_recent(
                    entry,
                    selected.as_ref().map(|r| r.path.as_path()),
                    &p,
                    center_width >= 39.,
                    cx,
                )
            })
            .collect::<Vec<_>>();
        let mut actions = div()
            .id("home-actions")
            .test_support()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_1()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(p.line);
        if !crate::app_state::settings(cx).compact_chrome {
            for (id, label, filter) in [
                ("home-filter-all", "All", HomeFilter::All),
                (
                    "home-filter-unfinished",
                    "Unfinished",
                    HomeFilter::Unfinished,
                ),
                ("home-filter-today", "Today", HomeFilter::Today),
                ("home-filter-starred", "Starred", HomeFilter::Starred),
            ] {
                actions = actions.child(
                    control(id, label, &p)
                        .selected(self.home_state.filter == filter)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.home_state.filter = filter;
                            cx.notify();
                        })),
                );
            }
        }
        actions = actions.child(div().flex_1());
        if !inspector && let Some(entry) = &selected {
            let path = entry.path.clone();
            let starred = crate::app_state::settings(cx).starred_files.contains(&path);
            actions = actions.child(
                control("home-star-selected", "Star", &p)
                    .selected(starred)
                    .tooltip(format!("Star {}", file_name(&path)))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        crate::app_state::update_settings(cx, |settings| {
                            if settings.starred_files.contains(&path) {
                                settings.starred_files.retain(|star| star != &path);
                            } else {
                                settings.starred_files.push(path.clone());
                            }
                        });
                    })),
            );
        }
        let checked = self.home_state.checked.len();
        if checked > 0 {
            actions = actions
                .child(
                    div()
                        .text_size(rems(0.625))
                        .text_color(p.muted)
                        .child(format!("{checked} selected")),
                )
                .child(
                    control("home-batch", "Batch…", &p)
                        .disabled(self.batch.running.is_some())
                        .on_click(cx.listener(|this, _, _, cx| this.batch_home_selection(cx))),
                )
                .child(
                    control("home-clear-checked", "Clear", &p).on_click(cx.listener(
                        |this, _, _, cx| {
                            this.home_state.checked.clear();
                            cx.notify();
                        },
                    )),
                );
        }
        actions = actions.child(
            div()
                .text_size(rems(0.625))
                .text_color(p.muted)
                .child(format!("{} of {}", visible.len(), self.recents.len())),
        );
        let gallery = if cells.is_empty() {
            div()
                .id("home-empty")
                .test_support()
                .p_5()
                .text_size(rems(0.75))
                .text_color(p.muted)
                .child(if self.recents.is_empty() {
                    "No recent files yet. Open an image or start a new canvas."
                } else {
                    "No work matches these filters."
                })
                .into_any_element()
        } else if self.home_state.rows {
            div()
                .id("home-recent-rows")
                .test_support()
                .flex()
                .flex_col()
                .p_1()
                .gap_1()
                .children(cells)
                .into_any_element()
        } else {
            div()
                .id("home-recent-grid")
                .test_support()
                .grid()
                .grid_cols(columns)
                .gap(px(1.))
                .bg(p.line)
                .border_b_1()
                .border_color(p.line)
                .children(cells)
                .into_any_element()
        };
        let center = div()
            .id("home-main")
            .test_support()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(actions)
            .child(
                div()
                    .id("home-scroll")
                    .test_support()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(self.hero(center_width, &p, cx))
                    .children(self.recovered_rows(&p, cx))
                    .child(gallery),
            )
            .child(self.home_presets(&p, cx));
        div()
            .id("home")
            .test_support()
            .flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .bg(p.paper)
            .child(self.home_library(sidebar_width, &p, cx))
            .child(center)
            .when(inspector, |row| {
                row.child(self.home_inspector(selected, &p, cx))
            })
            .into_any_element()
    }

    fn home_library(&self, width: f32, p: &Palette, cx: &Context<Self>) -> AnyElement {
        let mut folders: BTreeMap<PathBuf, usize> = BTreeMap::new();
        for recent in &self.recents {
            if let Some(folder) = recent.path.parent() {
                *folders.entry(folder.to_path_buf()).or_default() += 1;
            }
        }
        let mut library = div()
            .id("home-library-list")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p_2()
            .gap_1()
            .child(
                div()
                    .px_2()
                    .py_1()
                    .text_size(rems(0.625))
                    .text_color(p.muted)
                    .child("LIBRARY"),
            )
            .child(
                control(
                    "home-folder-all",
                    format!("All work  {}", self.recents.len()),
                    p,
                )
                .w_full()
                .justify_start()
                .selected(self.home_state.folder.is_none())
                .on_click(cx.listener(|this, _, _, cx| {
                    this.home_state.folder = None;
                    cx.notify();
                })),
            );
        for (folder, count) in folders {
            let label = folder
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| folder.display().to_string());
            library = library.child(
                control(
                    path_id("home-folder", &folder),
                    format!("{label}  {count}"),
                    p,
                )
                .tooltip(folder.display().to_string())
                .w_full()
                .justify_start()
                .selected(self.home_state.folder.as_ref() == Some(&folder))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.home_state.folder = Some(folder.clone());
                    cx.notify();
                })),
            );
        }
        div()
            .id("home-library")
            .test_support()
            .flex()
            .flex_col()
            .flex_none()
            .w(rems(width))
            .min_h_0()
            .border_r_1()
            .border_color(p.line)
            .child(library)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_2()
                    .border_t_1()
                    .border_color(p.line)
                    .child(
                        div()
                            .text_size(rems(0.625))
                            .text_color(p.muted)
                            .child("IMPORT"),
                    )
                    .child(
                        control("home-import-files", "Open files…", p)
                            .w_full()
                            .justify_start()
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(crate::actions::Open), cx)
                            }),
                    )
                    .child(
                        control("home-import-folder", "Batch folder…", p)
                            .w_full()
                            .justify_start()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.screen = crate::workspace::Screen::Batch;
                                this.refresh_batch_recipes(cx);
                                this.pick_batch_folder(cx);
                            })),
                    )
                    .child(
                        div()
                            .text_size(rems(0.625))
                            .text_color(p.muted)
                            .child("RAW and supported image files"),
                    ),
            )
            .into_any_element()
    }

    fn batch_home_selection(&mut self, cx: &mut Context<Self>) {
        if self.batch.running.is_some() {
            return;
        }
        let paths = self
            .recents
            .iter()
            .filter(|entry| self.home_state.checked.contains(&entry.path))
            .map(|entry| entry.path.clone())
            .collect::<Vec<_>>();
        let Some(folder) = paths
            .first()
            .and_then(|path| path.parent())
            .map(Path::to_path_buf)
        else {
            return;
        };
        self.load_batch(folder, paths, cx);
        // These pictures were explicitly checked on Home before entering Batch.
        for item in &mut self.batch.items {
            item.selected = true;
        }
        self.home_state.checked.clear();
        self.screen = crate::workspace::Screen::Batch;
        self.refresh_batch_recipes(cx);
        cx.notify();
    }

    fn recent_thumbnail(&self, path: &Path, p: &Palette) -> AnyElement {
        match self.thumbs.get(path).and_then(|thumb| thumb.image.as_ref()) {
            Some(image) => img(ImageSource::Render(image.clone()))
                .size_full()
                .object_fit(ObjectFit::Cover)
                .into_any_element(),
            None => div()
                .size_full()
                .bg(p.stage)
                .flex()
                .items_center()
                .justify_center()
                .text_size(rems(0.625))
                .text_color(p.muted)
                .child(file_kind(path))
                .into_any_element(),
        }
    }

    fn home_recent(
        &self,
        recent: &recent::Recent,
        selected: Option<&Path>,
        p: &Palette,
        wide: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        let path = recent.path.clone();
        let checked_path = path.clone();
        let forget = path.clone();
        let active = selected == Some(path.as_path());
        let name = file_name(&path);
        let kind = file_kind(&path);
        let unfinished = self.unfinished(&path, cx);
        let star = crate::app_state::settings(cx).starred_files.contains(&path);
        let mut content = div().flex().min_w_0();
        if self.home_state.rows {
            content = content
                .items_center()
                .gap_2()
                .w_full()
                .child(
                    div()
                        .flex_none()
                        .w(rems(2.125))
                        .h(rems(1.375))
                        .overflow_hidden()
                        .child(self.recent_thumbnail(&path, p)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_ellipsis()
                        .text_size(rems(0.719))
                        .child(format!(
                            "{}{}{}",
                            if unfinished { "• " } else { "" },
                            name,
                            if star { " ★" } else { "" }
                        )),
                )
                .child(
                    div()
                        .w_8()
                        .text_size(rems(0.625))
                        .text_color(p.muted)
                        .child(kind),
                )
                .when(wide, |row| {
                    row.child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_ellipsis()
                            .text_size(rems(0.625))
                            .text_color(p.muted)
                            .child(recent.summary.replace("nodes", "layers")),
                    )
                })
                .child(
                    div()
                        .w(rems(4.75))
                        .text_size(rems(0.625))
                        .text_color(p.muted)
                        .text_right()
                        .child(recent::ago(recent.opened)),
                );
        } else {
            content = content
                .flex_col()
                .w_full()
                .child(
                    div()
                        .relative()
                        .w_full()
                        .aspect_ratio(4. / 3.)
                        .overflow_hidden()
                        .child(self.recent_thumbnail(&path, p))
                        .child(
                            div()
                                .absolute()
                                .left_2()
                                .top_2()
                                .px_1()
                                .bg(p.chrome.opacity(0.85))
                                .text_size(rems(0.625))
                                .text_color(p.chrome_fg)
                                .child(kind),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .px_3()
                        .pt_2()
                        .pb_3()
                        .child(
                            div()
                                .text_size(rems(0.844))
                                .font_weight(FontWeight::MEDIUM)
                                .text_ellipsis()
                                .child(format!(
                                    "{}{}{}",
                                    if unfinished { "• " } else { "" },
                                    name,
                                    if star { " ★" } else { "" }
                                )),
                        )
                        .child(
                            div()
                                .text_size(rems(0.625))
                                .text_color(p.muted)
                                .text_ellipsis()
                                .child(format!(
                                    "{} · {}",
                                    recent::ago(recent.opened),
                                    recent.summary.replace("nodes", "layers")
                                )),
                        ),
                );
        }
        let check = Checkbox::new(path_id("home-check", &path))
            .small()
            .checked(self.home_state.checked.contains(&path))
            .accessibility_label(format!("Select {name} for batch"))
            .tooltip("Select for batch")
            .on_click(cx.listener(move |this, value, _, cx| {
                if *value {
                    this.home_state.checked.insert(checked_path.clone());
                } else {
                    this.home_state.checked.remove(&checked_path);
                }
                cx.notify();
            }));
        let forget_button = control(path_id("home-forget", &path), "×", p)
            .ghost()
            .accessibility_label(format!("Forget {name}"))
            .tooltip("Forget this entry; keep the file")
            .on_click(cx.listener(move |this, _, _, cx| {
                this.home_state.checked.remove(&forget);
                if this.home_state.selected.as_ref() == Some(&forget) {
                    this.home_state.selected = None;
                }
                this.remove_recent(&forget, cx);
            }));
        let select = Button::new(path_id("home-recent", &path))
            .ghost()
            .rounded_none()
            .p_0()
            .h_auto()
            .min_w_0()
            .w_full()
            .text_color(p.ink)
            .bg(if active { p.panel } else { p.paper })
            // A delayed tooltip on the whole card can survive the double-click
            // that replaces Home with the editor. Keep the instruction in the
            // accessible name instead of painting stale Home UI over the image.
            .accessibility_label(format!("Select {name}; double-click to open"))
            .child(content)
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                this.home_state.selected = Some(path.clone());
                if event.click_count() >= 2 {
                    this.open_path(path.clone(), window, cx);
                }
                cx.notify();
            }));
        if self.home_state.rows {
            div()
                .flex()
                .items_center()
                .gap_2()
                .h(rems(1.875))
                .px_2()
                .min_w_0()
                .border_1()
                .border_color(if active { p.ink } else { p.paper })
                .bg(if active { p.panel } else { p.paper })
                .child(check)
                .child(div().flex_1().min_w_0().child(select))
                .child(forget_button)
                .into_any_element()
        } else {
            div()
                .relative()
                .min_w_0()
                .bg(p.paper)
                .border_1()
                .border_color(if active { p.ink } else { p.paper })
                .child(select)
                .child(div().absolute().left_2().top(rems(2.)).child(check))
                .child(
                    div()
                        .absolute()
                        .right_1()
                        .top_1()
                        .bg(p.chrome.opacity(0.85))
                        .child(forget_button),
                )
                .into_any_element()
        }
    }

    fn home_inspector(
        &self,
        selected: Option<recent::Recent>,
        p: &Palette,
        cx: &Context<Self>,
    ) -> AnyElement {
        let panel = div()
            .id("home-inspector")
            .test_support()
            .flex()
            .flex_col()
            .flex_none()
            .w(rems(15.625))
            .min_h_0()
            .overflow_y_scroll()
            .border_l_1()
            .border_color(p.line)
            .p_3()
            .gap_2();
        let Some(recent) = selected else {
            return panel
                .child(
                    div()
                        .text_size(rems(0.75))
                        .text_color(p.muted)
                        .child("Select work to see its details."),
                )
                .into_any_element();
        };
        let path = recent.path.clone();
        let star_path = path.clone();
        let starred = crate::app_state::settings(cx).starred_files.contains(&path);
        let mut details = vec![
            ("Type", file_kind(&path)),
            ("Layers", recent.summary.replace("nodes", "layers")),
            (
                "Folder",
                path.parent()
                    .map(|folder| folder.display().to_string())
                    .unwrap_or_default(),
            ),
            ("Opened", recent::ago(recent.opened)),
        ];
        if let Some(editor) = self.tabs.iter().find(|editor| {
            let view = editor.read(cx);
            view.editor.path.as_deref() == Some(path.as_path())
                || view.source.as_deref() == Some(path.as_path())
        }) {
            let view = editor.read(cx);
            details.push((
                "Size",
                format!(
                    "{}×{} · {} bit",
                    view.editor.doc.width, view.editor.doc.height, view.editor.doc.source_depth
                ),
            ));
            details.push(("History", format!("{} steps", view.editor.history.len())));
        }
        panel
            .child(
                div()
                    .h(rems(9.375))
                    .w_full()
                    .overflow_hidden()
                    .child(self.recent_thumbnail(&path, p)),
            )
            .child(
                div()
                    .text_size(rems(0.813))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(file_name(&path)),
            )
            .children(details.into_iter().map(|(key, value)| {
                div()
                    .flex()
                    .gap_2()
                    .child(
                        div()
                            .flex_none()
                            .text_size(rems(0.625))
                            .text_color(p.muted)
                            .child(key),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_right()
                            .text_size(rems(0.625))
                            .child(value),
                    )
            }))
            .when(self.unfinished(&path, cx), |panel| {
                panel.child(
                    div()
                        .border_1()
                        .border_color(p.accent)
                        .p_2()
                        .text_size(rems(0.625))
                        .child("Unsaved changes · resumes in its open tab"),
                )
            })
            .child(
                control(
                    "home-toggle-star",
                    if starred { "★ Starred" } else { "☆ Star" },
                    p,
                )
                .selected(starred)
                .on_click(cx.listener(move |_, _, _, cx| {
                    crate::app_state::update_settings(cx, |settings| {
                        if settings.starred_files.contains(&star_path) {
                            settings.starred_files.retain(|path| path != &star_path);
                        } else {
                            settings.starred_files.push(star_path.clone());
                        }
                    });
                })),
            )
            .child(
                control("home-inspector-open", "Open in editor", p).on_click(
                    cx.listener(move |this, _, window, cx| {
                        this.open_path(path.clone(), window, cx)
                    }),
                ),
            )
            .into_any_element()
    }

    fn recovered_rows(&self, p: &Palette, cx: &Context<Self>) -> Option<AnyElement> {
        if self.recovered.is_empty() {
            return None;
        }
        let rows = self.recovered.iter().map(|(path, time)| {
            let open = path.clone();
            let discard = path.clone();
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .text_size(rems(0.813))
                        .child(crate::workspace::recovered_name(path)),
                )
                .child(
                    div()
                        .text_size(rems(0.625))
                        .text_color(p.muted)
                        .child(format!("autosaved {}", recent::ago(*time))),
                )
                .child(div().flex_1())
                .child(
                    control(path_id("home-recover", path), "Open", p).on_click(cx.listener(
                        move |this, _, window, cx| this.open_recovered(open.clone(), window, cx),
                    )),
                )
                .child(
                    control(path_id("home-discard-recovered", path), "Discard", p).on_click(
                        cx.listener(move |this, _, _, cx| this.discard_recovered(&discard, cx)),
                    ),
                )
        });
        Some(
            div()
                .id("home-recovery")
                .test_support()
                .flex()
                .flex_col()
                .flex_none()
                .gap_2()
                .p_4()
                .border_b_1()
                .border_color(p.line)
                .bg(p.panel)
                .child(
                    div()
                        .text_size(rems(0.625))
                        .text_color(p.accent)
                        .child("RECOVERED WORK · NOT SAVED LAST TIME"),
                )
                .children(rows)
                .into_any_element(),
        )
    }

    fn hero(&self, width: f32, p: &Palette, cx: &Context<Self>) -> AnyElement {
        let workspace = cx.entity().downgrade();
        let image = self
            .landing
            .as_ref()
            .map(|images| images.for_aspect(width / 13.125));
        div()
            .id("hero")
            .test_support()
            .relative()
            .w_full()
            .h(rems(13.125))
            .flex_none()
            .overflow_hidden()
            .bg(p.chrome)
            .border_b_1()
            .border_color(p.line)
            .when_some(image, |hero, (image, (x, y))| {
                hero.child(
                    img(ImageSource::Render(image))
                        .size_full()
                        .object_fit(ObjectFit::Cover)
                        .object_position(x, y),
                )
            })
            .child(div().absolute().inset_0().bg(linear_gradient(
                90.,
                linear_color_stop(p.chrome.opacity(0.88), 0.),
                linear_color_stop(p.chrome.opacity(0.), 0.75),
            )))
            .child(
                div()
                    .absolute()
                    .left(rems(1.75))
                    .right(rems(1.75))
                    .bottom(rems(1.375))
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .text_size(rems(0.625))
                            .text_color(p.chrome_fg.opacity(0.75))
                            .child(format!("{} RECENT FILES", self.recents.len())),
                    )
                    .child(
                        div()
                            .text_size(rems(2.375))
                            .font_weight(FontWeight::SEMIBOLD)
                            .line_height(relative(0.95))
                            .text_color(p.chrome_fg)
                            .child("Every edit,")
                            .child("still undoable."),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_2()
                            .child(control("open", "Open a file", p).small().on_click(
                                |_, window, cx| {
                                    window.dispatch_action(Box::new(crate::actions::Open), cx)
                                },
                            ))
                            .child(control("new", "New canvas", p).small().on_click(
                                cx.listener(|this, _, window, cx| this.new_document(window, cx)),
                            ))
                            .child(
                                control("home-new-more", "···", p)
                                    .small()
                                    .tooltip("More ways to start")
                                    .dropdown_menu(move |menu, _, _| {
                                        let transparent = workspace.clone();
                                        let landing = workspace.clone();
                                        menu.item(
                                            PopupMenuItem::new("New transparent canvas").on_click(
                                                move |_, window, cx| {
                                                    transparent
                                                        .update(cx, |this, cx| {
                                                            this.new_document_with(None, window, cx)
                                                        })
                                                        .ok();
                                                },
                                            ),
                                        )
                                        .item(
                                            PopupMenuItem::new("Edit this image").on_click(
                                                move |_, window, cx| {
                                                    landing
                                                        .update(cx, |this, cx| {
                                                            this.open_landing(window, cx)
                                                        })
                                                        .ok();
                                                },
                                            ),
                                        )
                                    }),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn home_presets(&self, p: &Palette, cx: &Context<Self>) -> AnyElement {
        let mut presets = div()
            .id("home-presets")
            .test_support()
            .flex()
            .flex_none()
            .flex_wrap()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .border_t_1()
            .border_color(p.line)
            .child(
                div()
                    .text_size(rems(0.625))
                    .text_color(p.muted)
                    .child("NEW"),
            );
        for (id, name, width, height, depth) in [
            ("home-preset-photo", "Photo 3:2", 3000, 2000, 8),
            ("home-preset-square", "Square", 2048, 2048, 8),
            ("home-preset-print", "A4 print", 3508, 4961, 16),
            ("home-preset-draw", "Draw 4K", 3840, 2160, 8),
        ] {
            presets = presets.child(
                control(id, name, p)
                    .h_auto()
                    .py_1()
                    .child(
                        div()
                            .text_size(rems(0.563))
                            .text_color(p.muted)
                            .child(format!("{width}×{height} · {depth} bit")),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let mut document = Document::new(width, height);
                        document.source_depth = depth;
                        if id == "home-preset-print" {
                            document.resolution = 300.;
                        }
                        let node = Node::new(
                            0,
                            "Background",
                            NodeKind::Fill {
                                rgba: [255, 255, 255, 255],
                            },
                        );
                        let _ = Command::AddNode {
                            node: Box::new(node),
                            slot: emulsion_core::command::Slot::TOP,
                        }
                        .apply(&mut document);
                        this.install(document, None, None, None, name.into(), window, cx);
                    })),
            );
        }
        presets
            .child(
                control("home-preset-custom", "Custom…", p).on_click(cx.listener(
                    |this, _, window, cx| {
                        this.new_document(window, cx);
                        window.dispatch_action(Box::new(crate::actions::CanvasSizeDialog), cx);
                    },
                )),
            )
            .into_any_element()
    }
}
