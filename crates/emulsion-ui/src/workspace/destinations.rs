//! Workspace destinations select an existing compatible tab or start a new one.
use super::*;
use emulsion_core::{creation::CanvasKind, project::ProjectKind};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Destination {
    Home,
    Photo,
    Paint,
    Library,
    Design,
    Diagram,
    Storyboard,
}
impl Destination {
    pub(crate) const ALL: [Self; 7] = [
        Self::Home,
        Self::Photo,
        Self::Paint,
        Self::Library,
        Self::Design,
        Self::Diagram,
        Self::Storyboard,
    ];
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Photo => "Photo",
            Self::Paint => "Paint",
            Self::Library => "Library",
            Self::Design => "Design",
            Self::Diagram => "Diagram",
            Self::Storyboard => "Storyboard",
        }
    }
    pub(crate) fn file_new_label(self) -> &'static str {
        match self {
            Self::Photo => "New photo document…",
            Self::Paint => "New painting…",
            Self::Design => "New design…",
            Self::Diagram => "New diagram…",
            Self::Storyboard => "New storyboard…",
            _ => "New document…",
        }
    }
    pub(crate) fn file_open_label(self) -> &'static str {
        match self {
            Self::Photo => "Open images…",
            Self::Paint => "Open artwork…",
            Self::Design => "Open design or presentation…",
            Self::Diagram => "Open diagram…",
            Self::Storyboard => "Open storyboard…",
            Self::Library => "Import photo folder…",
            Self::Home => "Open…",
        }
    }
    pub(crate) fn file_open_prompt(self) -> &'static str {
        match self {
            Self::Photo => "Open images — camera RAW, JPEG, PNG, TIFF, PSD, XCF or OpenRaster",
            Self::Paint => "Open artwork — OpenRaster, PSD, XCF or images",
            Self::Design => "Open an Emulsion design, PowerPoint presentation or Lottie animation",
            Self::Diagram => {
                "Open diagrams — Emulsion, Mermaid, D2, Graphviz, Markdown, Visio, draw.io or Lucid"
            }
            Self::Storyboard => "Open an Emulsion storyboard (.emu)",
            _ => "Open",
        }
    }
    pub(crate) fn canvas(self) -> Option<CanvasKind> {
        match self {
            Self::Photo => Some(CanvasKind::Photo),
            Self::Paint => Some(CanvasKind::Paint),
            Self::Design => Some(CanvasKind::Design),
            Self::Diagram => Some(CanvasKind::Diagram),
            Self::Storyboard => Some(CanvasKind::Storyboard),
            _ => None,
        }
    }
    pub(crate) fn subtitle(self) -> &'static str {
        match self {
            Self::Home => "Recent work",
            Self::Photo => "Retouch, composite, RAW",
            Self::Paint => "Blank canvas, brushes",
            Self::Library => "Import and batch edit",
            Self::Design => "Social, print, decks",
            Self::Diagram => "Flowcharts, architecture",
            Self::Storyboard => "Panels, scenes, animatics",
        }
    }
    pub(crate) fn for_editor(editor: &EditorView) -> Self {
        match editor.editor.kind() {
            Some(ProjectKind::Design) => Self::Design,
            Some(ProjectKind::Diagram) => Self::Diagram,
            Some(ProjectKind::Storyboard) => Self::Storyboard,
            None if editor.draw_mode => Self::Paint,
            None => Self::Photo,
        }
    }
}
impl Workspace {
    /// Shared Home/Library navigation; compact rails retain named keyboard targets.
    pub(crate) fn destination_navigation(
        &self,
        prefix: &'static str,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let active = self.destination(cx);
        div()
            .id((ElementId::from(prefix), "navigation"))
            .test_support()
            .flex()
            .flex_col()
            .flex_none()
            .gap_1()
            .p_2()
            .children(Destination::ALL.map(|destination| {
                let glyph = match destination {
                    Destination::Home => "house",
                    Destination::Photo => "image",
                    Destination::Paint => "paintbrush",
                    Destination::Library => "folder",
                    Destination::Design => "layout-template",
                    Destination::Diagram => "workflow",
                    Destination::Storyboard => "clapperboard",
                };
                let button = Button::new((ElementId::from(prefix), destination.label()))
                    .icon(gpui_kit::component::Icon::empty().path(format!("icons/{glyph}.svg")))
                    .accessibility_label(destination.label())
                    .tooltip(destination.subtitle())
                    .small()
                    .ghost()
                    .w_full()
                    .selected(active == Some(destination));
                button
                    .when(!compact, |b| b.label(destination.label()).justify_start())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.visit_destination(destination, window, cx)
                    }))
            }))
            .into_any_element()
    }
    pub(crate) fn destination(&self, cx: &App) -> Option<Destination> {
        match self.screen {
            Screen::Home => Some(Destination::Home),
            Screen::Batch => Some(Destination::Library),
            Screen::Editor => self
                .editor
                .as_ref()
                .map(|e| Destination::for_editor(e.read(cx))),
            _ => None,
        }
    }
    pub(crate) fn start_destination(
        &mut self,
        destination: Destination,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if destination == Destination::Photo {
            self.prompt_open_named("Open photo", true, window, cx);
        } else if let Some(kind) = destination.canvas() {
            self.open_new_canvas_kind(kind, window, cx);
        } else {
            self.visit_destination(destination, window, cx);
        }
    }
    pub(crate) fn visit_destination(
        &mut self,
        destination: Destination,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match destination {
            Destination::Home => self.show_home(window, cx),
            Destination::Library => {
                self.cancel_style_dialog(window, cx);
                self.set_screen(Screen::Batch, window, cx);
                self.refresh_batch_recipes(cx);
            }
            _ => {
                if let Some(index) = self
                    .tabs
                    .iter()
                    .rposition(|tab| Destination::for_editor(tab.read(cx)) == destination)
                {
                    self.activate_tab(index, window, cx);
                } else if destination == Destination::Photo {
                    self.prompt_open_named("Open photo", true, window, cx);
                } else if let Some(kind) = destination.canvas() {
                    self.open_new_canvas_kind(kind, window, cx);
                }
            }
        }
    }
    pub(super) fn workspace_switcher(&self, wide: bool, cx: &mut Context<Self>) -> AnyElement {
        let active = self.destination(cx);
        let p = theme::palette(cx);
        if wide {
            div()
                .id("workspace-switcher")
                .test_support()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(1.))
                .p(px(2.))
                .rounded(px(7.))
                .bg(p.soft_bg)
                .border_1()
                .border_color(p.line)
                .children(Destination::ALL.map(|destination| {
                    Button::new((
                        ElementId::from("workspace-destination"),
                        destination.label(),
                    ))
                    .label(destination.label())
                    .xsmall()
                    .ghost()
                    .selected(active == Some(destination))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.visit_destination(destination, window, cx)
                    }))
                }))
                .into_any_element()
        } else {
            let owner = cx.weak_entity();
            Button::new("workspace-switcher-menu")
                .label(format!(
                    "{} ▾",
                    active.map_or("Workspace", Destination::label)
                ))
                .xsmall()
                .ghost()
                .dropdown_menu(move |mut menu, _, _| {
                    for destination in Destination::ALL {
                        let owner = owner.clone();
                        menu = menu.item(
                            PopupMenuItem::new(destination.label())
                                .checked(active == Some(destination))
                                .on_click(move |_, window, cx| {
                                    owner
                                        .update(cx, |this, cx| {
                                            this.visit_destination(destination, window, cx)
                                        })
                                        .ok();
                                }),
                        );
                    }
                    menu
                })
                .into_any_element()
        }
    }
}
