//! About: what Emulsion is, its licence, and the attribution it owes —
//! the vendored GPUI family with its Apache-2.0 notices, every crate in
//! the build with its licence, the optional AI models, and the data it
//! downloads on request. All texts are embedded at build time from the
//! same files the release packages ship.

use crate::theme::{self, MONO_FONT, Palette};
use crate::widgets::{chip, mono};
use crate::workspace::Workspace;
use gpui_kit::*;
use std::sync::OnceLock;

const LICENSE: &str = include_str!("../../../LICENSE");
const NOTICES: &str = include_str!("../../../THIRD_PARTY_NOTICES.md");
const FONT_LICENSES: &[&str] = &[
    include_str!("../../../assets/fonts/Geist-OFL.txt"),
    include_str!("../../../assets/fonts/GeistMono-OFL.txt"),
    include_str!("../../../assets/fonts/CormorantGaramond-OFL.txt"),
    include_str!("../../../assets/fonts/Fraunces-OFL.txt"),
];
const GPUI_LICENSING: &str = include_str!("../../../vendor/gpui/LICENSING.md");
const GPUI_UPSTREAM: &str = include_str!("../../../vendor/gpui/UPSTREAM.json");
const CRATES: &str = include_str!("../../../THIRD_PARTY_CRATES.md");
const GPUI_CHANGES: [(&str, &str); 3] = [
    (
        "gpui-pre",
        include_str!("../../../vendor/gpui/gpui-pre/EMULSION_CHANGES.md"),
    ),
    (
        "gpui-pre-wgpu",
        include_str!("../../../vendor/gpui/gpui-pre-wgpu/EMULSION_CHANGES.md"),
    ),
    (
        "gpui-pre-windows",
        include_str!("../../../vendor/gpui/gpui-pre-windows/EMULSION_CHANGES.md"),
    ),
];

/// One vendored GPUI package, from UPSTREAM.json.
struct Vendored {
    name: String,
    version: String,
    license: String,
    repository: String,
}

