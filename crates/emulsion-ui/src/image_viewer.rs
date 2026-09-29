//! Standalone file viewer. Editor services and document state are created only on Edit.
use crate::batch::preview::PreviewNavigation;
use crate::{Workspace, app_state, theme, viewport::bgra_image};
use gpui_kit::component::{
    Disableable, Sizable, TitleBar,
    button::{Button, ButtonVariants},
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use image::GenericImageView;
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

/// Only built-in still-image decoders enter the fast viewer. Layered documents,
/// vectors, RAW and converter-backed formats keep their normal editor import.
pub fn supports(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "png"
                    | "jpg"
                    | "jpeg"
                    | "jpe"
                    | "jfif"
                    | "webp"
                    | "tif"
                    | "tiff"
                    | "bmp"
                    | "dib"
                    | "gif"
                    | "tga"
                    | "icb"
                    | "vda"
                    | "vst"
                    | "pnm"
                    | "pbm"
                    | "pgm"
                    | "ppm"
                    | "pam"
                    | "ico"
                    | "hdr"
                    | "exr"
                    | "dds"
                    | "qoi"
                    | "ff"
            )
        })
}

struct Tile {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    image: Arc<RenderImage>,
}
struct Picture {
    width: u32,
    height: u32,
    preview: Arc<RenderImage>,
    preview_scale: f64,
    tiles: Vec<Tile>,
}
fn render_image(image: image::RgbaImage) -> Arc<RenderImage> {
    let (w, h) = image.dimensions();
    let mut bytes = image.into_raw();
    for p in bytes.as_chunks_mut::<4>().0 {
        p.swap(0, 2);
    }
    Arc::new(bgra_image(w, h, bytes))
}
fn decode(path: &Path) -> Result<Picture, String> {
    // Share EXIF orientation, 16-bit and ICC conversion with Photo's importer,
    // without constructing a document, graph, history, masks or an editor.
    let decoded = emulsion_io::import::decode(path).map_err(|e| e.to_string())?;
    let (width, height) = (decoded.raster.width(), decoded.raster.height());
    let rgba = image::RgbaImage::from_raw(width, height, decoded.raster.to_srgba8()).unwrap();
    drop(decoded);
    const TILE: u32 = 2048;
    if width <= TILE && height <= TILE {
        return Ok(Picture {
            width,
            height,
            preview: render_image(rgba),
            preview_scale: 1.,
            tiles: Vec::new(),
        });
    }
    let reduced = image::imageops::thumbnail(&rgba, TILE, TILE);
    let preview_scale =
        (reduced.width() as f64 / width as f64).min(reduced.height() as f64 / height as f64);
    let preview = render_image(reduced);
    let mut tiles = Vec::new();
    for y in (0..height).step_by(TILE as usize) {
        for x in (0..width).step_by(TILE as usize) {
            let w = TILE.min(width - x);
            let h = TILE.min(height - y);
            tiles.push(Tile {
                x,
                y,
                width: w,
                height: h,
                image: render_image(rgba.view(x, y, w, h).to_image()),
            });
        }
    }
    Ok(Picture {
        width,
        height,
        preview,
        preview_scale,
        tiles,
    })
}

