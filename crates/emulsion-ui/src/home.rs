//! Local project dashboard following the supplied Home handoff.
#[path = "home_layout.rs"]
mod layout;
#[path = "home_recency.rs"]
pub(crate) mod recency;

use crate::theme::{self, Palette};
use crate::viewport::bgra_image;
use crate::workspace::{Workspace, destinations::Destination};
use emulsion_core::{Command, Document, Node, NodeKind};
use emulsion_io::recent;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt, DropdownMenu, PopupMenuItem};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::{Disableable, IconName, Selectable, Sizable};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

pub(crate) struct GalleryThumbnail {
    requested_width: u32,
    image: Option<Arc<RenderImage>>,
    file_bytes: Option<u64>,
}

fn thumbnail_width(card_width: f32, scale_factor: f32) -> u32 {
    let pixels = (card_width * scale_factor).ceil() as u32;
    // Bucket resize requests to avoid rebuilding on every single-pixel drag.
    pixels.div_ceil(128).saturating_mul(128).clamp(128, 1024)
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
    pub(crate) projects: crate::home_projects::HomeProjects,
    search: Option<(Entity<InputState>, Subscription)>,
    rows: bool,
    pub(crate) details: bool,
    management: bool,
    sort_name: bool,
    recent_expanded: [bool; 2],
    recent_pages: [usize; 3],
    pub(crate) unfiled: bool,
    pub(crate) folder: Option<PathBuf>,
    filter: HomeFilter,
    pub(crate) selected: Option<PathBuf>,
    pub(crate) checked: HashSet<PathBuf>,
    pub(crate) cloud_files: bool,
    pub(crate) page: usize,
    thumbnail_order: VecDeque<PathBuf>,
}

fn path_id(prefix: &'static str, path: &Path) -> ElementId {
    (ElementId::from(prefix), path.to_string_lossy().into_owned()).into()
}