fn vendored() -> &'static [Vendored] {
    static V: OnceLock<Vec<Vendored>> = OnceLock::new();
    V.get_or_init(|| {
        let v: serde_json::Value = serde_json::from_str(GPUI_UPSTREAM).unwrap_or_default();
        v.as_array()
            .map(|a| {
                a.iter()
                    .map(|p| Vendored {
                        name: p["name"].as_str().unwrap_or("").to_string(),
                        version: p["version"].as_str().unwrap_or("").to_string(),
                        license: p["license"].as_str().unwrap_or("").to_string(),
                        repository: p["repository"].as_str().unwrap_or("").to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    })
}

/// (crate, version, licence) rows of the generated inventory.
fn crates() -> &'static [(String, String, String)] {
    static C: OnceLock<Vec<(String, String, String)>> = OnceLock::new();
    C.get_or_init(|| {
        CRATES
            .lines()
            .filter(|l| l.starts_with("| ") && !l.starts_with("| Crate") && !l.starts_with("|---"))
            .filter_map(|l| {
                let cells: Vec<&str> = l.trim_matches('|').split('|').map(str::trim).collect();
                (cells.len() >= 3).then(|| {
                    (
                        cells[0].to_string(),
                        cells[1].to_string(),
                        cells[2].to_string(),
                    )
                })
            })
            .collect()
    })
}

/// Downloaded-on-request content that is not a crate, and bundled artwork.
/// Name and note are catalog keys, so they follow the interface language.
const DATA_SOURCES: [(&str, &str, &str); 3] = [
    (
        "about.data_picture",
        "Emulsion (MIT)",
        "about.data_picture_note",
    ),
    (
        "about.data_lensfun",
        "CC BY-SA 3.0",
        "about.data_lensfun_note",
    ),
    (
        "about.data_recipes",
        "Emulsion (MIT)",
        "about.data_recipes_note",
    ),
];

impl Workspace {
    pub(crate) fn about_screen(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let p = theme::palette(cx);
        let heading = |t: &str, p: &Palette| {
            div()
                .font_family(MONO_FONT)
                .text_size(px(11.))
                .text_color(p.muted)
                .child(t.to_uppercase())
        };
        let body = |t: &str, p: &Palette| {
            div()
                .max_w(px(720.))
                .text_size(px(13.))
                .text_color(p.ink)
                .child(t.to_string())
        };
        let pre = |t: &str, p: &Palette| {
            div()
                .max_w(px(760.))
                .p(px(12.))
                .border_1()
                .border_color(p.line)
                .bg(p.soft_bg)
                .font_family(MONO_FONT)
                .text_size(px(10.5))
                .text_color(p.ink)
                .whitespace_normal()
                .child(t.to_string())
        };
        let section = |p: &Palette| {
            div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .px(px(40.))
                .py(px(20.))
                .border_b_1()
                .border_color(p.line)
        };

        // Licence mix of the whole build, for a one-line summary.
        let all = crates();
        let mut counts: std::collections::BTreeMap<&str, usize> = Default::default();
        for (_, _, l) in all {
            *counts.entry(l.as_str()).or_default() += 1;
        }
        let mut mix: Vec<(&str, usize)> = counts.into_iter().collect();
        mix.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        let mix_line = mix
            .iter()
            .take(5)
            .map(|(l, n)| format!("{n} {l}"))
            .collect::<Vec<_>>()
            .join(" · ");
        let copyleft: Vec<&(String, String, String)> = all
            .iter()
            .filter(|(_, _, l)| l.contains("GPL") || l.contains("MPL"))
            .collect();

        let mut gpui_rows = div().flex().flex_col().gap(px(3.));
        for v in vendored() {
            let modified = GPUI_CHANGES.iter().any(|(n, _)| *n == v.name);
            gpui_rows = gpui_rows.child(
                div()
                    .flex()
                    .gap(px(12.))
                    .font_family(MONO_FONT)
                    .text_size(px(10.5))
                    .child(div().w(px(220.)).text_color(p.ink).child(v.name.clone()))
                    .child(
                        div()
                            .w(px(60.))
                            .text_color(p.muted)
                            .child(v.version.clone()),
                    )
                    .child(
                        div()
                            .w(px(140.))
                            .text_color(p.muted)
                            .child(v.license.clone()),
                    )
                    .child(div().text_color(p.muted).child(if modified {
                        t!("about.modified", repository = v.repository).into_owned()
                    } else {
                        v.repository.clone()
                    })),
            );
        }

        let show_all = self.about_all_crates;
        let mut crate_rows = div().flex().flex_col().gap(px(2.));
        if show_all {
            for (n, ver, l) in all {
                crate_rows = crate_rows.child(
                    div()
                        .flex()
                        .gap(px(12.))
                        .font_family(MONO_FONT)
                        .text_size(px(10.))
                        .child(div().w(px(260.)).text_color(p.ink).child(n.clone()))
                        .child(div().w(px(90.)).text_color(p.muted).child(ver.clone()))
                        .child(div().text_color(p.muted).child(l.clone())),
                );
            }
        }

        let mut models = div().flex().flex_col().gap(px(3.));
        for m in emulsion_ai::models::MANIFEST {
            models = models.child(
                div()
                    .flex()
                    .gap(px(12.))
                    .font_family(MONO_FONT)
                    .text_size(px(10.5))
                    .child(div().w(px(220.)).text_color(p.ink).child(m.name))
                    .child(div().w(px(260.)).text_color(p.muted).child(m.license))
                    .child(div().text_color(p.muted).child(m.note)),
            );
        }
        let mut data = div().flex().flex_col().gap(px(3.));
        for (name, lic, note) in DATA_SOURCES {
            data = data.child(
                div()
                    .flex()
                    .gap(px(12.))
                    .font_family(MONO_FONT)
                    .text_size(px(10.5))
                    .child(
                        div()
                            .w(px(220.))
                            .text_color(p.ink)
                            .child(t!(name).into_owned()),
                    )
                    .child(div().w(px(140.)).text_color(p.muted).child(lic))
                    .child(div().text_color(p.muted).child(t!(note).into_owned())),
            );
        }

        div()
            .id("about-screen")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .bg(p.paper)
            .child(
                section(&p)
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(px(12.))
                            .child(div().text_size(px(26.)).text_color(p.ink).child("Emulsion"))
                            .child(mono(env!("CARGO_PKG_VERSION"), 12., p.muted)),
                    )
                    .child(body(&t!("about.intro"), &p))
                    .child(heading(&t!("about.licence"), &p))
                    .child(body(&t!("about.licence_body"), &p))
                    .child(pre(LICENSE.trim(), &p)),
            )
            .child(
                section(&p)
                    .child(heading(&t!("about.gpui"), &p))
                    .child(body(&t!("about.gpui_body"), &p))
                    .child(gpui_rows)
                    .children(GPUI_CHANGES.iter().map(|(_, text)| pre(text.trim(), &p)))
                    .child(pre(GPUI_LICENSING.trim(), &p))
                    .child(pre(NOTICES.trim(), &p))
                    .children(FONT_LICENSES.iter().map(|text| pre(text.trim(), &p))),
            )
            .child(
                section(&p)
                    .child(heading(&t!("about.models"), &p))
                    .child(body(&t!("about.models_body"), &p))
                    .child(models),
            )
            .child(
                section(&p)
                    .child(heading(&t!("about.data"), &p))
                    .child(data),
            )
            .child(
                section(&p)
                    .child(heading(&t!("about.crates", count = all.len()), &p))
                    .child(body(&t!("about.licence_mix", mix = mix_line), &p))
                    .children((!copyleft.is_empty()).then(|| {
                        body(
                            &t!(
                                "about.copyleft",
                                list = copyleft
                                    .iter()
                                    .map(|(n, v, l)| format!("{n} {v} ({l})"))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                            &p,
                        )
                    }))
                    .child(
                        chip(
                            "about-all-crates",
                            if show_all {
                                t!("about.hide_list")
                            } else {
                                t!("about.show_list")
                            },
                            show_all,
                            &p,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.about_all_crates = !this.about_all_crates;
                            cx.notify();
                        })),
                    )
                    .child(crate_rows),
            )
    }
}
