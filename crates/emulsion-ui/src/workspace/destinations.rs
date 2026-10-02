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
}
impl Destination {
    pub(crate) const ALL: [Self; 6] = [
        Self::Home,
        Self::Photo,
        Self::Paint,
        Self::Library,
        Self::Design,
        Self::Diagram,
    ];
    /// Stable English name, also used in element ids; show `name()` instead.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Photo => "Photo",
            Self::Paint => "Paint",
            Self::Library => "Library",
            Self::Design => "Design",
            Self::Diagram => "Diagram",
        }
    }
    /// The destination's name in the interface language.
    pub(crate) fn name(self) -> SharedString {
        match self {
            Self::Home => t!("window.home"),
            Self::Photo => t!("shell.dest_photo"),
            Self::Paint => t!("shell.dest_paint"),
            Self::Library => t!("home.library"),
            Self::Design => t!("shell.dest_design"),
            Self::Diagram => t!("shell.dest_diagram"),
        }
        .into()
    }
    pub(crate) fn file_new_label(self) -> SharedString {
        match self {
            Self::Photo => t!("file.new_photo"),
            Self::Paint => t!("file.new_painting"),
            Self::Design => t!("file.new_design"),
            Self::Diagram => t!("file.new_diagram"),
            _ => t!("file.new_document"),
        }
        .into()
    }
    pub(crate) fn file_open_label(self) -> SharedString {
        match self {
            Self::Photo => t!("file.open_images"),
            Self::Paint => t!("file.open_artwork"),
            Self::Design => t!("file.open_design"),
            Self::Diagram => t!("file.open_diagram"),
            Self::Library => t!("file.import_photo_folder"),
            Self::Home => t!("file.open"),
        }
        .into()
    }
    pub(crate) fn file_open_prompt(self) -> SharedString {
        match self {
            Self::Photo => t!("shell.open_prompt_photo"),
            Self::Paint => t!("shell.open_prompt_paint"),
            Self::Design => t!("shell.open_prompt_design"),
            Self::Diagram => t!("shell.open_prompt_diagram"),
            _ => t!("shell.open_prompt"),
        }
        .into()
    }
    pub(crate) fn canvas(self) -> Option<CanvasKind> {
        match self {
            Self::Photo => Some(CanvasKind::Photo),
            Self::Paint => Some(CanvasKind::Paint),
            Self::Design => Some(CanvasKind::Design),
            Self::Diagram => Some(CanvasKind::Diagram),
            _ => None,
        }
    }
    pub(crate) fn subtitle(self) -> SharedString {
        match self {
            Self::Home => t!("shell.dest_home_sub"),
            Self::Photo => t!("shell.dest_photo_sub"),
            Self::Paint => t!("shell.dest_paint_sub"),
            Self::Library => t!("shell.dest_library_sub"),
            Self::Design => t!("shell.dest_design_sub"),
            Self::Diagram => t!("shell.dest_diagram_sub"),
        }
        .into()
    }
    pub(crate) fn for_editor(editor: &EditorView) -> Self {
        match editor.editor.kind() {
            Some(ProjectKind::Design) => Self::Design,
            Some(ProjectKind::Diagram) => Self::Diagram,
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
                };
                let button = Button::new((ElementId::from(prefix), destination.label()))
                    .icon(gpui_kit::component::Icon::empty().path(format!("icons/{glyph}.svg")))
                    .accessibility_label(destination.name())
                    .tooltip(destination.subtitle())
                    .small()
                    .ghost()
                    .w_full()
                    .selected(active == Some(destination));
                button
                    .when(!compact, |b| b.label(destination.name()).justify_start())
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
            self.prompt_open_named(t!("shell.open_photo"), true, window, cx);
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
                    self.prompt_open_named(t!("shell.open_photo"), true, window, cx);
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
                    .label(destination.name())
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
                    active.map_or_else(|| t!("shell.workspace").into(), Destination::name)
                ))
                .xsmall()
                .ghost()
                .dropdown_menu(move |mut menu, _, _| {
                    for destination in Destination::ALL {
                        let owner = owner.clone();
                        menu = menu.item(
                            PopupMenuItem::new(destination.name())
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
