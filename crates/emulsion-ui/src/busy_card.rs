//! The progress card shown while a slow task runs: a spinner, what is
//! happening, a bar (filling when the task reports progress, sweeping when it
//! cannot), and the elapsed time.
use crate::theme::{MONO_FONT, Palette};
use gpui_kit::component::Sizable;
use gpui_kit::component::progress::Progress;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use std::time::{Duration, Instant};

/// How long a task runs before its card appears, so quick ones never flash.
pub(crate) const SHOW_AFTER: Duration = Duration::from_millis(300);

/// A task slow enough to show progress for.
#[derive(Clone, Debug)]
pub(crate) struct Busy {
    /// What is happening, e.g. "Opening IMG_0042.CR3".
    pub title: SharedString,
    /// A quieter second line, e.g. "Camera raw · 24.3 MB".
    pub detail: Option<SharedString>,
    pub started: Instant,
}

impl Busy {
    pub(crate) fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            detail: None,
            started: Instant::now(),
        }
    }

    pub(crate) fn detail(mut self, detail: impl Into<SharedString>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

/// "0:07", "1:23".
fn elapsed_label(elapsed: Duration) -> String {
    let s = elapsed.as_secs();
    format!("{}:{:02}", s / 60, s % 60)
}

/// "24.3 MB", "812 KB".
pub(crate) fn file_size_label(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    if bytes as f64 >= MB {
        format!("{:.1} MB", bytes as f64 / MB)
    } else {
        format!("{} KB", bytes.div_ceil(1024))
    }
}

/// The card. `fraction` is `Some` when the task reports how far it is.
/// Callers add their own Cancel control with `.child(..)` if the task can
/// stop.
pub(crate) fn busy_card(
    id: impl Into<ElementId>,
    busy: &Busy,
    fraction: Option<f32>,
    p: &Palette,
) -> impl IntoElement + ParentElement {
    let id = id.into();
    let elapsed = busy.started.elapsed();
    let status = match fraction {
        Some(f) => format!("{:.0} %", (f * 100.).clamp(0., 100.)),
        None => "Working…".into(),
    };
    div()
        .id(id.clone())
        .test_support()
        .flex()
        .flex_col()
        .gap(px(10.))
        .w(px(360.))
        .max_w_full()
        .p(px(16.))
        .rounded_lg()
        .border_1()
        .border_color(p.line)
        .bg(p.panel)
        .shadow_lg()
        .occlude()
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(Spinner::new().large().color(p.accent))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .gap(px(2.))
                        .child(
                            div()
                                .text_size(px(13.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(p.ink)
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(busy.title.clone()),
                        )
                        .when_some(busy.detail.clone(), |d, detail| {
                            d.child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(p.muted)
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(detail),
                            )
                        }),
                ),
        )
        .child(
            Progress::new((id.clone(), "bar"))
                .small()
                .color(p.accent)
                .loading(fraction.is_none())
                .value(fraction.unwrap_or(0.) * 100.)
                .accessibility_label(busy.title.clone()),
        )
        .child(
            div()
                .flex()
                .justify_between()
                .font_family(MONO_FONT)
                .text_size(px(10.))
                .text_color(p.muted)
                .child(status)
                // Only worth showing once the wait is noticeable.
                .when(elapsed >= Duration::from_secs(1), |d| {
                    d.child(elapsed_label(elapsed))
                }),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn labels_read_naturally() {
        assert_eq!(elapsed_label(Duration::from_secs(7)), "0:07");
        assert_eq!(elapsed_label(Duration::from_secs(83)), "1:23");
        assert_eq!(file_size_label(812 * 1024), "812 KB");
        assert_eq!(file_size_label(25_480_000), "24.3 MB");
    }
}