pub(crate) fn file_name(path: &Path) -> String {
    path.file_stem()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn file_kind(path: &Path) -> String {
    path.extension()
        .map(|kind| kind.to_string_lossy().to_uppercase())
        .unwrap_or_default()
}

pub(crate) fn control(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    p: &Palette,
) -> Button {
    Button::new(id)
        .label(label)
        .xsmall()
        .rounded_none()
        .bg(p.soft_bg)
        .border_color(p.line)
        .text_color(p.ink)
}

pub(crate) fn app_icon() -> Arc<Image> {
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
    use gpui_kit::InputEvent as _;
    use gpui_kit::component::Root;
    use gpui_kit::test::TestWindowExt;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn thumbnail_requests_follow_card_size_and_display_scale() {
        assert_eq!(thumbnail_width(230., 1.), 256);
        assert_eq!(thumbnail_width(230., 2.), 512);
        assert_eq!(thumbnail_width(64., 1.), 128);
        assert_eq!(thumbnail_width(900., 4.), 1024);
    }

    #[gpui_kit::test]
    fn home_scrolling_preserves_card_geometry_and_selection(cx: &mut TestAppContext) {
        let (workspace, cx) = browser(cx);
        let entries: Vec<_> = (0..47)
            .map(|i| recent::Recent {
                path: format!("photos/scroll-{i:03}.png").into(),
                opened: recent::now(),
                summary: String::new(),
            })
            .collect();
        cx.update(|_, cx| {
            workspace.update(cx, |ws, cx| {
                ws.home_state.sort_name = true;
                ws.home_state.selected = Some(entries[0].path.clone());
                ws.recents = entries.clone();
                for entry in &entries {
                    ws.thumbs.insert(
                        entry.path.clone(),
                        GalleryThumbnail {
                            requested_width: 2048,
                            image: None,
                            file_bytes: None,
                        },
                    );
                }
                cx.notify();
            });
        });
        cx.run_until_parked();
        let (card, sidebar) = cx.update(|window, _| {
            (
                window
                    .find(path_id("home-file-card", &entries[0].path))
                    .bounds(),
                window.find("home-library").bounds(),
            )
        });
        for _ in 0..2 {
            for dy in [-100., 100.] {
                cx.update(|window, cx| {
                    window.scroll(
                        "home-scroll",
                        ScrollDelta::Pixels(point(px(0.), px(dy))),
                        cx,
                    );
                });
                cx.run_until_parked();
                cx.update(|window, _| {
                    let moved = window
                        .find(path_id("home-file-card", &entries[0].path))
                        .bounds();
                    assert_eq!(moved.size, card.size);
                    assert_eq!(
                        moved.top(),
                        card.top() + px(if dy < 0. { -100. } else { 0. })
                    );
                });
            }
        }
        cx.update(|window, cx| {
            assert_eq!(
                window
                    .find(path_id("home-file-card", &entries[0].path))
                    .bounds(),
                card
            );
            assert_eq!(window.find("home-library").bounds(), sidebar);
            let last = window
                .find(path_id("home-file-card", &entries.last().unwrap().path))
                .bounds();
            assert!((last.size.width - card.size.width).abs() <= px(1.));
            assert_eq!(
                workspace.read(cx).home_state.selected.as_ref(),
                Some(&entries[0].path)
            );
        });
        for rows in [false, true] {
            cx.update(|_, cx| {
                workspace.update(cx, |ws, cx| ws.set_home_rows(rows, cx));
            });
            cx.run_until_parked();
            cx.update(|window, cx| {
                let last = &entries.last().unwrap().path;
                assert!(window.try_find(path_id("home-recent", last)).is_none());
                window.scroll(
                    "home-scroll",
                    ScrollDelta::Pixels(point(px(0.), px(-50_000.))),
                    cx,
                );
                assert!(window.find(path_id("home-recent", last)).visible());
                assert!(
                    window
                        .try_find(path_id("home-recent", &entries[0].path))
                        .is_none()
                );
                window.click(path_id("home-recent", last), cx);
                assert_eq!(workspace.read(cx).home_state.selected.as_ref(), Some(last));
                window.scroll(
                    "home-scroll",
                    ScrollDelta::Pixels(point(px(0.), px(50_000.))),
                    cx,
                );
                assert!(
                    window
                        .find(path_id("home-recent", &entries[0].path))
                        .visible()
                );
            });
        }
    }

    #[gpui_kit::test]
    fn many_home_cards_keep_their_height_and_recent_pages_keep_previews(cx: &mut TestAppContext) {
        let (workspace, cx) = browser(cx);
        let entries: Vec<_> = (0..144)
            .map(|i| recent::Recent {
                path: format!("photos/preview-{i:03}.png").into(),
                opened: recent::now().saturating_sub(i),
                summary: "1 layer".into(),
            })
            .collect();
        let preview = Arc::new(bgra_image(1, 1, vec![0, 0, 0, 255]));
        cx.update(|_, cx| {
            workspace.update(cx, |ws, cx| {
                ws.home_state.sort_name = true;
                ws.recents = entries.clone();
                ws.thumbs.clear();
                ws.home_state.thumbnail_order.clear();
                for entry in &entries[..48] {
                    ws.thumbs.insert(
                        entry.path.clone(),
                        GalleryThumbnail {
                            requested_width: 512,
                            image: Some(preview.clone()),
                            file_bytes: Some(10),
                        },
                    );
                }
                cx.notify();
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            let card = window
                .find(path_id("home-file-card", &entries[0].path))
                .bounds();
            assert!(
                card.size.height > card.size.width * 0.625 + px(40.),
                "{card:?}"
            );
            let grid = window.find("home-recent-grid").bounds();
            assert!(grid.size.height > window.viewport_size().height);
            workspace.update(cx, |ws, cx| {
                ws.home_state.page = 1;
                cx.notify();
            });
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            workspace.update(cx, |ws, cx| {
                assert!(Arc::ptr_eq(
                    ws.thumbs[&entries[0].path].image.as_ref().unwrap(),
                    &preview
                ));
                ws.home_state.page = 0;
                cx.notify();
            })
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            workspace.update(cx, |ws, cx| {
                assert!(Arc::ptr_eq(
                    ws.thumbs[&entries[0].path].image.as_ref().unwrap(),
                    &preview
                ));
                ws.home_state.page = 2;
                cx.notify();
            })
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            let ws = workspace.read(cx);
            assert!(ws.thumbs.len() <= 96);
            assert!(ws.home_state.thumbnail_order.len() <= 96);
            assert!(
                !ws.thumbs.contains_key(&entries[48].path),
                "least-recent page is evicted"
            );
            assert!(ws.thumbs.contains_key(&entries[0].path));
        });
    }

    #[gpui_kit::test]
    fn home_toolbar_arrows_move_focus_and_space_applies_filter(cx: &mut TestAppContext) {
        let (workspace, cx) = browser(cx);
        cx.update(|window, cx| {
            // Mouse clicks intentionally preserve editor/input focus in Kit.
            // Enter the toolbar through keyboard traversal instead.
            for _ in 0..80 {
                window.focus_next(cx);
                window.render_frame(cx);
                if window.find(("home-kind", 0usize)).focused() == Some(true) {
                    break;
                }
            }
            assert_eq!(window.find(("home-kind", 0usize)).focused(), Some(true));
            window.press("right", cx);
            assert_eq!(window.find(("home-kind", 1usize)).focused(), Some(true));
            assert_eq!(workspace.read(cx).home_state.projects.kind, None);
            window.press("space", cx);
            window.dispatch_event(
                KeyUpEvent {
                    keystroke: Keystroke::parse("space").unwrap(),
                }
                .to_platform_input(),
                cx,
            );
            window.render_frame(cx);
            assert_eq!(
                workspace.read(cx).home_state.projects.kind,
                Some(emulsion_core::creation::CanvasKind::Photo)
            );
            window.press("left", cx);
            window.press("left", cx);
            assert_eq!(window.find(("home-kind", 4usize)).focused(), Some(true));
        });
        cx.simulate_resize(size(px(680.), px(800.)));
        cx.run_until_parked();
        cx.update(|window, _| {
            for id in ["home-sort", "home-grid", "home-list", "home-details"] {
                let bounds = window.find(id).bounds();
                assert!(
                    bounds.right() <= window.viewport_size().width,
                    "{id}: {bounds:?}"
                );
            }
        });
    }

    #[gpui_kit::test]
    fn home_empty_clear_filters_preserves_project_and_new_uses_it(cx: &mut TestAppContext) {
        let (workspace, cx) = browser(cx);
        cx.update(|window, cx| {
            workspace.update(cx, |this, cx| {
                this.home_state.projects.folder = Some(42);
                this.home_state.projects.kind = Some(emulsion_core::creation::CanvasKind::Diagram);
                let input = this.home_state.search.as_ref().unwrap().0.clone();
                input.update(cx, |input, cx| input.set_value("missing", window, cx));
                cx.notify();
            });
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.click("home-empty-clear", cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            let state = workspace.read(cx);
            assert_eq!(state.home_state.projects.folder, Some(42));
            assert_eq!(state.home_state.projects.kind, None);
            assert!(state.home_search_query(cx).is_empty());
            assert!(window.try_find("home-empty-clear").is_none());
            window.click("home-empty-new", cx);
        });
        cx.run_until_parked();
        cx.update(|window, _| assert!(window.find("new-canvas-form").visible()));
    }

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
                            file_bytes: None,
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
                    cx.update(|window, cx| window.click("window-menu-button", cx));
                    cx.run_until_parked();
                    cx.update(|window, cx| window.within("popup-menu").click(0usize, cx));
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
        cx.update(|window, cx| window.click("home-more", cx));
        cx.run_until_parked();
        cx.update(|window, cx| window.click("home-locations", cx));
        cx.run_until_parked();
        cx.update(|window, cx| window.click(path_id("home-folder", Path::new("prints")), cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert_eq!(
                workspace.read(cx).visible_recents(cx)[0].path,
                Path::new("prints/Poster.ora")
            )
        });
        cx.update(|window, cx| window.click("home-folder-all", cx));
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
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
        cx.update(|window, cx| window.click("workspace-view-menu-button", cx));
        cx.run_until_parked();
        cx.update(|window, cx| window.within("popup-menu").click(3usize, cx));
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
        cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.batch_home_selection(window, cx);
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
    fn home_layout_matches_dashboard_and_scopes_content_scrolling(cx: &mut TestAppContext) {
        let (_, cx) = browser(cx);
        for (width, height) in [(3840., 2160.), (1280., 800.), (800., 600.)] {
            cx.simulate_resize(size(px(width), px(height)));
            cx.run_until_parked();
            cx.update(|window, _| {
                let library = window.find("home-library").bounds();
                let main = window.find("home-main").bounds();
                let scroll = window.find("home-scroll").bounds();
                assert_eq!(library.size.width, px(220.));
                assert!(library.right() <= main.left());
                assert!(main.size.width <= px(1240.));
                assert!(scroll.bottom() <= px(height));
                assert!(scroll.size.height > px(200.));
                assert!(window.try_find("hero").is_none());
                assert!(window.try_find("home-inspector").is_none());
                assert!(window.find("home-welcome").visible());
                for name in ["Photo", "Paint", "Design", "Diagram", "Library"] {
                    let card = window.find((ElementId::from("home-start"), name)).bounds();
                    assert!(card.left() >= main.left() && card.right() <= main.right());
                    assert_eq!(card.size.height, px(132.));
                }
            });
        }
    }

    #[gpui_kit::test]
    fn dashboard_projects_filters_sort_and_details_use_local_records(cx: &mut TestAppContext) {
        use emulsion_core::creation::CanvasKind;
        use emulsion_io::creative_library::{ProjectFolder, ProjectRecord};
        let (workspace, cx) = browser(cx);
        cx.update(|_, cx| {
            workspace.update(cx, |this, cx| {
                let catalog = &mut this.home_state.projects.catalog;
                catalog.folders = vec![ProjectFolder {
                    id: 50,
                    name: "Campaign".into(),
                    cloud_id: None,
                }];
                catalog.projects = this
                    .recents
                    .iter()
                    .enumerate()
                    .map(|(i, r)| ProjectRecord {
                        id: i as u64 + 1,
                        path: r.path.clone(),
                        name: if i == 0 { "Z portrait" } else { "A poster" }.into(),
                        kind: Some(if i == 0 {
                            CanvasKind::Photo
                        } else {
                            CanvasKind::Design
                        }),
                        kind_override: None,
                        folder: if i == 1 { Some(50) } else { None },
                        trashed: false,
                        opened: r.opened,
                        summary: r.summary.clone(),
                    })
                    .collect();
                cx.notify();
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.click(("home-project-card", 50u64), cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            let entries = workspace.read(cx).visible_recents(cx);
            assert_eq!(entries.len(), 1);
            assert!(entries[0].path.ends_with("Poster.ora"));
            window.click("home-filter-all", cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.click("home-sort", cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(
                workspace.read(cx).visible_recents(cx)[0]
                    .path
                    .ends_with("Poster.ora")
            );
            window.click(("home-kind", 1usize), cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert_eq!(workspace.read(cx).visible_recents(cx).len(), 1);
            assert!(
                workspace.read(cx).visible_recents(cx)[0]
                    .path
                    .ends_with("Portrait.png")
            );
            window.click("home-list", cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.find("home-recent-rows").visible());
            window.click("home-details", cx);
        });
        cx.run_until_parked();
        cx.update(|window, _| assert!(window.find("home-inspector").visible()));
    }

    #[gpui_kit::test]
    fn home_project_actions_rename_and_delete_without_removing_files(cx: &mut TestAppContext) {
        use emulsion_io::creative_library as library;
        let (workspace, cx) = browser(cx);
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("catalog");
        let path = dir.path().join("project.ora");
        std::fs::write(&path, b"original project contents").unwrap();
        let (catalog, (folder, file)) = library::update(&root, |catalog| {
            catalog.add_project_folder("Other campaign".into())?;
            let folder = catalog.add_project_folder("Campaign actions test".into())?;
            let file = catalog.remember_project(
                &recent::Recent {
                    path: path.clone(),
                    opened: recent::now(),
                    summary: String::new(),
                },
                None,
            )?;
            catalog.move_project(file, Some(folder))?;
            Ok((folder, file))
        })
        .unwrap();
        cx.update(|_, cx| {
            workspace.update(cx, |this, cx| {
                this.home_state.projects.catalog = catalog;
                this.home_state.projects.catalog_root = Some(root.clone());
                assert!(!this.home_state.management);
                cx.notify();
            })
        });
        cx.run_until_parked();
        let at =
            cx.update(|window, _| window.find(("home-project-card", folder)).bounds().center());
        cx.simulate_mouse_down(at, MouseButton::Right, Default::default());
        cx.simulate_mouse_up(at, MouseButton::Right, Default::default());
        cx.run_until_parked();
        cx.update(|window, _| assert!(window.find("popup-menu").visible()));
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        cx.update(|window, cx| window.click(("home-project-card-actions", folder), cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert_eq!(workspace.read(cx).home_state.projects.folder, None);
            window.within("popup-menu").click(0usize, cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.click("home-project-name-input", cx));
        cx.simulate_keystrokes("ctrl-a");
        cx.simulate_input("Renamed campaign");
        cx.update(|window, cx| window.click("ok", cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            let catalog = &workspace.read(cx).home_state.projects.catalog;
            assert_eq!(
                catalog
                    .folders
                    .iter()
                    .find(|f| f.id == folder)
                    .unwrap()
                    .name,
                "Renamed campaign"
            );
            window.click(("home-project-nav-actions", folder), cx);
        });
        assert_eq!(
            library::load(&root)
                .unwrap()
                .folders
                .iter()
                .find(|f| f.id == folder)
                .unwrap()
                .name,
            "Renamed campaign"
        );
        cx.run_until_parked();
        cx.update(|window, cx| window.within("popup-menu").click(0usize, cx));
        cx.run_until_parked();
        cx.update(|window, _| assert!(window.find("home-project-name-input").visible()));
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        cx.update(|window, cx| window.click(("home-project-nav", folder), cx));
        cx.run_until_parked();
        // The project toolbar remains available when the sidebar is hidden.
        cx.simulate_resize(size(px(480.), px(760.)));
        cx.run_until_parked();
        cx.update(|window, cx| window.click("home-current-project-actions", cx));
        cx.run_until_parked();
        cx.update(|window, cx| window.within("popup-menu").click(1usize, cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            let state = &workspace.read(cx).home_state.projects;
            assert_eq!(state.folder, None);
            assert!(!state.catalog.folders.iter().any(|f| f.id == folder));
            let record = state
                .catalog
                .projects
                .iter()
                .find(|p| p.id == file)
                .unwrap();
            assert_eq!(record.folder, None);
            assert!(!record.trashed);
        });
        let saved = library::load(&root).unwrap();
        assert!(!saved.folders.iter().any(|f| f.id == folder));
        assert_eq!(saved.folders.len(), 1);
        assert_eq!(saved.folders[0].name, "Other campaign");
        assert_eq!(
            saved.projects.iter().find(|p| p.id == file).unwrap().folder,
            None
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"original project contents");
    }

    #[gpui_kit::test]
    fn home_file_actions_and_compact_navigation_keep_local_work_reachable(cx: &mut TestAppContext) {
        let (workspace, cx) = browser(cx);
        let path = Path::new("photos/Portrait.png");
        cx.update(|window, cx| window.click("home-list", cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.find("home-list-heading").visible());
            window.click(path_id("home-file-actions", path), cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.within("popup-menu").click(1usize, cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert_eq!(
                workspace.read(cx).home_state.selected.as_deref(),
                Some(path)
            );
            assert!(window.find("home-inspector").visible());
            window.click("home-details-close", cx);
        });
        cx.run_until_parked();
        cx.simulate_resize(size(px(480.), px(760.)));
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.try_find("home-inspector").is_none());
            assert_eq!(
                window.find("home-navigation-compact").bounds().size.width,
                px(56.)
            );
            window.click("home-navigation-open", cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.click("home-filter-today", cx));
        cx.run_until_parked();
        cx.update(|_, cx| assert_eq!(workspace.read(cx).visible_recents(cx).len(), 1));
    }

    #[gpui_kit::test]
    fn photo_entry_opens_file_picker_without_creating_a_canvas(cx: &mut TestAppContext) {
        let (workspace, cx) = browser(cx);
        cx.update(|window, cx| window.click((ElementId::from("home-start"), "Photo"), cx));
        cx.run_until_parked();
        assert!(cx.did_prompt_for_paths());
        cx.update(|window, cx| {
            assert!(window.try_find("new-canvas-form").is_none());
            assert!(workspace.read(cx).tabs.is_empty());
        });
        cx.simulate_path_prompt_response(|_| None);
        cx.run_until_parked();
        cx.update(|window, cx| {
            workspace.update(cx, |this, cx| {
                this.visit_destination(Destination::Photo, window, cx)
            })
        });
        cx.run_until_parked();
        assert!(cx.did_prompt_for_paths());
        cx.simulate_path_prompt_response(|_| None);
        cx.run_until_parked();
        cx.update(|_, cx| assert!(workspace.read(cx).tabs.is_empty()));
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("photo.png");
        image::RgbaImage::from_pixel(40, 30, image::Rgba([80, 120, 160, 255]))
            .save(&path)
            .unwrap();
        cx.update(|window, cx| {
            workspace.update(cx, |this, cx| {
                this.visit_destination(Destination::Photo, window, cx)
            })
        });
        cx.run_until_parked();
        cx.simulate_path_prompt_response(|_| Some(vec![path.clone()]));
        cx.run_until_parked();
        cx.update(|window, cx| {
            let e = workspace.read(cx).editor.clone().unwrap();
            assert_eq!(
                (e.read(cx).editor.doc.width, e.read(cx).editor.doc.height),
                (40, 30)
            );
            assert_eq!(workspace.read(cx).destination(cx), Some(Destination::Photo));
            assert!(window.try_find("new-canvas-form").is_none());
            workspace.update(cx, |this, cx| {
                this.visit_destination(Destination::Photo, window, cx)
            });
            assert_eq!(workspace.read(cx).tabs.len(), 1);
        });
    }

    #[gpui_kit::test]
    fn file_context_menu_targets_its_card_and_brand_returns_home(cx: &mut TestAppContext) {
        let (workspace, cx) = browser(cx);
        let path = Path::new("prints/Poster.ora");
        cx.update(|window, cx| {
            workspace.update(cx, |this, _| {
                this.home_state.selected = Some("photos/Portrait.png".into())
            });
            window.click(path_id("home-file-actions", path), cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.within("popup-menu").click(1usize, cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert_eq!(
                workspace.read(cx).home_state.selected.as_deref(),
                Some(path)
            )
        });
        let at = cx.update(|window, _| {
            window
                .find(path_id("home-file-card", path))
                .bounds()
                .center()
        });
        cx.simulate_mouse_down(at, MouseButton::Right, Default::default());
        cx.simulate_mouse_up(at, MouseButton::Right, Default::default());
        cx.run_until_parked();
        cx.update(|window, _| assert!(window.find("popup-menu").visible()));
        cx.simulate_keystrokes("escape");
        cx.update(|window, cx| {
            workspace.update(cx, |this, cx| {
                this.install(
                    Document::new(32, 32),
                    None,
                    None,
                    None,
                    "unsaved".into(),
                    window,
                    cx,
                );
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.click("app-menu-button", cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert_eq!(workspace.read(cx).screen, crate::workspace::Screen::Home);
            assert_eq!(workspace.read(cx).tabs.len(), 1);
            assert!(window.try_find("popup-menu").is_none());
        });
    }

    #[gpui_kit::test]
    fn recent_history_is_bounded_collapsible_and_searchable(cx: &mut TestAppContext) {
        let (workspace, cx) = browser(cx);
        cx.simulate_resize(size(px(1440.), px(1600.)));
        let now = recent::now();
        let mut entries: Vec<_> = (0..30)
            .map(|i| recent::Recent {
                path: format!("photos/new-{i:02}.png").into(),
                opened: now.saturating_sub(i),
                summary: String::new(),
            })
            .collect();
        entries.extend([
            recent::Recent {
                path: "photos/fortnight.png".into(),
                opened: now - 14 * 86_400,
                summary: String::new(),
            },
            recent::Recent {
                path: "photos/month.png".into(),
                opened: now - 30 * 86_400,
                summary: String::new(),
            },
        ]);
        cx.update(|_, cx| {
            workspace.update(cx, |this, cx| {
                this.recents = entries.clone();
                this.home_state.projects.catalog.projects.clear();
                this.thumbs.clear();
                this.home_state.thumbnail_order.clear();
                // Reversed input proves Home orders by opened time before grouping.
                this.recents.reverse();
                cx.notify();
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(
                window
                    .try_find(path_id("home-file-card", &entries[0].path))
                    .is_some()
            );
            assert!(
                window
                    .try_find(path_id("home-file-card", &entries[11].path))
                    .is_some()
            );
            assert!(
                window
                    .try_find(path_id("home-file-card", &entries[12].path))
                    .is_none()
            );
            assert!(
                window
                    .try_find(path_id("home-file-card", &entries[30].path))
                    .is_none()
            );
            assert!(
                window
                    .try_find(path_id("home-file-card", &entries[31].path))
                    .is_none()
            );
            assert!(!workspace.read(cx).thumbs.contains_key(&entries[30].path));
            assert!(!workspace.read(cx).thumbs.contains_key(&entries[31].path));
            window.click(("home-age-next", 0usize), cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(
                window
                    .try_find(path_id("home-file-card", &entries[0].path))
                    .is_none()
            );
            assert!(
                window
                    .try_find(path_id("home-file-card", &entries[12].path))
                    .is_some()
            );
            window.click(("home-age-toggle", 1usize), cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(
                window
                    .try_find(path_id("home-file-card", &entries[30].path))
                    .is_some()
            );
            window.click(("home-age-toggle", 1usize), cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(
                window
                    .try_find(path_id("home-file-card", &entries[30].path))
                    .is_none()
            );
            // Search reaches old work even while its age section is collapsed.
            let input = workspace
                .read(cx)
                .home_state
                .search
                .as_ref()
                .unwrap()
                .0
                .clone();
            input.update(cx, |input, cx| input.set_value("month", window, cx));
        });
        cx.run_until_parked();
        cx.update(|window, _| {
            assert!(window.try_find("home-recent-groups").is_none());
            assert!(
                window
                    .try_find(path_id("home-file-card", &entries[31].path))
                    .is_some()
            );
        });
    }

    #[gpui_kit::test]
    fn home_paginates_large_local_collections(cx: &mut TestAppContext) {
        let (workspace, cx) = browser(cx);
        cx.update(|_, cx| {
            workspace.update(cx, |this, cx| {
                this.home_state.sort_name = true;
                this.recents = (0..1000)
                    .map(|i| recent::Recent {
                        path: format!("/tmp/emulsion-large-library/photo-{i:04}.jpg").into(),
                        opened: 1000 - i,
                        summary: String::new(),
                    })
                    .collect();
                for r in &this.recents {
                    this.thumbs.insert(
                        r.path.clone(),
                        GalleryThumbnail {
                            requested_width: 2048,
                            image: None,
                            file_bytes: None,
                        },
                    );
                }
                cx.notify();
            })
        });
        cx.run_until_parked();
        cx.update(|window, _| {
            assert!(
                window
                    .try_find(path_id(
                        "home-file-card",
                        Path::new("/tmp/emulsion-large-library/photo-0047.jpg")
                    ))
                    .is_some()
            );
            assert!(
                window
                    .try_find(path_id(
                        "home-file-card",
                        Path::new("/tmp/emulsion-large-library/photo-0048.jpg")
                    ))
                    .is_none()
            );
            assert!(window.try_find("home-pages").is_some());
        });
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
        self.forget_home_project(path, cx);
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

    fn load_thumbs(&mut self, visible: &[recent::Recent], width: u32, cx: &mut Context<Self>) {
        if self.home_state.cloud_files {
            return;
        }
        // Keep two recent pages warm instead of decoding again after each filter change.
        for r in visible {
            self.home_state.thumbnail_order.retain(|p| p != &r.path);
            self.home_state.thumbnail_order.push_back(r.path.clone());
        }
        while self.home_state.thumbnail_order.len() > 96 {
            if let Some(path) = self.home_state.thumbnail_order.pop_front() {
                self.thumbs.remove(&path);
            }
        }
        for r in visible {
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
                let (result, file_bytes) = cx
                    .background_spawn(async move {
                        let file_bytes = std::fs::metadata(&p).ok().map(|m| m.len());
                        let result = emulsion_io::thumb::thumbnail_cover(&p, width, width * 5 / 8)
                            .map(|(w, h, mut rgba)| {
                                for px in rgba.as_chunks_mut::<4>().0 {
                                    px.swap(0, 2);
                                }
                                (w, h, rgba)
                            });
                        (result, file_bytes)
                    })
                    .await;
                this.update(cx, |this, cx| {
                    if this.thumbs_loading.get(&path) != Some(&generation) {
                        return;
                    }
                    this.thumbs_loading.remove(&path);
                    if !this.home_state.thumbnail_order.contains(&path) {
                        cx.notify();
                        return;
                    }
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
                            file_bytes,
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
                cx.new(|cx| InputState::new(window, cx).placeholder(t!("home.search_placeholder")));
            let subscription = cx.subscribe(&input, |this, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    this.home_state.page = 0;
                    this.cloud_reset_page();
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

    pub(crate) fn home_search_query(&self, cx: &App) -> String {
        self.home_state
            .search
            .as_ref()
            .map(|(input, _)| input.read(cx).value().to_lowercase())
            .unwrap_or_default()
    }

    pub(crate) fn clear_home_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((input, _)) = &self.home_state.search {
            input.update(cx, |input, cx| input.set_value("", window, cx));
        }
        self.home_state.page = 0;
        self.cloud_reset_page();
    }
    fn visible_recents(&self, cx: &App) -> Vec<recent::Recent> {
        let query = self.home_search_query(cx);
        let query = query.trim();
        let stars = &crate::app_state::settings(cx).starred_files;
        let now = recent::now();
        let state = &self.home_state.projects;
        let records: std::collections::HashMap<_, _> = state
            .catalog
            .projects
            .iter()
            .map(|p| (&p.path, p))
            .collect();
        let mut entries = self.home_project_entries();
        if self.home_state.sort_name {
            entries.sort_by_key(|entry| {
                records
                    .get(&entry.path)
                    .map(|p| p.name.clone())
                    .unwrap_or_else(|| file_name(&entry.path))
                    .to_lowercase()
            });
        }
        entries
            .into_iter()
            .filter(|entry| {
                let record = records.get(&entry.path).copied();
                let search = query.is_empty()
                    || entry.path.to_string_lossy().to_lowercase().contains(query)
                    || record.is_some_and(|p| p.name.to_lowercase().contains(query));
                let filter = match self.home_state.filter {
                    HomeFilter::All => true,
                    HomeFilter::Unfinished => self.unfinished(&entry.path, cx),
                    HomeFilter::Today => now.saturating_sub(entry.opened) < 86_400,
                    HomeFilter::Starred => stars.contains(&entry.path),
                };
                search
                    && filter
                    && self
                        .home_state
                        .folder
                        .as_ref()
                        .is_none_or(|f| entry.path.parent() == Some(f.as_path()))
                    && record.is_some_and(|p| p.trashed) == state.trash
                    && state
                        .folder
                        .is_none_or(|id| record.is_some_and(|p| p.folder == Some(id)))
                    && state.kind.is_none_or(|kind| {
                        crate::home_projects::file_classification(&entry.path, record) == Some(kind)
                    })
                    && (!self.home_state.unfiled || record.is_none_or(|p| p.folder.is_none()))
            })
            .collect()
    }

    pub(crate) fn home_header(
        &mut self,
        navigation: AnyElement,
        theme_controls: AnyElement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.ensure_home_search(window, cx);
        self.ensure_home_projects(cx);
        let input = self.home_state.search.as_ref().unwrap().0.clone();
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
            .child(div().flex().items_center().child(navigation))
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
                            .tooltip(t!("home.search_tooltip"))
                            .xsmall()
                            .ghost()
                            .rounded_none(),
                    )
                    .content(move |_, window, _| {
                        div()
                            .id("home-search-container")
                            .test_support()
                            .w(rems(18.75))
                            .max_w((window.viewport_size().width - px(24.)).max(px(0.)))
                            .p_1()
                            .child(Input::new(&input).small())
                            .into_any_element()
                    }),
            )
            .child(theme_controls)
            .into_any_element()
    }

    pub(crate) fn home(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.ensure_home_search(window, cx);
        self.ensure_home_projects(cx);
        let p = theme::palette(cx);
        let width = f32::from(window.viewport_size().width) / f32::from(window.rem_size());
        let inspector = self.home_state.details;
        let docked_inspector = inspector && width >= 64.;
        let narrow = width < 40.;
        let sidebar_width = if narrow { 3.5 } else { 13.75 };
        let center_width =
            (width - sidebar_width - if docked_inspector { 15.625 } else { 0. }).clamp(12., 77.5);
        let columns = ((center_width - 4.) / 13.25).floor().clamp(1., 6.) as u16;
        let visible = self.visible_recents(cx);
        let grouped_recent = self.home_groups_recents(cx);
        self.home_state.page = self
            .home_state
            .page
            .min(visible.len().saturating_sub(1) / 48);

        let page_start = self.home_state.page * 48;
        let page = &visible[page_start..(page_start + 48).min(visible.len())];
        let card_width = if self.home_state.rows {
            64.
        } else {
            ((center_width * f32::from(window.rem_size())).min(1240.)
                - 64.
                - 12. * f32::from(columns - 1))
                / f32::from(columns)
        };
        let preview_width = thumbnail_width(card_width, window.scale_factor());
        if !grouped_recent {
            self.load_thumbs(page, preview_width, cx);
        }
        let selected = visible
            .iter()
            .find(|entry| Some(&entry.path) == self.home_state.selected.as_ref())
            .or_else(|| visible.first())
            .cloned();
        let cells = visible
            .iter()
            .skip(self.home_state.page * 48)
            .take(if self.home_state.cloud_files || grouped_recent {
                0
            } else {
                48
            })
            .map(|entry| {
                self.home_recent(
                    entry,
                    self.home_state.selected.as_deref(),
                    &p,
                    center_width >= 52.,
                    cx,
                )
            })
            .collect::<Vec<_>>();
        let gallery = if self.home_state.cloud_files {
            self.cloud_home_browser(columns, cx)
        } else if grouped_recent && !visible.is_empty() {
            self.home_recent_groups(
                &visible,
                columns,
                center_width >= 52.,
                preview_width,
                &p,
                cx,
            )
        } else if cells.is_empty() {
            self.home_empty(cx)
        } else if self.home_state.rows {
            div()
                .id("home-recent-rows")
                .test_support()
                .flex_none()
                .flex()
                .flex_col()
                .rounded(px(8.))
                .border_1()
                .border_color(p.line)
                .overflow_hidden()
                .child(self.home_list_heading(center_width >= 52., &p))
                .children(cells)
                .into_any_element()
        } else {
            div()
                .id("home-recent-grid")
                .test_support()
                .flex_none()
                .grid()
                .grid_cols(columns)
                .gap(px(12.))
                .children(cells)
                .into_any_element()
        };
        let center = div()
            .id("home-main")
            .test_support()
            .max_w(px(1240.))
            .w_full()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("home-scroll")
                    .test_support()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(32.))
                    .pt(px(28.))
                    .pb(px(32.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_none()
                            .gap(px(22.))
                            .child(self.home_welcome(&p, cx))
                            .when(!self.home_state.cloud_files, |column| {
                                column
                                    .child(self.home_starts(if narrow { 2 } else { 5 }, &p, cx))
                                    .children(self.recovered_rows(&p, cx))
                                    .children(self.home_project_cards(center_width, &p, cx))
                                    .child(self.home_file_controls(visible.len(), &p, cx))
                            })
                            .children(self.cloud_home_notice())
                            .children(self.home_project_notice())
                            .when(
                                self.home_state.management && !self.home_state.cloud_files,
                                |column| {
                                    column
                                        .child(self.home_projects_controls(&p, cx))
                                        .child(self.home_locations(cx))
                                        .child(
                                            control(
                                                "home-edit-artwork",
                                                t!("home.edit_artwork"),
                                                &p,
                                            )
                                            .on_click(
                                                cx.listener(|this, _, window, cx| {
                                                    this.open_landing(window, cx)
                                                }),
                                            ),
                                        )
                                        .child(self.home_presets(&p, cx))
                                },
                            )
                            .child(gallery)
                            .when(
                                !self.home_state.cloud_files
                                    && !grouped_recent
                                    && visible.len() > 48,
                                |column| column.child(self.home_page_controls(visible.len(), cx)),
                            ),
                    ),
            );
        div()
            .id("home")
            .test_support()
            .relative()
            .flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .bg(p.paper)
            .child(if narrow {
                self.home_navigation_menu(&p, cx)
            } else {
                self.home_dashboard_sidebar(&p, cx)
            })
            .child(div().flex().flex_1().min_w_0().child(center))
            .when(inspector, |row| {
                row.child(
                    div()
                        .flex()
                        .min_h_0()
                        .when(!docked_inspector, |panel| {
                            panel
                                .absolute()
                                .right_0()
                                .top_0()
                                .bottom_0()
                                .occlude()
                                .shadow_lg()
                        })
                        .child(self.home_inspector(selected, &p, cx)),
                )
            })
            .into_any_element()
    }

    fn home_page_controls(&self, total: usize, cx: &Context<Self>) -> AnyElement {
        let page = self.home_state.page;
        div()
            .id("home-pages")
            .test_support()
            .flex()
            .items_center()
            .gap_2()
            .child(
                Button::new("home-page-prev")
                    .label(t!("home.previous"))
                    .small()
                    .outline()
                    .disabled(page == 0)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.home_state.page = this.home_state.page.saturating_sub(1);
                        cx.notify();
                    })),
            )
            .child(t!(
                "home.page_range_files",
                start = page * 48 + 1,
                end = ((page + 1) * 48).min(total),
                total = total
            ))
            .child(
                Button::new("home-page-next")
                    .label(t!("home.next"))
                    .small()
                    .outline()
                    .disabled((page + 1) * 48 >= total)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.home_state.page += 1;
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    fn home_library(&self, width: f32, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let mut folders: BTreeMap<PathBuf, usize> = BTreeMap::new();
        for recent in &self.recents {
            if let Some(folder) = recent.path.parent() {
                *folders.entry(folder.to_path_buf()).or_default() += 1;
            }
        }
        if width < 8. {
            let owner = cx.weak_entity();
            let folder_menu = Button::new("home-folders-menu")
                .label("…")
                .accessibility_label(t!("home.folders_import_a11y"))
                .tooltip(t!("home.folders_import"))
                .small()
                .ghost()
                .dropdown_menu(move |mut menu, _, _| {
                    let all = owner.clone();
                    menu = menu.item(PopupMenuItem::new(t!("home.all_work")).on_click(
                        move |_, _, cx| {
                            all.update(cx, |this, cx| {
                                this.home_state.folder = None;
                                cx.notify();
                            })
                            .ok();
                        },
                    ));
                    for folder in folders.keys() {
                        let owner = owner.clone();
                        let path = folder.clone();
                        menu =
                            menu.item(PopupMenuItem::new(folder.display().to_string()).on_click(
                                move |_, _, cx| {
                                    owner
                                        .update(cx, |this, cx| {
                                            this.home_state.folder = Some(path.clone());
                                            cx.notify();
                                        })
                                        .ok();
                                },
                            ));
                    }
                    menu = menu.item(PopupMenuItem::new(t!("home.open_files")).on_click(
                        |_, window, cx| window.dispatch_action(Box::new(crate::actions::Open), cx),
                    ));
                    let owner = owner.clone();
                    menu.item(PopupMenuItem::new(t!("home.import_folder")).on_click(
                        move |_, window, cx| {
                            owner
                                .update(cx, |this, cx| {
                                    this.visit_destination(Destination::Library, window, cx);
                                    this.pick_batch_folder(cx);
                                })
                                .ok();
                        },
                    ))
                });
            return div()
                .id("home-locations-library")
                .test_support()
                .w(rems(width))
                .flex_none()
                .flex()
                .flex_col()
                .border_r_1()
                .border_color(p.line)
                .child(self.destination_navigation("home-destination", true, cx))
                .child(folder_menu)
                .into_any_element();
        }
        let mut library = div()
            .id("home-locations-list")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p_2()
            .gap_1()
            .child(self.destination_navigation("home-destination", width < 8., cx))
            .child(
                div()
                    .px_2()
                    .py_1()
                    .text_size(rems(0.625))
                    .text_color(p.muted)
                    .child(t!("home.library_heading")),
            )
            .child(
                control(
                    "home-folder-all",
                    format!("{}  {}", t!("home.all_work"), self.recents.len()),
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
            .id("home-locations-library")
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
                            .child(t!("home.import_heading")),
                    )
                    .child(
                        control("home-import-files-menu", t!("home.open_files"), p)
                            .w_full()
                            .justify_start()
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(crate::actions::Open), cx)
                            }),
                    )
                    .child(
                        control("home-import-folder", t!("home.batch_folder"), p)
                            .w_full()
                            .justify_start()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.set_screen(crate::workspace::Screen::Batch, window, cx);
                                this.refresh_batch_recipes(cx);
                                this.pick_batch_folder(cx);
                            })),
                    )
                    .child(
                        div()
                            .text_size(rems(0.625))
                            .text_color(p.muted)
                            .child(t!("home.import_hint")),
                    ),
            )
            .into_any_element()
    }

    fn batch_home_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.batch.running.is_some() {
            return;
        }
        let paths = self
            .home_project_entries()
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
        self.set_screen(crate::workspace::Screen::Batch, window, cx);
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
        let active = selected == Some(path.as_path());
        let name = self.home_project_name(&path);
        let kind = self.home_workspace_label(&path);
        let unfinished = self.unfinished(&path, cx);
        let star = crate::app_state::settings(cx).starred_files.contains(&path);
        let folder = self
            .home_state
            .projects
            .catalog
            .projects
            .iter()
            .find(|entry| entry.path == path)
            .and_then(|entry| entry.folder)
            .and_then(|id| {
                self.home_state
                    .projects
                    .catalog
                    .folders
                    .iter()
                    .find(|folder| folder.id == id)
            });
        let folder_name = folder.map_or_else(
            || t!("home.unfiled").into_owned(),
            |folder| folder.name.clone(),
        );
        let dot = folder.map_or(p.muted, |folder| layout::folder_color(folder.id));
        let project = || {
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .min_w_0()
                .child(div().size(px(6.)).flex_none().rounded_full().bg(dot))
                .child(div().min_w_0().text_ellipsis().child(folder_name.clone()))
        };
        let dimensions = self
            .thumbs
            .get(&path)
            .and_then(|t| t.file_bytes)
            .map(|bytes| {
                if bytes >= 1_048_576 {
                    format!("{:.1} MB", bytes as f64 / 1_048_576.)
                } else {
                    format!("{:.0} KB", bytes as f64 / 1024.)
                }
            })
            .unwrap_or_else(|| "—".into());
        let title = format!("{}{}", if unfinished { "• " } else { "" }, name);
        let mut content = div().flex().min_w_0().w_full();
        if self.home_state.rows {
            content = content
                .items_center()
                .gap(px(12.))
                .child(
                    div()
                        .flex()
                        .flex_1()
                        .min_w_0()
                        .items_center()
                        .gap(px(10.))
                        .child(
                            div()
                                .w(px(34.))
                                .h(px(24.))
                                .flex_none()
                                .rounded(px(3.))
                                .overflow_hidden()
                                .child(self.recent_thumbnail(&path, p)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_ellipsis()
                                .text_size(px(12.5))
                                .child(title),
                        )
                        .when(star, |row| row.child(layout::icon("pin", 10.))),
                )
                .when(wide, |row| {
                    row.child(
                        div()
                            .w(px(130.))
                            .flex_none()
                            .text_size(px(11.5))
                            .text_color(p.muted)
                            .child(project()),
                    )
                })
                .child(
                    div()
                        .w(px(100.))
                        .flex_none()
                        .text_size(px(11.5))
                        .text_color(p.muted)
                        .child(kind),
                )
                .when(wide, |row| {
                    row.child(
                        div()
                            .w(px(90.))
                            .flex_none()
                            .font_family(theme::MONO_FONT)
                            .text_size(px(10.5))
                            .text_color(p.muted)
                            .text_ellipsis()
                            .child(dimensions),
                    )
                })
                .child(
                    div()
                        .w(px(100.))
                        .flex_none()
                        .font_family(theme::MONO_FONT)
                        .text_size(px(10.5))
                        .text_color(p.muted)
                        .child(recency::ago(recent.opened)),
                );
        } else {
            content = content
                .flex_col()
                .child(
                    div()
                        .relative()
                        .w_full()
                        .flex_none()
                        .aspect_ratio(16. / 10.)
                        .overflow_hidden()
                        .child(self.recent_thumbnail(&path, p))
                        .child(
                            div()
                                .absolute()
                                .left(px(10.))
                                .top(px(10.))
                                .h(px(20.))
                                .px(px(7.))
                                .flex()
                                .items_center()
                                .rounded(px(5.))
                                .bg(rgba(0x0a0a0caa))
                                .text_color(gpui_kit::white())
                                .font_family(theme::MONO_FONT)
                                .text_size(px(9.5))
                                .child(kind),
                        )
                        .when(star, |thumb| {
                            thumb.child(
                                div()
                                    .absolute()
                                    .right(px(10.))
                                    .top(px(10.))
                                    .size(px(20.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(5.))
                                    .bg(rgba(0x0a0a0caa))
                                    .text_color(gpui_kit::white())
                                    .child(layout::icon("pin", 10.)),
                            )
                        }),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .px(px(12.))
                        .py(px(10.))
                        .pr(px(70.))
                        .child(
                            div()
                                .text_size(px(12.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_ellipsis()
                                .child(title),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .font_family(theme::MONO_FONT)
                                .text_size(px(10.))
                                .text_color(p.muted)
                                .child(project())
                                .child(
                                    div()
                                        .min_w_0()
                                        .text_ellipsis()
                                        .child(format!("· {}", recency::ago(recent.opened))),
                                ),
                        ),
                );
        }
        let checked_path = path.clone();
        let check = Checkbox::new(path_id("home-check", &path))
            .small()
            .checked(self.home_state.checked.contains(&path))
            .accessibility_label(t!("home.select_for_batch_a11y", name = name))
            .tooltip(t!("home.select_for_batch"))
            .on_click(cx.listener(move |this, value, _, cx| {
                if *value {
                    this.home_state.checked.insert(checked_path.clone());
                } else {
                    this.home_state.checked.remove(&checked_path);
                }
                cx.notify();
            }));
        let menu = self.home_file_menu(path.clone(), star, cx);
        let actions = Button::new(path_id("home-file-actions", &path))
            .label("•••")
            .accessibility_label(t!("home.actions_for", name = name))
            .xsmall()
            .ghost()
            .size(px(24.))
            .dropdown_menu(menu.clone());
        let card_id = path_id("home-file-card", &path);
        let sync = self.cloud_file_badge(&path, cx);
        let select = Button::new(path_id("home-recent", &path))
            .ghost()
            .rounded_none()
            .p_0()
            .h_auto()
            .min_w_0()
            .w_full()
            .text_color(p.ink)
            .bg(if active { p.soft_bg } else { p.panel })
            .accessibility_label(t!("home.select_open_a11y", name = name))
            .child(content)
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                this.home_state.selected = Some(path.clone());
                if event.click_count() >= 2 {
                    this.open_path(path.clone(), window, cx);
                }
                cx.notify();
            }));
        let card = if self.home_state.rows {
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .min_h(px(52.))
                .px(px(14.))
                .min_w_0()
                .border_b_1()
                .border_color(if active { p.accent } else { p.line })
                .bg(p.panel)
                .child(check)
                .child(div().flex_1().min_w_0().child(select))
                .child(sync)
                .child(actions)
                .context_menu(menu)
                .into_any_element()
        } else {
            div()
                .relative()
                .min_w_0()
                .rounded(px(crate::app_state::settings(cx).corners.radius() + 2.))
                .overflow_hidden()
                .bg(p.panel)
                .border_1()
                .border_color(if active { p.accent } else { p.line })
                .child(
                    div()
                        .relative()
                        .child(select)
                        .child(div().absolute().right(px(38.)).bottom(px(14.)).child(sync))
                        .child(
                            div()
                                .absolute()
                                .right(px(10.))
                                .bottom(px(14.))
                                .child(actions),
                        ),
                )
                .child(div().absolute().left(px(10.)).top(px(36.)).child(check))
                .context_menu(menu)
                .into_any_element()
        };
        div()
            .id(card_id)
            .test_support()
            .min_w_0()
            .child(layout::VisibleCard(card))
            .into_any_element()
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
            .bg(p.panel)
            .gap_2()
            .child(
                control("home-details-close", t!("home.close_details"), p)
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.home_state.details = false;
                        cx.notify();
                    })),
            );
        let Some(recent) = selected else {
            return panel
                .child(
                    div()
                        .text_size(rems(0.75))
                        .text_color(p.muted)
                        .child(t!("home.details_empty")),
                )
                .into_any_element();
        };
        let path = recent.path.clone();
        let star_path = path.clone();
        let starred = crate::app_state::settings(cx).starred_files.contains(&path);
        let mut details = vec![
            (t!("home.detail_type"), file_kind(&path)),
            (
                t!("home.detail_layers"),
                recent.summary.replace("nodes", "layers"),
            ),
            (
                t!("home.detail_folder"),
                path.parent()
                    .map(|folder| folder.display().to_string())
                    .unwrap_or_default(),
            ),
            (t!("home.detail_opened"), recency::ago(recent.opened)),
        ];
        if let Some(editor) = self.tabs.iter().find(|editor| {
            let view = editor.read(cx);
            view.editor.path.as_deref() == Some(path.as_path())
                || view.source.as_deref() == Some(path.as_path())
        }) {
            let view = editor.read(cx);
            details.push((
                t!("home.detail_size"),
                t!(
                    "home.size_value",
                    width = view.editor.doc.width,
                    height = view.editor.doc.height,
                    depth = view.editor.doc.source_depth
                )
                .into_owned(),
            ));
            details.push((
                t!("home.detail_history"),
                recency::plural(
                    view.editor.history.len(),
                    "home.steps_one",
                    "home.steps_many",
                ),
            ));
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
            .child(self.cloud_file_control(&path, cx))
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
                        .child(t!("home.unsaved_resume")),
                )
            })
            .child(
                control(
                    "home-toggle-star",
                    if starred {
                        t!("home.starred")
                    } else {
                        t!("home.star")
                    },
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
                control("home-inspector-open", t!("home.open_in_editor"), p).on_click(
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
                        .child(t!("home.autosaved", time = recency::ago(*time))),
                )
                .child(div().flex_1())
                .child(
                    control(path_id("home-recover", path), t!("home.open"), p).on_click(
                        cx.listener(move |this, _, window, cx| {
                            this.open_recovered(open.clone(), window, cx)
                        }),
                    ),
                )
                .child(
                    control(
                        path_id("home-discard-recovered", path),
                        t!("home.discard"),
                        p,
                    )
                    .on_click(
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
                        .child(t!("home.recovered_heading")),
                )
                .children(rows)
                .into_any_element(),
        )
    }

    fn home_starts(&self, columns: u16, p: &Palette, cx: &Context<Self>) -> AnyElement {
        div()
            .id("home-starts")
            .test_support()
            .grid()
            .grid_cols(columns)
            .gap(px(8.))
            .children(
                [
                    Destination::Photo,
                    Destination::Paint,
                    Destination::Design,
                    Destination::Diagram,
                    Destination::Library,
                ]
                .map(|destination| {
                    Button::new((ElementId::from("home-start"), destination.label()))
                        .accessibility_label(format!(
                            "{}: {}",
                            destination.name(),
                            destination.subtitle()
                        ))
                        .outline()
                        .items_start()
                        .flex()
                        .flex_col()
                        .min_w_0()
                        .h(px(132.))
                        .justify_between()
                        .gap(px(14.))
                        .p(px(14.))
                        .rounded(px(crate::app_state::settings(cx).corners.radius() + 2.))
                        .border_1()
                        .border_color(p.line)
                        .bg(p.panel)
                        .cursor_pointer()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .items_start()
                                .justify_between()
                                .w_full()
                                .h(px(104.))
                                .child(
                                    div()
                                        .size(px(30.))
                                        .rounded(px(8.))
                                        .bg(p.accent.opacity(0.18))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(layout::icon(
                                            layout::destination_icon(destination),
                                            15.,
                                        )),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .w_full()
                                        .min_w_0()
                                        .gap(px(2.))
                                        .child(
                                            div()
                                                .text_size(px(12.5))
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(destination.name()),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(10.5))
                                                .text_color(p.muted)
                                                .text_ellipsis()
                                                .child(destination.subtitle()),
                                        ),
                                ),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.start_destination(destination, window, cx)
                        }))
                }),
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
                    .child(t!("home.new_heading")),
            );
        for (id, name, width, height, depth) in [
            ("home-preset-photo", "home.preset_photo", 3000, 2000, 8),
            ("home-preset-square", "home.preset_square", 2048, 2048, 8),
            ("home-preset-print", "home.preset_print", 3508, 4961, 16),
            ("home-preset-draw", "home.preset_draw", 3840, 2160, 8),
        ] {
            let name = t!(name).into_owned();
            presets = presets.child(
                control(id, name.clone(), p)
                    .h_auto()
                    .py_1()
                    .child(div().text_size(rems(0.563)).text_color(p.muted).child(t!(
                        "home.size_value",
                        width = width,
                        height = height,
                        depth = depth
                    )))
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
                        this.install(document, None, None, None, name.clone(), window, cx);
                    })),
            );
        }
        presets
            .child(
                control("home-preset-custom", t!("home.custom"), p).on_click(cx.listener(
                    |this, _, window, cx| {
                        this.new_document(window, cx);
                        window.dispatch_action(Box::new(crate::actions::CanvasSizeDialog), cx);
                    },
                )),
            )
            .into_any_element()
    }
}
