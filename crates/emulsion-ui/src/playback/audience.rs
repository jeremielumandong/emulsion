//! The audience window: a borderless full-screen window on a chosen display
//! that shows only the picture playback produces, letterboxed on black.
//! Its owner (any view) supplies the picture and handles keys; the window
//! redraws whenever the owner changes and closes itself when the owner
//! stops supplying pictures. Escape always closes it.
use gpui_kit::*;
use std::sync::Arc;

/// A display to play on, as offered to the person.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Screen {
    pub id: DisplayId,
    pub label: String,
}

/// Every connected display, the primary one first.
pub(crate) fn screens(cx: &App) -> Vec<Screen> {
    let primary = cx.primary_display().map(|d| d.id());
    let mut displays = cx.displays();
    displays.sort_by_key(|d| Some(d.id()) != primary);
    displays
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let size = d.bounds().size;
            let what = if Some(d.id()) == primary {
                "main display"
            } else {
                "display"
            };
            Screen {
                id: d.id(),
                label: format!(
                    "{} ({what}, {}×{})",
                    i + 1,
                    f32::from(size.width).round(),
                    f32::from(size.height).round()
                ),
            }
        })
        .collect()
}

/// Reads the picture from the owner; `None` closes the window.
type Picture<T> = fn(&T, &App) -> Option<Option<Arc<RenderImage>>>;
/// Handles a key; Escape is handled before this is asked.
type Keys<T> = fn(&mut T, &KeyDownEvent, &mut Context<T>);
/// Told when the window closes, for whatever reason.
type Closed<T> = fn(&mut T, &mut Context<T>);

pub(crate) struct Audience<T: 'static> {
    owner: WeakEntity<T>,
    picture: Picture<T>,
    keys: Keys<T>,
    closed: Closed<T>,
    focus: FocusHandle,
    _watch: Subscription,
}

/// Open the audience window on `display` (the main display when `None`).
pub(crate) fn open<T: 'static>(
    owner: &Entity<T>,
    display: Option<DisplayId>,
    picture: Picture<T>,
    keys: Keys<T>,
    closed: Closed<T>,
    cx: &mut App,
) -> anyhow::Result<AnyWindowHandle> {
    let target = display
        .and_then(|id| cx.find_display(id))
        .or_else(|| cx.primary_display());
    let bounds = target.as_ref().map_or_else(
        || Bounds::centered(None, size(px(1280.), px(720.)), cx),
        |d| d.bounds(),
    );
    let weak = owner.downgrade();
    let handle = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Fullscreen(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("Emulsion — Player".into()),
                ..Default::default()
            }),
            focus: true,
            display_id: target.map(|d| d.id()),
            ..Default::default()
        },
        move |window, cx| {
            let closing = weak.clone();
            window.on_window_should_close(cx, move |_, cx| {
                closing.update(cx, closed).ok();
                true
            });
            cx.new(|cx| {
                let watch = match weak.upgrade() {
                    Some(owner) => cx.observe(&owner, |_, _, cx| cx.notify()),
                    None => Subscription::new(|| {}),
                };
                let focus = cx.focus_handle();
                window.focus(&focus, cx);
                Audience {
                    owner: weak,
                    picture,
                    keys,
                    closed,
                    focus,
                    _watch: watch,
                }
            })
        },
    )?;
    Ok(handle.into())
}

impl<T: 'static> Render for Audience<T> {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let picture = self
            .owner
            .upgrade()
            .and_then(|owner| (self.picture)(owner.read(cx), cx));
        let Some(picture) = picture else {
            window.remove_window();
            return div().into_any_element();
        };
        div()
            .id("playback-audience")
            .test_support()
            .track_focus(&self.focus)
            .size_full()
            .bg(rgb(0x000000))
            .flex()
            .items_center()
            .justify_center()
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                cx.stop_propagation();
                let Some(owner) = this.owner.upgrade() else {
                    return;
                };
                if event.keystroke.key == "escape" {
                    owner.update(cx, |owner, cx| (this.closed)(owner, cx));
                    window.remove_window();
                } else {
                    owner.update(cx, |owner, cx| (this.keys)(owner, event, cx));
                }
            }))
            .children(picture.map(|image| img(image).size_full().object_fit(ObjectFit::Contain)))
            .into_any_element()
    }
}
