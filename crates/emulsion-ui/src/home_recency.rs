//! A small recent shelf, with older work available on demand.
use super::*;
use std::ops::Range;

const PAGE_SIZE: usize = 12;
const DAY: u64 = 86_400;

// The input is already sorted by last opened, newest first. Half-open age
// intervals put each file in exactly one group, including boundary timestamps.
fn age_ranges(entries: &[recent::Recent], now: u64) -> [Range<usize>; 3] {
    let fortnight = entries.partition_point(|entry| now.saturating_sub(entry.opened) < 14 * DAY);
    let month = entries.partition_point(|entry| now.saturating_sub(entry.opened) < 30 * DAY);
    [0..fortnight, fortnight..month, month..entries.len()]
}

impl Workspace {
    pub(super) fn home_groups_recents(&self, cx: &App) -> bool {
        !self.home_state.cloud_files
            && !self.home_state.sort_name
            && self.home_state.filter == HomeFilter::All
            && !self.home_state.projects.trash
            && self.home_state.projects.folder.is_none()
            && self.home_state.projects.kind.is_none()
            && self.home_state.folder.is_none()
            && !self.home_state.unfiled
            && self.home_search_query(cx).trim().is_empty()
    }

    pub(super) fn home_recent_groups(
        &mut self,
        entries: &[recent::Recent],
        columns: u16,
        wide: bool,
        thumbnail_width: u32,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ranges = age_ranges(entries, recent::now());
        let mut sections = Vec::new();
        let mut previews = Vec::new();
        for (group, range) in ranges.into_iter().enumerate() {
            let total = range.len();
            if total == 0 {
                continue;
            }
            let open = group == 0 || self.home_state.recent_expanded[group - 1];
            let page = self.home_state.recent_pages[group].min((total - 1) / PAGE_SIZE);
            self.home_state.recent_pages[group] = page;
            let start = range.start + page * PAGE_SIZE;
            let shown = &entries[start..(start + PAGE_SIZE).min(range.end)];
            let label = ["Last 2 weeks", "2 weeks to 1 month", "Older than 1 month"][group];
            let mut section = div().flex().flex_col().flex_none().gap(px(12.));
            if group == 0 {
                section = section.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_sm()
                        .text_color(p.muted)
                        .child(label)
                        .child(format!("· {total} files")),
                );
            } else {
                section = section.child(
                    Button::new(("home-age-toggle", group))
                        .label(format!("{label} · {total} files"))
                        .accessibility_label(format!(
                            "{} {label}, {total} files",
                            if open { "Collapse" } else { "Expand" }
                        ))
                        .icon(if open {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .ghost()
                        .w_full()
                        .justify_start()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.home_state.recent_expanded[group - 1] =
                                !this.home_state.recent_expanded[group - 1];
                            cx.notify();
                        })),
                );
            }
            if open {
                previews.extend_from_slice(shown);
                let cells = shown
                    .iter()
                    .map(|entry| {
                        self.home_recent(entry, self.home_state.selected.as_deref(), p, wide, cx)
                    })
                    .collect::<Vec<_>>();
                let id = if self.home_state.rows {
                    "home-recent-rows"
                } else {
                    "home-recent-grid"
                };
                let id = if group == 0 {
                    ElementId::from(id)
                } else {
                    (id, group).into()
                };
                let gallery = div().id(id).test_support().flex_none().min_w_0();
                let gallery = if self.home_state.rows {
                    gallery
                        .flex()
                        .flex_col()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(p.line)
                        .overflow_hidden()
                        .when(group == 0, |list| {
                            list.child(self.home_list_heading(wide, p))
                        })
                } else {
                    gallery.grid().grid_cols(columns).gap(px(12.))
                };
                section = section.child(gallery.children(cells));
                if total > PAGE_SIZE {
                    section = section.child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap_2()
                            .text_xs()
                            .text_color(p.muted)
                            .child(
                                Button::new(("home-age-prev", group))
                                    .label("Previous")
                                    .small()
                                    .ghost()
                                    .disabled(page == 0)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.home_state.recent_pages[group] =
                                            this.home_state.recent_pages[group].saturating_sub(1);
                                        cx.notify();
                                    })),
                            )
                            .child(format!(
                                "{}–{} of {total}",
                                page * PAGE_SIZE + 1,
                                ((page + 1) * PAGE_SIZE).min(total)
                            ))
                            .child(
                                Button::new(("home-age-next", group))
                                    .label("Next 12")
                                    .small()
                                    .ghost()
                                    .disabled((page + 1) * PAGE_SIZE >= total)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.home_state.recent_pages[group] += 1;
                                        cx.notify();
                                    })),
                            ),
                    );
                }
            }
            sections.push(section);
        }
        // Collapsed groups don't create cards or request thumbnails. Even with
        // all groups open, only 36 previews are eligible for background work.
        self.load_thumbs(&previews, thumbnail_width, cx);
        div()
            .id("home-recent-groups")
            .test_support()
            .flex()
            .flex_col()
            .flex_none()
            .gap(px(20.))
            .children(sections)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn age_groups_cover_boundaries_future_dates_and_empty_history() {
        let now = 60 * DAY;
        let ages = [
            0,
            1,
            14 * DAY - 1,
            14 * DAY,
            30 * DAY - 1,
            30 * DAY,
            60 * DAY,
        ];
        let entries = ages.map(|age| recent::Recent {
            path: format!("{age}.png").into(),
            opened: now - age,
            summary: String::new(),
        });
        assert_eq!(age_ranges(&entries, now), [0..3, 3..5, 5..7]);
        assert_eq!(age_ranges(&entries[..1], 0), [0..1, 1..1, 1..1]);
        assert_eq!(age_ranges(&[], now), [0..0, 0..0, 0..0]);
    }
}
