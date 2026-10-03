//! Curated native template families across five design categories.
//! A variant is an alternative composition; pages are only its coordinated content set.
use crate::{
    Document,
    graph::Graph,
    project::{PageMeta, Project, ProjectKind, ProjectPage},
};
use serde::{Deserialize, Serialize};

#[path = "design_invitation_artwork.rs"]
mod artwork;
#[path = "design_family_art.rs"]
mod native_art;
#[path = "design_poster_artwork.rs"]
mod poster_artwork;
#[path = "design_presentation_artwork.rs"]
mod presentation_artwork;
#[path = "design_social_artwork.rs"]
mod social_artwork;
#[cfg(test)]
#[path = "design_invitation_tests.rs"]
mod tests;

/// Legacy invitation canvas size. Prefer `Family::native_size`.
pub const NATIVE_SIZE: (u32, u32) = (1500, 2100);
/// Legacy invitation set size. Prefer `Family::page_labels().len()`.
pub const MATCHING_SET_SIZE: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Wedding,
    Birthday,
    Social,
    Posters,
    Presentations,
}
/// Backward-compatible name for invitation clients.
pub type Occasion = Category;
impl Category {
    pub const ALL: [Self; 5] = [
        Self::Wedding,
        Self::Birthday,
        Self::Social,
        Self::Posters,
        Self::Presentations,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Wedding => "Wedding",
            Self::Birthday => "Birthday",
            Self::Social => "Social",
            Self::Posters => "Posters",
            Self::Presentations => "Presentations",
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
    FieldNotes,
    SignalStudio,
    AfterHours,
    MarketDay,
    StudioBrief,
    MomentumDeck,
}
impl FamilyId {
    pub const ALL: [Self; 10] = [
        Self::GardenVows,
        Self::ModernVows,
        Self::ConfettiClub,
        Self::MidnightToast,
        Self::FieldNotes,
        Self::SignalStudio,
        Self::AfterHours,
        Self::MarketDay,
        Self::StudioBrief,
        Self::MomentumDeck,
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
    FieldJournal,
    FieldSidebar,
    FieldDiptych,
    SignalGrid,
    SignalOrbit,
    SignalBlocks,
    AfterHoursWave,
    AfterHoursStack,
    AfterHoursSpotlight,
    MarketHarvest,
    MarketGrid,
    MarketSunrise,
    StudioSplit,
    StudioType,
    StudioFrame,
    MomentumDashboard,
    MomentumRail,
    MomentumBlueprint,
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
    pub const fn native_size(&self) -> (u32, u32) {
        match self.occasion {
            Category::Wedding | Category::Birthday | Category::Posters => (1500, 2100),
            Category::Social => (1080, 1080),
            Category::Presentations => (1920, 1080),
        }
    }
    /// Content pages belonging to one selected composition, never variant alternatives.
    pub const fn page_labels(&self) -> &'static [&'static str] {
        match self.occasion {
            Category::Wedding | Category::Birthday => &["Invitation", "Details", "RSVP"],
            Category::Social => &["Cover", "Story", "Call to action"],
            Category::Posters => &["Flyer / poster"],
            Category::Presentations => &["Title", "Overview", "Next steps"],
        }
    }
    pub fn selection(&self) -> Selection {
        Selection {
            family: self.id,
            variant: self.variants[0].id,
            palette: 0,
        }
    }
}

pub const FAMILIES: [Family; 10] = [
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
    Family {
        id: FamilyId::FieldNotes,
        occasion: Category::Social,
        label: "Field Notes",
        description: "Slow-living stories, botanical silhouettes and warm editorial typography.",
        variants: [
            Variant {
                id: VariantId::FieldJournal,
                label: "Field journal",
                description: "A quiet notebook composition with sculptural botanical studies.",
            },
            Variant {
                id: VariantId::FieldSidebar,
                label: "Field sidebar",
                description: "A slender botanical side panel beside spacious editorial copy.",
            },
            Variant {
                id: VariantId::FieldDiptych,
                label: "Field diptych",
                description: "Paired botanical panels and a graceful, balanced type system.",
            },
        ],
        palettes: [
            palette(
                "moss",
                "Moss & parchment",
                [0xf0eee3, 0x283e32, 0x496348, 0xb6c5a1, 0xfffdf5],
            ),
            palette(
                "terracotta",
                "Terracotta & linen",
                [0xf3e7dc, 0x5d352b, 0x9b5039, 0xdcae92, 0xfffaf1],
            ),
            palette(
                "indigo",
                "Indigo & mist",
                [0xe8eced, 0x253948, 0x415e75, 0xafc3cd, 0xfafcf9],
            ),
        ],
    },
    Family {
        id: FamilyId::SignalStudio,
        occasion: Category::Social,
        label: "Signal Studio",
        description: "Graphic studio stories with clear ideas, vibrant geometry and bold display type.",
        variants: [
            Variant {
                id: VariantId::SignalGrid,
                label: "Signal grid",
                description: "A modular graphic grid with confident, asymmetric headlines.",
            },
            Variant {
                id: VariantId::SignalOrbit,
                label: "Signal orbit",
                description: "Orbital geometry and broad negative space around expressive type.",
            },
            Variant {
                id: VariantId::SignalBlocks,
                label: "Signal blocks",
                description: "Stacked color fields that give each story beat a distinct rhythm.",
            },
        ],
        palettes: [
            palette(
                "ultramarine",
                "Ultramarine & butter",
                [0xf4edcf, 0x20346e, 0x3b4fa0, 0xf0be63, 0xfffcf0],
            ),
            palette(
                "persimmon",
                "Persimmon & pink",
                [0xf7e4e7, 0x52292e, 0xa94432, 0xe2a4a7, 0xfff8ef],
            ),
            palette(
                "forest",
                "Forest & lime",
                [0xeaf0cd, 0x233c35, 0x3c6654, 0xb9cd70, 0xfafdec],
            ),
        ],
    },
    Family {
        id: FamilyId::AfterHours,
        occasion: Category::Posters,
        label: "After Hours",
        description: "A late-night listening session in luminous geometry and striking typography.",
        variants: [
            Variant {
                id: VariantId::AfterHoursWave,
                label: "Sound wave",
                description: "An oversized sonic waveform beneath an intimate editorial masthead.",
            },
            Variant {
                id: VariantId::AfterHoursStack,
                label: "Night stack",
                description: "A monumental stacked title and a crisp, asymmetric event panel.",
            },
            Variant {
                id: VariantId::AfterHoursSpotlight,
                label: "Spotlight",
                description: "A circular spotlight and framed central typography with cinematic poise.",
            },
        ],
        palettes: [
            palette(
                "electric",
                "Midnight & electric",
                [0x152539, 0xf4f1df, 0xa5cbd3, 0x375772, 0x20364d],
            ),
            palette(
                "ember",
                "Onyx & ember",
                [0x292329, 0xfff0dc, 0xf1aa77, 0x654752, 0x3a2e36],
            ),
            palette(
                "acid",
                "Forest & acid",
                [0x15352f, 0xf5f4df, 0xdae897, 0x477267, 0x25483e],
            ),
        ],
    },
    Family {
        id: FamilyId::MarketDay,
        occasion: Category::Posters,
        label: "Market Day",
        description: "A neighborhood gathering with generous display type and illustrated market produce.",
        variants: [
            Variant {
                id: VariantId::MarketHarvest,
                label: "Harvest stack",
                description: "A joyful stacked market headline above graphic produce silhouettes.",
            },
            Variant {
                id: VariantId::MarketGrid,
                label: "Market grid",
                description: "A tidy vendor-grid poster with oversized date and market details.",
            },
            Variant {
                id: VariantId::MarketSunrise,
                label: "Market sunrise",
                description: "A rising sun and spacious centered lettering for an open-air gathering.",
            },
        ],
        palettes: [
            palette(
                "tomato",
                "Tomato & cream",
                [0xf6ecd4, 0x593c2e, 0xa93f2c, 0xd8af70, 0xfffaf0],
            ),
            palette(
                "olive",
                "Olive & lemon",
                [0xf2efcc, 0x35462d, 0x56733d, 0xc5ca77, 0xfffdef],
            ),
            palette(
                "plum",
                "Plum & peach",
                [0xf5e2d6, 0x51364e, 0x86536e, 0xd6a890, 0xfff9ee],
            ),
        ],
    },
    Family {
        id: FamilyId::StudioBrief,
        occasion: Category::Presentations,
        label: "Studio Brief",
        description: "An editorial presentation system for clear concepts, thoughtful direction and next steps.",
        variants: [
            Variant {
                id: VariantId::StudioSplit,
                label: "Editorial split",
                description: "A sculptural side composition paired with an open editorial content field.",
            },
            Variant {
                id: VariantId::StudioType,
                label: "Type stage",
                description: "Oversized type anchors spacious, sharply ordered story sections.",
            },
            Variant {
                id: VariantId::StudioFrame,
                label: "Gallery frame",
                description: "Fine framing and carefully balanced panels give the work room to breathe.",
            },
        ],
        palettes: [
            palette(
                "ochre",
                "Ochre & paper",
                [0xf0ece0, 0x34342e, 0x806127, 0xcbbc8e, 0xfffcf5],
            ),
            palette(
                "marine",
                "Marine & chalk",
                [0xe8edef, 0x273c51, 0x42627b, 0xb0c7cd, 0xf9fcfb],
            ),
            palette(
                "mulberry",
                "Mulberry & linen",
                [0xf0e5e4, 0x4c3343, 0x805569, 0xd0b0bc, 0xfff9f2],
            ),
        ],
    },
    Family {
        id: FamilyId::MomentumDeck,
        occasion: Category::Presentations,
        label: "Momentum",
        description: "A structured strategy deck with crisp hierarchy, geometric signals and practical action plans.",
        variants: [
            Variant {
                id: VariantId::MomentumDashboard,
                label: "Dashboard",
                description: "Modular panels and a disciplined dashboard grid organize the narrative.",
            },
            Variant {
                id: VariantId::MomentumRail,
                label: "Chapter rail",
                description: "A strong side rail and numbered chapters create a clear progression.",
            },
            Variant {
                id: VariantId::MomentumBlueprint,
                label: "Blueprint",
                description: "Precise rules and diagrammatic accents turn strategy into a readable plan.",
            },
        ],
        palettes: [
            palette(
                "cobalt",
                "Cobalt & ice",
                [0xe9eef4, 0x23364b, 0x335f97, 0xb3c9e0, 0xfcfdff],
            ),
            palette(
                "pine",
                "Pine & mint",
                [0xe6f0e8, 0x24443c, 0x357361, 0xadd0bd, 0xfafff9],
            ),
            palette(
                "aubergine",
                "Aubergine & lilac",
                [0xeee9f5, 0x3e3155, 0x705191, 0xc9b6dd, 0xfffbff],
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
        FamilyId::FieldNotes => 4,
        FamilyId::SignalStudio => 5,
        FamilyId::AfterHours => 6,
        FamilyId::MarketDay => 7,
        FamilyId::StudioBrief => 8,
        FamilyId::MomentumDeck => 9,
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
            return Err("This layout does not belong to the selected template family.".into());
        }
        if self.palette >= family.palettes.len() {
            return Err("Unknown template palette.".into());
        }
        Ok(())
    }
    pub fn palette(self) -> Result<&'static Palette, String> {
        self.validate()?;
        Ok(&family(self.family).palettes[self.palette])
    }
    /// The first content page at this family's native canvas size.
    pub fn primary(self) -> Result<Document, String> {
        let (width, height) = family(self.family).native_size();
        self.create_primary(width, height)
    }
    /// The selected composition's first content page, scaled for a preview or import.
    pub fn create_primary(self, width: u32, height: u32) -> Result<Document, String> {
        self.validate()?;
        create_page(self, 0, width, height)
    }
    /// Creates only the selected composition's coordinated content pages.
    pub fn create(self) -> Result<Project, String> {
        let (width, height) = family(self.family).native_size();
        self.create_sized(width, height)
    }
    /// Uses the same content set at a preview size, never inserting alternative layouts.
    pub fn create_sized(self, width: u32, height: u32) -> Result<Project, String> {
        self.validate()?;
        let family = family(self.family);
        let mut pages = Vec::with_capacity(family.page_labels().len());
        for (index, label) in family.page_labels().iter().enumerate() {
            let doc = create_page(self, index, width, height)?;
            pages.push(ProjectPage {
                meta: PageMeta {
                    id: index as u64 + 1,
                    name: format!("{} · {label}", family.label),
                    bleed_mm: 0.,
                },
                graph: Graph::new(doc.clone(), "Template family starter"),
                doc,
            });
        }
        let project = Project {
            kind: ProjectKind::Design,
            next_page_id: pages.len() as u64 + 1,
            pages,
            active: 1,
            storyboard: None,
        };
        project.validate()?;
        Ok(project)
    }
}

fn create_page(
    selection: Selection,
    page: usize,
    width: u32,
    height: u32,
) -> Result<Document, String> {
    match family(selection.family).occasion {
        Category::Wedding | Category::Birthday => {
            let card = match page {
                0 => artwork::Card::Invitation,
                1 => artwork::Card::Details,
                2 => artwork::Card::Rsvp,
                _ => return Err("Unknown invitation content page.".into()),
            };
            artwork::create(selection, card, width, height)
        }
        Category::Social => social_artwork::create(selection, page, width, height),
        Category::Posters => poster_artwork::create(selection, page, width, height),
        Category::Presentations => presentation_artwork::create(selection, page, width, height),
    }
}
