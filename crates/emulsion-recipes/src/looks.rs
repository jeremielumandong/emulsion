//! Base looks: Emulsion's own approximations of the film simulations
//! people write recipes against, as stacks of ordinary adjustments.
//!
//! These are described by character, not fitted to any camera's output,
//! and named descriptively. A `.cube` LUT can stand in for any of them.

use emulsion_raster::adjust::{Adjustment, Stop, straight_curve};

pub struct Look {
    /// The key recipes use, e.g. `classic-chrome`.
    pub key: &'static str,
    /// What to call it in menus.
    pub label: &'static str,
    /// Also accepted when importing text.
    pub aliases: &'static [&'static str],
    pub build: fn() -> Vec<Adjustment>,
}

fn curves(
    master: Vec<[f32; 2]>,
    red: Vec<[f32; 2]>,
    green: Vec<[f32; 2]>,
    blue: Vec<[f32; 2]>,
) -> Adjustment {
    Adjustment::Curves {
        master,
        red,
        green,
        blue,
    }
}

fn master(pts: Vec<[f32; 2]>) -> Adjustment {
    curves(pts, straight_curve(), straight_curve(), straight_curve())
}

fn sat(saturation: f32) -> Adjustment {
    Adjustment::HueSaturation {
        hue: 0.0,
        saturation,
        lightness: 0.0,
    }
}

fn balance(shadows: [f32; 3], midtones: [f32; 3], highlights: [f32; 3]) -> Adjustment {
    Adjustment::ColorBalance {
        shadows,
        midtones,
        highlights,
        preserve_luminosity: true,
    }
}

fn bw(reds: f32, yellows: f32, greens: f32, cyans: f32, blues: f32, magentas: f32) -> Adjustment {
    Adjustment::BlackAndWhite {
        reds,
        yellows,
        greens,
        cyans,
        blues,
        magentas,
        tint_hue: 42.0,
        tint_strength: 0.0,
    }
}

const S_CURVE: [[f32; 2]; 4] = [[0.0, 0.0], [64.0, 56.0], [192.0, 200.0], [255.0, 255.0]];

