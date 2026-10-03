//! Dialogs whose body can outgrow the window keep their action buttons in the
//! dialog's footer slot. gpui-kit caps the dialog surface at the viewport and
//! scrolls its body, while the footer stays pinned below the body, so Create /
//! Save / OK remain visible and clickable on small (laptop) windows.

use gpui_kit::{component::dialog::Dialog, *};

/// A dialog view that renders its own action row (Cancel / OK, notices next to
/// the buttons) separately from its scrollable body.
pub(crate) trait DialogActions: Render {
    fn render_actions(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement;
}

/// Puts `view` in the dialog's scrolling body and its action row in the sticky
/// footer. Call it from the `open_dialog` builder, which runs every frame, so
/// the footer tracks the view's state.
pub(crate) fn with_actions<V: DialogActions>(
    dialog: Dialog,
    view: &Entity<V>,
    window: &mut Window,
    cx: &mut App,
) -> Dialog {
    let actions = view.update(cx, |view, cx| view.render_actions(window, cx));
    dialog
        .child(view.clone())
        .footer(div().w_full().child(actions))
}