pub struct ImageViewer {
    files: Vec<PathBuf>,
    current: usize,
    generation: u64,
    picture: Option<Arc<Picture>>,
    error: Option<String>,
    loading: bool,
    focus: FocusHandle,
    navigation: Rc<RefCell<PreviewNavigation>>,
    load_task: Option<Task<()>>,
    folder_task: Option<Task<()>>,
    workspace: Option<Entity<Workspace>>,
    last_title: String,
}
impl ImageViewer {
    pub fn new(files: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let files = files
            .into_iter()
            .map(|p| p.canonicalize().unwrap_or(p))
            .collect::<Vec<_>>();
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let mut this = Self {
            files,
            current: 0,
            generation: 0,
            picture: None,
            error: None,
            loading: false,
            focus,
            navigation: Default::default(),
            load_task: None,
            folder_task: None,
            workspace: None,
            last_title: String::new(),
        };
        this.load(cx);
        if this.files.len() == 1 {
            let path = this.files[0].clone();
            this.folder_task = Some(cx.spawn(async move |this, cx| {
                let original = path.clone();
                let siblings = cx
                    .background_spawn(async move {
                        let mut siblings = path
                            .parent()
                            .and_then(|dir| std::fs::read_dir(dir).ok())
                            .into_iter()
                            .flatten()
                            .filter_map(Result::ok)
                            .map(|e| e.path())
                            .filter(|p| supports(p) && p.is_file())
                            .collect::<Vec<_>>();
                        siblings.sort();
                        siblings
                    })
                    .await;
                this.update(cx, |this, cx| {
                    if this.workspace.is_none()
                        && this.files == [original.clone()]
                        && let Some(index) = siblings.iter().position(|p| p == &original)
                    {
                        this.files = siblings;
                        this.current = index;
                        cx.notify();
                    }
                })
                .ok();
            }));
        }
        this
    }
    fn load(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.files.get(self.current).cloned() else {
            return;
        };
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        self.picture = None;
        self.error = None;
        self.loading = true;
        self.load_task = Some(cx.spawn(async move |this, cx| {
            let source = path.clone();
            let result = cx.background_spawn(async move { decode(&source) }).await;
            this.update(cx, |this, cx| {
                if this.generation != generation || this.workspace.is_some() {
                    return;
                }
                this.loading = false;
                match result {
                    Ok(picture) => {
                        this.navigation
                            .borrow_mut()
                            .sync(&path, (picture.width, picture.height));
                        this.picture = Some(Arc::new(picture));
                    }
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }
    fn navigate(&mut self, next: bool, cx: &mut Context<Self>) {
        let index = if next {
            self.current.saturating_add(1)
        } else {
            self.current.saturating_sub(1)
        };
        if index != self.current && index < self.files.len() {
            self.current = index;
            self.load(cx);
        }
    }
    fn actual_size(&mut self, window: &Window, cx: &mut Context<Self>) {
        let mut nav = self.navigation.borrow_mut();
        nav.manual = true;
        nav.view.zoom = 1. / window.scale_factor() as f64;
        nav.view.center = (nav.dimensions.0 as f64 / 2., nav.dimensions.1 as f64 / 2.);
        cx.notify();
    }
    fn edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.files.get(self.current).cloned() else {
            return;
        };
        if self.workspace.is_some() {
            return;
        }
        app_state::start_editor_services(cx);
        self.load_task = None;
        self.folder_task = None;
        self.picture = None;
        self.generation = self.generation.wrapping_add(1);
        let workspace = cx.new(|cx| Workspace::new_for_file(window, cx));
        workspace.update(cx, |ws, cx| ws.open_photo_path(path, window, cx));
        self.workspace = Some(workspace);
        cx.notify();
    }
}
impl Render for ImageViewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(workspace) = &self.workspace {
            return div()
                .size_full()
                .child(workspace.clone())
                .into_any_element();
        }
        let p = theme::palette(cx);
        let path = self.files.get(self.current).cloned().unwrap_or_default();
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let title = format!("{name} — Emulsion Viewer");
        if title != self.last_title {
            window.set_window_title(&title);
            self.last_title = title;
        }
        let nav_layout = self.navigation.clone();
        let nav_paint = self.navigation.clone();
        let picture = self.picture.clone();
        let surface = div()
            .id("image-viewer-canvas")
            .test_support()
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_hidden()
            .bg(p.stage)
            .cursor(CursorStyle::OpenHand)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, window, cx| {
                    window.focus(&this.focus, cx);
                    let mut nav = this.navigation.borrow_mut();
                    if e.click_count == 2 {
                        nav.fit();
                    } else {
                        nav.drag = Some(e.position);
                    }
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
                let mut nav = this.navigation.borrow_mut();
                if e.pressed_button != Some(MouseButton::Left) {
                    nav.drag = None;
                    return;
                }
                if let Some(last) = nav.drag {
                    nav.drag = Some(e.position);
                    nav.pan(
                        f32::from(e.position.x - last.x) as f64,
                        f32::from(e.position.y - last.y) as f64,
                    );
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.navigation.borrow_mut().drag = None),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.navigation.borrow_mut().drag = None),
            )
            .on_scroll_wheel(cx.listener(|this, e: &ScrollWheelEvent, _, cx| {
                let d = e.delta.pixel_delta(px(20.));
                this.navigation
                    .borrow_mut()
                    .zoom((f32::from(d.y) as f64 * 0.004).exp(), e.position);
                cx.stop_propagation();
                cx.notify();
            }))
            .on_pinch(cx.listener(|this, e: &PinchEvent, _, cx| {
                this.navigation
                    .borrow_mut()
                    .zoom((1. + e.delta as f64).max(0.01), e.position);
                cx.stop_propagation();
                cx.notify();
            }))
            .child(
                canvas(
                    move |bounds, _, _| nav_layout.borrow_mut().layout(bounds),
                    move |bounds, _, window, _| {
                        let Some(picture) = picture else {
                            return;
                        };
                        let nav = nav_paint.borrow();
                        let rect = |x: u32, y: u32, w: u32, h: u32| {
                            let (sx, sy) = nav.view.doc_to_screen((x as f64, y as f64), &bounds);
                            Bounds::new(
                                point(px(sx as f32), px(sy as f32)),
                                size(
                                    px(w as f32 * nav.view.zoom as f32),
                                    px(h as f32 * nav.view.zoom as f32),
                                ),
                            )
                        };
                        if picture.tiles.is_empty()
                            || nav.view.device_zoom(window.scale_factor()) <= picture.preview_scale
                        {
                            let b = rect(0, 0, picture.width, picture.height);
                            let _ = window.paint_image(
                                b,
                                b,
                                Corners::default(),
                                picture.preview.clone(),
                                0,
                                false,
                            );
                        } else {
                            for tile in &picture.tiles {
                                let b = rect(tile.x, tile.y, tile.width, tile.height);
                                if b.right() <= bounds.left()
                                    || b.left() >= bounds.right()
                                    || b.bottom() <= bounds.top()
                                    || b.top() >= bounds.bottom()
                                {
                                    continue;
                                }
                                let _ = window.paint_image(
                                    b,
                                    b,
                                    Corners::default(),
                                    tile.image.clone(),
                                    0,
                                    false,
                                );
                            }
                        }
                    },
                )
                .size_full(),
            )
            .when(self.loading || self.error.is_some(), |d| {
                d.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .p_4()
                        .text_color(p.ink)
                        .child(
                            self.error
                                .as_ref()
                                .map(|e| format!("Could not display this image: {e}"))
                                .unwrap_or_else(|| "Loading image…".into()),
                        ),
                )
            });
        let info = self
            .picture
            .as_ref()
            .map(|pic| {
                let nav = self.navigation.borrow();
                let zoom = if nav.manual {
                    format!("{:.0}%", nav.view.device_zoom(window.scale_factor()) * 100.)
                } else {
                    "Fit".into()
                };
                format!("{} × {} · {zoom}", pic.width, pic.height)
            })
            .unwrap_or_default();
        div()
            .id("image-viewer")
            .test_support()
            .size_full()
            .flex()
            .flex_col()
            .bg(p.paper)
            .text_color(p.ink)
            .track_focus(&self.focus)
            .key_context("ImageViewer")
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                match e.keystroke.key.as_str() {
                    "left" => this.navigate(false, cx),
                    "right" => this.navigate(true, cx),
                    "+" | "=" => {
                        this.navigation.borrow_mut().step(true);
                        cx.notify();
                    }
                    "-" => {
                        this.navigation.borrow_mut().step(false);
                        cx.notify();
                    }
                    "0" | "f" => {
                        this.navigation.borrow_mut().fit();
                        cx.notify();
                    }
                    "1" => this.actual_size(window, cx),
                    "e" => this.edit(window, cx),
                    "escape" => window.remove_window(),
                    _ => return,
                }
                cx.stop_propagation();
            }))
            .child(
                TitleBar::new().child(
                    div()
                        .text_sm()
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(name),
                ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_1()
                    .p_2()
                    .child(
                        Button::new("viewer-previous")
                            .label("‹")
                            .accessibility_label("Previous image")
                            .tooltip("Previous image · Left")
                            .small()
                            .ghost()
                            .disabled(self.current == 0)
                            .on_click(cx.listener(|this, _, _, cx| this.navigate(false, cx))),
                    )
                    .child(
                        Button::new("viewer-next")
                            .label("›")
                            .accessibility_label("Next image")
                            .tooltip("Next image · Right")
                            .small()
                            .ghost()
                            .disabled(self.current + 1 >= self.files.len())
                            .on_click(cx.listener(|this, _, _, cx| this.navigate(true, cx))),
                    )
                    .child(
                        Button::new("viewer-fit")
                            .label("Fit")
                            .small()
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.navigation.borrow_mut().fit();
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("viewer-actual")
                            .label("1:1")
                            .tooltip("Actual pixels · 1")
                            .small()
                            .ghost()
                            .on_click(cx.listener(|this, _, w, cx| this.actual_size(w, cx))),
                    )
                    .children([(false, "viewer-out", "−"), (true, "viewer-in", "+")].map(
                        |(zoom, id, label)| {
                            Button::new(id)
                                .label(label)
                                .accessibility_label(if zoom { "Zoom in" } else { "Zoom out" })
                                .small()
                                .ghost()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.navigation.borrow_mut().step(zoom);
                                    cx.notify();
                                }))
                        },
                    ))
                    .child(div().flex_1())
                    .child(
                        Button::new("viewer-edit")
                            .label("Edit in Photo")
                            .small()
                            .primary()
                            .disabled(self.files.is_empty())
                            .on_click(cx.listener(|this, _, w, cx| this.edit(w, cx))),
                    ),
            )
            .child(surface)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_1()
                    .text_xs()
                    .text_color(p.muted)
                    .child(format!("{} / {}", self.current + 1, self.files.len()))
                    .child(info)
                    .child("Drag to pan · Scroll to zoom · E to edit")
                    .when(
                        path.extension().is_some_and(|e| {
                            e.eq_ignore_ascii_case("gif") || e.eq_ignore_ascii_case("webp")
                        }),
                        |d| d.child("First frame"),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use emulsion_io::settings::Settings;
    use gpui_kit::component::Root;
    use gpui_kit::test::TestWindowExt;

    #[test]
    fn viewer_routing_excludes_editable_documents_and_external_decoders() {
        for name in [
            "Photo.PNG",
            "two words.JPEG",
            "clip.gif",
            "scan.tiff",
            "image.webp",
        ] {
            assert!(supports(Path::new(name)));
        }
        for name in [
            "project.emu",
            "layers.ora",
            "layers.psd",
            "layers.xcf",
            "chart.drawio",
            "chart.vsdx",
            "page.pdf",
            "raw.dng",
            "drawing.svg",
            "photo.heic",
            "pack.emutemplate",
        ] {
            assert!(!supports(Path::new(name)), "{name}");
        }
    }

    fn open_viewer(
        cx: &mut TestAppContext,
        files: Vec<PathBuf>,
    ) -> (Entity<ImageViewer>, &mut VisualTestContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            theme::install(cx);
            crate::actions::bind(cx);
            cx.set_global(app_state::AppSettings(Settings {
                draw_mode: true,
                ai_hint_dismissed: true,
                ..Default::default()
            }));
            cx.set_global(app_state::Capabilities {
                cli: app_state::CliStatus::Missing,
            });
        });
        let slot = Rc::new(RefCell::new(None));
        let saved = slot.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let viewer = cx.new(|cx| ImageViewer::new(files, window, cx));
            *saved.borrow_mut() = Some(viewer.clone());
            Root::new(viewer, window, cx)
        });
        let viewer = slot.borrow().clone().unwrap();
        cx.run_until_parked();
        (viewer, cx)
    }

    #[gpui_kit::test]
    fn viewer_browses_zooms_and_hands_original_file_to_photo_without_splash(
        cx: &mut TestAppContext,
    ) {
        let folder = tempfile::tempdir().unwrap();
        let first = folder.path().join("a.png");
        let second = folder.path().join("b.png");
        image::RgbaImage::from_pixel(40, 30, image::Rgba([20, 40, 60, 255]))
            .save(&first)
            .unwrap();
        image::RgbaImage::from_pixel(80, 60, image::Rgba([90, 80, 70, 255]))
            .save(&second)
            .unwrap();
        let original = std::fs::read(&second).unwrap();
        let (viewer, cx) = open_viewer(cx, vec![first.clone()]);
        cx.simulate_resize(size(px(760.), px(600.)));
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(viewer.read(cx).workspace.is_none());
            assert_eq!(viewer.read(cx).files, vec![first, second.clone()]);
            assert!(window.try_find("splash").is_none());
            assert!(window.try_find("workspace").is_none());
            assert_eq!(
                cx.global::<app_state::Capabilities>().cli,
                app_state::CliStatus::Missing
            );
            assert!(window.find("viewer-edit").visible());
            window.click("viewer-next", cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert_eq!(viewer.read(cx).picture.as_ref().unwrap().width, 80);
            window.click("viewer-actual", cx);
            assert_eq!(
                viewer
                    .read(cx)
                    .navigation
                    .borrow()
                    .view
                    .device_zoom(window.scale_factor()),
                1.
            );
            window.click("viewer-in", cx);
            assert!(
                viewer
                    .read(cx)
                    .navigation
                    .borrow()
                    .view
                    .device_zoom(window.scale_factor())
                    > 1.
            );
            window.click("viewer-edit", cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            let viewer = viewer.read(cx);
            assert!(viewer.picture.is_none() && viewer.load_task.is_none());
            let ws = viewer.workspace.as_ref().unwrap().read(cx);
            assert!(!ws.splash);
            assert!(window.try_find("splash").is_none());
            assert!(window.try_find("image-viewer").is_none());
            let editor = ws.editor.as_ref().unwrap().read(cx);
            assert!(!editor.draw_mode);
            assert_eq!(editor.source.as_ref(), Some(&second));
            assert_eq!(
                (editor.editor.doc.width, editor.editor.doc.height),
                (80, 60)
            );
        });
        assert_eq!(std::fs::read(second).unwrap(), original);
    }

    #[gpui_kit::test]
    fn rapid_navigation_and_bad_files_cannot_replace_the_selected_image(cx: &mut TestAppContext) {
        let folder = tempfile::tempdir().unwrap();
        let bad = folder.path().join("bad.png");
        let good = folder.path().join("good.png");
        std::fs::write(&bad, b"not an image").unwrap();
        image::RgbaImage::from_pixel(10, 20, image::Rgba([20, 40, 60, 255]))
            .save(&good)
            .unwrap();
        let (viewer, cx) = open_viewer(cx, vec![bad, good]);
        cx.update(|window, cx| {
            assert!(viewer.read(cx).error.is_some());
            assert!(window.find("viewer-edit").visible());
            viewer.update(cx, |v, cx| {
                v.navigate(true, cx);
                v.navigate(false, cx);
                v.navigate(true, cx);
            });
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            let v = viewer.read(cx);
            assert_eq!(v.current, 1);
            assert!(v.error.is_none() && !v.loading);
            assert_eq!(
                (
                    v.picture.as_ref().unwrap().width,
                    v.picture.as_ref().unwrap().height
                ),
                (10, 20)
            );
        });
    }

    #[test]
    fn large_images_keep_full_resolution_tiles_for_actual_size() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("wide.png");
        image::RgbaImage::from_fn(2300, 40, |x, _| {
            image::Rgba([if x % 2 == 0 { 0 } else { 255 }, 0, 0, 255])
        })
        .save(&path)
        .unwrap();
        let picture = decode(&path).unwrap();
        assert_eq!((picture.width, picture.height), (2300, 40));
        assert_eq!(picture.tiles.len(), 2);
        assert_eq!((picture.tiles[1].x, picture.tiles[1].width), (2048, 252));
        assert_eq!(picture.tiles[1].image.size(0).width.0 as u32, 252);
        assert!(picture.preview_scale < 1.);
    }
}
