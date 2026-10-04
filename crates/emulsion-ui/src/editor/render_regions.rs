//! Retained render boundaries for the canvas and its sibling panels.
use super::*;

pub(crate) struct CanvasView {
    owner: WeakEntity<EditorView>,
    #[cfg(any(test, feature = "layout-bench"))]
    pub(crate) render_count: usize,
}

impl CanvasView {
    pub(super) fn new(owner: WeakEntity<EditorView>) -> Self {
        Self {
            owner,
            #[cfg(any(test, feature = "layout-bench"))]
            render_count: 0,
        }
    }
}

impl Render for CanvasView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(any(test, feature = "layout-bench"))]
        {
            self.render_count += 1;
        }
        self.owner
            .update(cx, |owner, cx| {
                let palette = owner.workspace_palette(cx);
                owner.canvas_area(&palette, window, cx).into_any_element()
            })
            .unwrap_or_else(|_| div().into_any_element())
    }
}

/// The Design library is independent of the canvas transform, just like Layers.
/// Keep its template cards and layout resident during pointer/navigation frames.
pub(crate) struct DesignLibraryView {
    owner: WeakEntity<EditorView>,
    _owner_subscription: Subscription,
    #[cfg(test)]
    pub(crate) render_count: usize,
}

impl DesignLibraryView {
    pub(super) fn new(owner: WeakEntity<EditorView>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&owner.upgrade().expect("library owner"), |_, _, cx| {
            cx.notify()
        });
        Self {
            owner,
            _owner_subscription: subscription,
            #[cfg(test)]
            render_count: 0,
        }
    }
}

impl Render for DesignLibraryView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        {
            self.render_count += 1;
        }
        let content = self
            .owner
            .update(cx, |owner, cx| {
                owner.design_drawer(&theme::palette(cx), window, cx)
            })
            .ok()
            .flatten();
        div().flex().size_full().min_h_0().children(content)
    }
}

pub(crate) struct SidebarView {
    owner: WeakEntity<EditorView>,
    _owner_subscription: Subscription,
    #[cfg(any(test, feature = "layout-bench"))]
    pub(crate) render_count: usize,
}

impl SidebarView {
    pub(super) fn new(owner: WeakEntity<EditorView>, cx: &mut Context<Self>) -> Self {
        // The editor owns document and panel state. Canvas-only updates notify
        // CanvasView instead, leaving this sibling's cached subtree reusable.
        let owner_entity = owner.upgrade().expect("sidebar owner must be alive");
        let subscription = cx.observe(&owner_entity, |_, _, cx| cx.notify());
        Self {
            owner,
            _owner_subscription: subscription,
            #[cfg(any(test, feature = "layout-bench"))]
            render_count: 0,
        }
    }
}

impl Render for SidebarView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(any(test, feature = "layout-bench"))]
        {
            self.render_count += 1;
        }
        let panel = self.owner.update(cx, |owner, cx| {
            let palette = owner.workspace_palette(cx);
            owner
                .render_sidebar(&palette, window, cx)
                .into_any_element()
        });
        // Cached views lay out their contents as a separate root. Give that
        // root the allocated size so the panel retains its stretched height.
        div().flex().size_full().min_h_0().children(panel.ok())
    }
}

impl EditorView {
    pub(super) fn design_library_region(
        &mut self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        // Benchmark the previous boundary in the same release executable.
        // This switch does not exist in shipping builds.
        #[cfg(feature = "canvas-bench")]
        if std::env::var_os("EMULSION_BENCH_UNCACHED_DESIGN_LIBRARY").is_some() {
            return self.design_drawer(p, window, cx);
        }
        let _ = (p, cx);
        self.is_design().then(|| {
            self.design_library_view
                .clone()
                .cached(
                    StyleRefinement::default()
                        .flex_none()
                        .w(self.design_library_width(window))
                        .h_full()
                        .min_h_0(),
                )
                .into_any_element()
        })
    }

    pub(super) fn sidebar_content_visible(&self, window: &Window, _cx: &App) -> bool {
        !self.sidebar_layout.collapsed
            && (self.sidebar_layout.overlay_open
                || self
                    .sidebar_layout
                    .width_for_viewport(
                        f32::from(window.viewport_size().width),
                        f32::from(window.rem_size()),
                    )
                    .is_some())
    }

    /// Navigation changes the canvas and the view-dependent sidebar panels.
    /// Uncached ancestors still rebuild the zoom and rotation controls.
    pub(super) fn notify_canvas_navigation(&self, window: &Window, cx: &mut Context<Self>) {
        self.notify_canvas(cx);
        let view_dependent_panel = match self.sidebar_tab {
            SidebarTab::Info => self.panels.info,
            SidebarTab::Navigator => self.panels.navigator,
            SidebarTab::Properties => self.shared_panel_mode(),
            _ => false,
        };
        if view_dependent_panel && self.sidebar_content_visible(window, cx) {
            self.notify_sidebar(cx);
        }
    }

    pub(super) fn canvas_region(&self) -> impl IntoElement + use<> {
        // Keep ancestors of the cached sidebar uncached: refreshing a cached
        // ancestor also refreshes all of its cached descendants in GPUI.
        self.canvas_view.clone()
    }

    pub(super) fn sidebar_region(&self, window: &Window, _cx: &App) -> impl IntoElement + use<> {
        let rem_size = f32::from(window.rem_size());
        let width = self
            .sidebar_layout
            .width_for_viewport(f32::from(window.viewport_size().width), rem_size)
            .map(px)
            .unwrap_or_else(|| px(1.875 * rem_size));
        // The cache's size must be explicit; it cannot measure the panel's
        // contents. Match the expanded/collapsed sidebar geometry exactly.
        self.sidebar_view.clone().cached(
            StyleRefinement::default()
                .flex_none()
                .w(width)
                .h_full()
                .min_h_0(),
        )
    }
}
