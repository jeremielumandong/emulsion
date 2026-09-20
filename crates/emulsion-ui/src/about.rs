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

/// Downloaded-on-request content that is not a crate.
const DATA_SOURCES: [(&str, &str, &str); 2] = [
    (
        "lensfun database",
        "CC BY-SA 3.0",
        "Lens distortion and vignetting profiles, fetched from the lensfun project when you install them in Settings.",
    ),
    (
        "Camera looks and starter recipes",
        "Emulsion (MIT)",
        "Written for this project. Recipes you import from the web stay their authors' work and live only in your library.",
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
                        format!("{} · modified by Emulsion", v.repository)
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
                    .child(div().w(px(220.)).text_color(p.ink).child(name))
                    .child(div().w(px(140.)).text_color(p.muted).child(lic))
                    .child(div().text_color(p.muted).child(note)),
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
                    .child(body(
                        "A non-destructive image editor with layers, masks, adjustment nodes and a history you can branch. Brushes behave like their medium, recipes recreate film and camera looks, and camera RAW and Photoshop files open directly. Every AI feature is optional and runs on your machine unless you point it at a server of your own.",
                        &p,
                    ))
                    .child(heading("licence", &p))
                    .child(body(
                        "Emulsion's own code is released under the MIT licence. The third-party code below keeps its own licences; nothing here relicenses it.",
                        &p,
                    ))
                    .child(pre(LICENSE.trim(), &p)),
            )
            .child(
                section(&p)
                    .child(heading("built with GPUI", &p))
                    .child(body(
                        "The user interface runs on GPUI, the GPU-accelerated UI framework from Zed Industries, through the gpui-kit component library by Longbridge. Emulsion ships its own copy of these packages, copied from their published releases and patched in a few files for software rendering and graphics-device recovery. Every change is marked in the file it touches and listed below, as the Apache licence requires.",
                        &p,
                    ))
                    .child(gpui_rows)
                    .children(GPUI_CHANGES.iter().map(|(_, text)| pre(text.trim(), &p)))
                    .child(pre(GPUI_LICENSING.trim(), &p))
                    .child(pre(NOTICES.trim(), &p)),
            )
            .child(
                section(&p)
                    .child(heading("AI models (downloaded only when you ask)", &p))
                    .child(body(
                        "Each model keeps its author's licence. Read the terms before using a model's output commercially; the non-commercial ones say so here.",
                        &p,
                    ))
                    .child(models),
            )
            .child(
                section(&p)
                    .child(heading("data", &p))
                    .child(data),
            )
            .child(
                section(&p)
                    .child(heading(&format!("every crate in this build · {}", all.len()), &p))
                    .child(body(&format!("Licence mix: {mix_line}."), &p))
                    .children((!copyleft.is_empty()).then(|| {
                        body(
                            &format!(
                                "Weak-copyleft dependencies: {}. Their source is unmodified and available from their repositories; the release packages carry their licence texts.",
                                copyleft
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
                            if show_all { "hide the list" } else { "show the full list" },
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
