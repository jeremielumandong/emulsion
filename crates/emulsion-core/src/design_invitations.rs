//! Curated invitation families. A layout is an alternative front, never an extra page.
//! A selection creates only its invitation and its two coordinated companion cards.
use crate::{
    Document,
    graph::Graph,
    project::{PageMeta, Project, ProjectKind, ProjectPage},
};
use serde::{Deserialize, Serialize};

#[path = "design_invitation_artwork.rs"]
mod artwork;
#[cfg(test)]
#[path = "design_invitation_tests.rs"]
mod tests;

pub const NATIVE_SIZE: (u32, u32) = (1500, 2100);
pub const MATCHING_SET_SIZE: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Occasion {
    Wedding,
    Birthday,
}
impl Occasion {
    pub const ALL: [Self; 2] = [Self::Wedding, Self::Birthday];
    pub fn label(self) -> &'static str {
        match self {
            Self::Wedding => "Wedding",
            Self::Birthday => "Birthday",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FamilyId {
    GardenVows,
    ModernVows,
    ConfettiClub,
    MidnightToast,
}
impl FamilyId {
    pub const ALL: [Self; 4] = [
        Self::GardenVows,
        Self::ModernVows,
        Self::ConfettiClub,
        Self::MidnightToast,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VariantId {
    BotanicalArch,
    GardenBorder,
    WildflowerEditorial,
    SplitType,
    Monogram,
    Gallery,
    BigNumber,
    PartyTicket,
    ShapeStack,
    Moonlight,
    ArtDeco,
    SupperClub,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Variant {
    pub id: VariantId,
    pub label: &'static str,
    pub description: &'static str,
}

/// All colors are opaque native sRGB colors; each palette is art-directed as a set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub id: &'static str,
    pub label: &'static str,
    pub background: [u8; 4],
    pub ink: [u8; 4],
    pub accent: [u8; 4],
    pub secondary: [u8; 4],
    pub surface: [u8; 4],
}
const fn rgb(c: u32) -> [u8; 4] {
    [(c >> 16) as u8, (c >> 8) as u8, c as u8, 255]
}
const fn palette(id: &'static str, label: &'static str, colors: [u32; 5]) -> Palette {
    Palette {
        id,
        label,
        background: rgb(colors[0]),
        ink: rgb(colors[1]),
        accent: rgb(colors[2]),
        secondary: rgb(colors[3]),
        surface: rgb(colors[4]),
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Family {
    pub id: FamilyId,
    pub occasion: Occasion,
    pub label: &'static str,
    pub description: &'static str,
    pub variants: [Variant; 3],
    pub palettes: [Palette; 3],
}
impl Family {
    pub fn selection(&self) -> Selection {
        Selection {
            family: self.id,
            variant: self.variants[0].id,
            palette: 0,
        }
    }
}

pub const FAMILIES: [Family; 4] = [
    Family {
        id: FamilyId::GardenVows,
        occasion: Occasion::Wedding,
        label: "Garden Vows",
        description: "Botanical linework, graceful serif type and quiet paper tones.",
        variants: [
            Variant {
                id: VariantId::BotanicalArch,
                label: "Botanical arch",
                description: "An arched paper panel with delicate climbing stems.",
            },
            Variant {
                id: VariantId::GardenBorder,
                label: "Garden border",
                description: "Horizontal names and an oversized date in a fine botanical frame.",
            },
            Variant {
                id: VariantId::WildflowerEditorial,
                label: "Wildflower editorial",
                description: "Asymmetric typography beside a sculptural botanical column.",
            },
        ],
        palettes: [
            palette(
                "sage",
                "Ivory & sage",
                [0xe6eadf, 0x293c31, 0x58705a, 0xb4bf9d, 0xfffcf4],
            ),
            palette(
                "rose",
                "Rose & wine",
                [0xf0dfdd, 0x5c303b, 0x8b5260, 0xc69c9e, 0xfff8f1],
            ),
            palette(
                "mist",
                "Mist & olive",
                [0xe2e8eb, 0x344248, 0x606e51, 0xb3bdac, 0xfaf9f2],
            ),
        ],
    },
    Family {
        id: FamilyId::ModernVows,
        occasion: Occasion::Wedding,
        label: "Modern Heirloom",
        description: "Confident editorial typography and architectural color blocks.",
        variants: [
            Variant {
                id: VariantId::SplitType,
                label: "Split typography",
                description: "Oversized names above a sharply divided information panel.",
            },
            Variant {
                id: VariantId::Monogram,
                label: "Monogram",
                description: "An intimate initials seal and a restrained centered composition.",
            },
            Variant {
                id: VariantId::Gallery,
                label: "Gallery",
                description: "A gallery-poster arrangement with a bold date column.",
            },
        ],
        palettes: [
            palette(
                "ink",
                "Ink & bone",
                [0xf1eee6, 0x262627, 0x474849, 0xc9c4b8, 0xfffcf5],
            ),
            palette(
                "cobalt",
                "Cobalt & cream",
                [0xf7f2e6, 0x1d3194, 0x3149ae, 0xc3cbed, 0xfffdf4],
            ),
            palette(
                "clay",
                "Clay & oat",
                [0xf0e3d4, 0x76392a, 0xa34f35, 0xdcb094, 0xfff8ed],
            ),
        ],
    },
    Family {
        id: FamilyId::ConfettiClub,
        occasion: Occasion::Birthday,
        label: "Confetti Club",
        description: "Joyful display type, punchy shapes and playful color.",
        variants: [
            Variant {
                id: VariantId::BigNumber,
                label: "Big birthday",
                description: "A giant age badge surrounded by celebratory confetti.",
            },
            Variant {
                id: VariantId::PartyTicket,
                label: "Party ticket",
                description: "A playful admission ticket with its own information stub.",
            },
            Variant {
                id: VariantId::ShapeStack,
                label: "Shape party",
                description: "A bold stacked headline with graphic blocks and a starburst.",
            },
        ],
        palettes: [
            palette(
                "tangerine",
                "Tangerine & lilac",
                [0xf5e8d4, 0x392455, 0xc84b29, 0xc9b7ed, 0xfffcf3],
            ),
            palette(
                "lemon",
                "Cobalt & lemon",
                [0xf4e9a5, 0x1c3390, 0x294dbb, 0xf1bf50, 0xfffbee],
            ),
            palette(
                "cherry",
                "Cherry & pink",
                [0xf7dce3, 0x752338, 0xbc2946, 0xe9a4b6, 0xfff7ec],
            ),
        ],
    },
    Family {
        id: FamilyId::MidnightToast,
        occasion: Occasion::Birthday,
        label: "Midnight Toast",
        description: "After-dark elegance, fine geometric details and refined serif type.",
        variants: [
            Variant {
                id: VariantId::Moonlight,
                label: "Moonlight",
                description: "A luminous roundel over a poised evening invitation.",
            },
            Variant {
                id: VariantId::ArtDeco,
                label: "Deco frame",
                description: "Stepped geometric framing and a dramatic centered numeral.",
            },
            Variant {
                id: VariantId::SupperClub,
                label: "Supper club",
                description: "An editorial dinner-party card with a tall side panel.",
            },
        ],
        palettes: [
            palette(
                "champagne",
                "Onyx & champagne",
                [0x242829, 0xf9f2df, 0xd9bc83, 0x4b514f, 0x333a3a],
            ),
            palette(
                "copper",
                "Midnight & copper",
                [0x202d40, 0xfff0de, 0xecb18c, 0x485569, 0x2a3c55],
            ),
            palette(
                "pearl",
                "Forest & pearl",
                [0x1e3932, 0xf4f0dd, 0xddd4ae, 0x496357, 0x2b4940],
            ),
        ],
    },
];

pub fn families(occasion: Occasion) -> impl Iterator<Item = &'static Family> {
    FAMILIES
        .iter()
        .filter(move |family| family.occasion == occasion)
}
pub fn family(id: FamilyId) -> &'static Family {
    &FAMILIES[match id {
        FamilyId::GardenVows => 0,
        FamilyId::ModernVows => 1,
        FamilyId::ConfettiClub => 2,
        FamilyId::MidnightToast => 3,
    }]
}

/// A complete, serializable catalog choice. Invalid cross-family variants and
/// palette indices are rejected, rather than silently substituting artwork.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub family: FamilyId,
    pub variant: VariantId,
    pub palette: usize,
}
impl Selection {
    pub fn for_family(id: FamilyId) -> Self {
        family(id).selection()
    }
    pub fn validate(self) -> Result<(), String> {
        let family = family(self.family);
        if !family.variants.iter().any(|v| v.id == self.variant) {
            return Err("This layout does not belong to the selected invitation family.".into());
        }
        if self.palette >= family.palettes.len() {
            return Err("Unknown invitation palette.".into());
        }
        Ok(())
    }
    pub fn palette(self) -> Result<&'static Palette, String> {
        self.validate()?;
        Ok(&family(self.family).palettes[self.palette])
    }
    pub fn create_primary(self, width: u32, height: u32) -> Result<Document, String> {
        self.validate()?;
        artwork::create(self, artwork::Card::Invitation, width, height)
    }
    /// Builds only the chosen front and its matching Details and RSVP cards.
    pub fn create(self) -> Result<Project, String> {
        self.create_sized(NATIVE_SIZE.0, NATIVE_SIZE.1)
    }
    /// Scaled matching-set previews use the same layouts and never alternate fronts.
    pub fn create_sized(self, width: u32, height: u32) -> Result<Project, String> {
        self.validate()?;
        let family = family(self.family);
        let mut pages = Vec::with_capacity(MATCHING_SET_SIZE);
        for (index, (card, label)) in [
            (artwork::Card::Invitation, "Invitation"),
            (artwork::Card::Details, "Details"),
            (artwork::Card::Rsvp, "RSVP"),
        ]
        .into_iter()
        .enumerate()
        {
            let doc = artwork::create(self, card, width, height)?;
            pages.push(ProjectPage {
                meta: PageMeta {
                    id: index as u64 + 1,
                    name: format!("{} · {label}", family.label),
                    bleed_mm: 0.,
                },
                graph: Graph::new(doc.clone(), "Invitation starter"),
                doc,
            });
        }
        let project = Project {
            kind: ProjectKind::Design,
            pages,
            active: 1,
            next_page_id: 4,
            storyboard: None,
        };
        project.validate()?;
        Ok(project)
    }
}