pub const LOOKS: &[Look] = &[
    Look {
        key: "provia",
        label: "Standard style",
        aliases: &["standard", "std"],
        build: || vec![master(S_CURVE.to_vec()), sat(6.0)],
    },
    Look {
        key: "velvia",
        label: "Vivid slide style",
        aliases: &["vivid"],
        build: || {
            vec![
                master(vec![
                    [0.0, 0.0],
                    [56.0, 44.0],
                    [200.0, 212.0],
                    [255.0, 255.0],
                ]),
                sat(32.0),
                balance([0.0, 0.0, 6.0], [4.0, 0.0, 0.0], [0.0, 0.0, -4.0]),
            ]
        },
    },
    Look {
        key: "astia",
        label: "Soft portrait style",
        aliases: &["soft"],
        build: || {
            vec![
                master(vec![
                    [0.0, 8.0],
                    [64.0, 66.0],
                    [192.0, 196.0],
                    [255.0, 250.0],
                ]),
                sat(10.0),
                balance([0.0; 3], [4.0, 0.0, -3.0], [3.0, 0.0, -2.0]),
            ]
        },
    },
    Look {
        key: "classic-chrome",
        label: "Classic chrome style",
        aliases: &["chrome", "classic chrome"],
        build: || {
            vec![
                master(vec![
                    [0.0, 6.0],
                    [60.0, 50.0],
                    [200.0, 204.0],
                    [255.0, 248.0],
                ]),
                sat(-18.0),
                balance([-6.0, 2.0, 8.0], [-3.0, 0.0, 2.0], [2.0, 0.0, -4.0]),
                Adjustment::Vibrance {
                    vibrance: -10.0,
                    saturation: 0.0,
                },
            ]
        },
    },
    Look {
        key: "classic-negative",
        label: "Classic negative style",
        aliases: &["classic neg", "classic negative", "neg"],
        build: || {
            vec![
                curves(
                    vec![[0.0, 10.0], [64.0, 52.0], [192.0, 204.0], [255.0, 250.0]],
                    straight_curve(),
                    vec![[0.0, 0.0], [128.0, 132.0], [255.0, 255.0]],
                    vec![[0.0, 12.0], [128.0, 122.0], [255.0, 244.0]],
                ),
                sat(8.0),
                balance([-8.0, 4.0, 10.0], [2.0, -2.0, -4.0], [8.0, 2.0, -10.0]),
            ]
        },
    },
    Look {
        key: "pro-neg-std",
        label: "Pro negative standard style",
        aliases: &["pro neg std", "pro neg. std", "pro negative standard"],
        build: || {
            vec![
                master(vec![
                    [0.0, 6.0],
                    [64.0, 62.0],
                    [192.0, 194.0],
                    [255.0, 250.0],
                ]),
                sat(-8.0),
                balance([0.0; 3], [3.0, 0.0, -2.0], [0.0; 3]),
            ]
        },
    },
    Look {
        key: "pro-neg-hi",
        label: "Pro negative high style",
        aliases: &["pro neg hi", "pro neg. hi", "pro negative high"],
        build: || {
            vec![
                master(vec![
                    [0.0, 0.0],
                    [64.0, 54.0],
                    [192.0, 202.0],
                    [255.0, 255.0],
                ]),
                sat(-2.0),
                balance([0.0; 3], [3.0, 0.0, -2.0], [0.0; 3]),
            ]
        },
    },
    Look {
        key: "eterna",
        label: "Cinema flat style",
        aliases: &["cinema", "eterna cinema"],
        build: || {
            vec![
                master(vec![
                    [0.0, 16.0],
                    [64.0, 70.0],
                    [192.0, 190.0],
                    [255.0, 240.0],
                ]),
                sat(-28.0),
                balance([0.0, 2.0, 6.0], [0.0; 3], [2.0, 0.0, -2.0]),
            ]
        },
    },
    Look {
        key: "eterna-bleach-bypass",
        label: "Bleach bypass style",
        aliases: &["bleach bypass", "bleach"],
        build: || {
            vec![
                master(vec![
                    [0.0, 0.0],
                    [48.0, 30.0],
                    [208.0, 226.0],
                    [255.0, 255.0],
                ]),
                sat(-60.0),
            ]
        },
    },
    Look {
        key: "nostalgic-negative",
        label: "Nostalgic negative style",
        aliases: &["nostalgic neg", "nostalgic negative", "nostalgic"],
        build: || {
            vec![
                master(vec![
                    [0.0, 12.0],
                    [64.0, 64.0],
                    [192.0, 200.0],
                    [255.0, 246.0],
                ]),
                sat(4.0),
                balance([0.0, 0.0, 4.0], [4.0, 0.0, -4.0], [14.0, 4.0, -18.0]),
            ]
        },
    },
    Look {
        key: "reala-ace",
        label: "Faithful negative style",
        aliases: &["reala", "reala ace"],
        build: || {
            vec![
                master(vec![
                    [0.0, 2.0],
                    [64.0, 58.0],
                    [192.0, 198.0],
                    [255.0, 254.0],
                ]),
                sat(4.0),
            ]
        },
    },
    Look {
        key: "acros",
        label: "Fine-grain monochrome style",
        aliases: &["acros std", "acros standard"],
        build: || {
            vec![
                bw(60.0, 60.0, 50.0, 50.0, 30.0, 70.0),
                master(S_CURVE.to_vec()),
            ]
        },
    },
    Look {
        key: "acros-ye",
        label: "Monochrome, yellow filter",
        aliases: &["acros+ye", "acros ye", "acros+y", "acros yellow"],
        build: || {
            vec![
                bw(90.0, 120.0, 60.0, 30.0, -20.0, 60.0),
                master(S_CURVE.to_vec()),
            ]
        },
    },
    Look {
        key: "acros-r",
        label: "Monochrome, red filter",
        aliases: &["acros+r", "acros r", "acros red"],
        build: || {
            vec![
                bw(150.0, 110.0, 20.0, -20.0, -60.0, 90.0),
                master(vec![
                    [0.0, 0.0],
                    [64.0, 50.0],
                    [192.0, 206.0],
                    [255.0, 255.0],
                ]),
            ]
        },
    },
    Look {
        key: "acros-g",
        label: "Monochrome, green filter",
        aliases: &["acros+g", "acros g", "acros green"],
        build: || {
            vec![
                bw(40.0, 90.0, 130.0, 60.0, 0.0, 30.0),
                master(S_CURVE.to_vec()),
            ]
        },
    },
    Look {
        key: "monochrome",
        label: "Monochrome style",
        aliases: &["mono", "b&w", "black and white"],
        build: || vec![bw(40.0, 60.0, 40.0, 60.0, 20.0, 80.0)],
    },
    Look {
        key: "monochrome-ye",
        label: "Monochrome style, yellow filter",
        aliases: &["monochrome+ye", "mono+ye"],
        build: || vec![bw(80.0, 110.0, 50.0, 30.0, -10.0, 60.0)],
    },
    Look {
        key: "monochrome-r",
        label: "Monochrome style, red filter",
        aliases: &["monochrome+r", "mono+r"],
        build: || vec![bw(140.0, 100.0, 20.0, -10.0, -50.0, 90.0)],
    },
    Look {
        key: "monochrome-g",
        label: "Monochrome style, green filter",
        aliases: &["monochrome+g", "mono+g"],
        build: || vec![bw(30.0, 90.0, 130.0, 60.0, 0.0, 30.0)],
    },
    Look {
        key: "sepia",
        label: "Sepia",
        aliases: &[],
        build: || {
            vec![Adjustment::GradientMap {
                stops: vec![
                    Stop {
                        pos: 0.0,
                        color: [24, 14, 6],
                    },
                    Stop {
                        pos: 0.55,
                        color: [160, 120, 80],
                    },
                    Stop {
                        pos: 1.0,
                        color: [250, 242, 226],
                    },
                ],
                reverse: false,
            }]
        },
    },
];

/// Find a look by key, label or alias, ignoring case and punctuation.
pub fn find(name: &str) -> Option<&'static Look> {
    let n = norm(name);
    if n.is_empty() {
        return None;
    }
    LOOKS
        .iter()
        .find(|l| norm(l.key) == n || norm(l.label) == n || l.aliases.iter().any(|a| norm(a) == n))
}

fn norm(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '+')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_look_builds_and_is_findable() {
        for l in LOOKS {
            assert!(!(l.build)().is_empty(), "{}", l.key);
            assert_eq!(find(l.key).map(|f| f.key), Some(l.key));
            assert_eq!(find(l.label).map(|f| f.key), Some(l.key));
        }
        assert_eq!(find("Classic Chrome").unwrap().key, "classic-chrome");
        assert_eq!(find("ACROS+R").unwrap().key, "acros-r");
        assert_eq!(find("Pro Neg. Hi").unwrap().key, "pro-neg-hi");
        assert!(find("kodachrome").is_none());
    }
}
