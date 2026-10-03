//! Two original print-poster systems, each with three alternative compositions.
use super::native_art::{Art, Face};
use super::{FamilyId, Palette, Selection, VariantId};
use crate::{Document, text::Align};

pub(super) fn create(
    selection: Selection,
    page: usize,
    width: u32,
    height: u32,
) -> Result<Document, String> {
    if page != 0 {
        return Err("A poster composition contains one page.".into());
    }
    let p = *selection.palette()?;
    let mut a = Art::new(width, height, p.background)?;
    match selection.family {
        FamilyId::AfterHours => after_hours(&mut a, selection.variant, p),
        FamilyId::MarketDay => market_day(&mut a, selection.variant, p),
        _ => return Err("This family is not a poster.".into()),
    }
    a.finish()
}

fn after_hours(a: &mut Art, variant: VariantId, p: Palette) {
    use Align::{Center, Left, Right};
    match variant {
        VariantId::AfterHoursWave => {
            a.caps(
                "Series label",
                "THE LISTENING ROOM PRESENTS",
                [36., 35., 428., 9.],
                p.accent,
                Left,
            );
            a.line("Masthead rule", (36., 63.), (464., 63.), p.secondary, 1.);
            a.text(
                "Event title",
                "After\nhours",
                [30., 85., 440., 98.],
                Face::Serif,
                p.ink,
                Left,
            );
            a.text(
                "Event introduction",
                "A night for the deep cuts.",
                [37., 302., 426., 18.],
                Face::Italic,
                p.accent,
                Left,
            );
            let heights = [
                20., 42., 65., 97., 136., 89., 55., 33., 73., 112., 147., 103., 69., 43., 79.,
                119., 81., 53., 30., 61., 26.,
            ];
            for (index, h) in heights.into_iter().enumerate() {
                let x = 37. + index as f64 * 20.;
                a.rect(
                    "Sonic waveform bar",
                    [x, 429. - h / 2., 7., h],
                    if index % 3 == 0 { p.ink } else { p.accent },
                );
            }
            a.line(
                "Event information rule",
                (36., 535.),
                (464., 535.),
                p.secondary,
                1.,
            );
            a.caps(
                "Event date",
                "SATURDAY 18 SEPTEMBER",
                [37., 554., 426., 11.],
                p.ink,
                Left,
            );
            a.text(
                "Event time",
                "8 PM — LATE",
                [37., 587., 220., 15.],
                Face::Sans,
                p.accent,
                Left,
            );
            a.text(
                "Venue name",
                "THE LISTENING ROOM",
                [37., 632., 280., 12.],
                Face::Sans,
                p.ink,
                Left,
            );
            a.text(
                "Entry note",
                "FREE ENTRY",
                [330., 632., 133., 10.],
                Face::Sans,
                p.accent,
                Right,
            );
            a.text(
                "Venue address",
                "24 Mercer Street · Music brings us together",
                [37., 657., 426., 10.],
                Face::Sans,
                p.ink,
                Left,
            );
        }
        VariantId::AfterHoursStack => {
            a.rect("Night header", [0., 0., 500., 51.], p.accent);
            a.caps(
                "Series label",
                "AN INDEPENDENT MUSIC NIGHT",
                [32., 17., 436., 9.],
                p.background,
                Left,
            );
            a.text(
                "Event title first line",
                "AFTER",
                [27., 88., 440., 83.],
                Face::Display,
                p.ink,
                Left,
            );
            a.text(
                "Event title second line",
                "HOURS",
                [87., 184., 382., 79.],
                Face::Display,
                p.accent,
                Left,
            );
            a.line("Title underline", (34., 301.), (465., 301.), p.ink, 2.);
            a.text(
                "Event format",
                "VINYL SETS.\nGOOD COMPANY.",
                [35., 332., 307., 26.],
                Face::Sans,
                p.ink,
                Left,
            );
            a.ellipse("Record silhouette", [348., 335., 116., 116.], p.accent);
            a.ellipse("Record label", [384., 371., 44., 44.], p.background);
            a.ellipse("Record center", [403., 390., 6., 6.], p.ink);
            a.rect("Event detail panel", [31., 471., 438., 177.], p.surface);
            a.text(
                "Event date number",
                "18",
                [47., 480., 154., 85.],
                Face::Serif,
                p.accent,
                Left,
            );
            a.caps(
                "Event month",
                "SEPTEMBER",
                [216., 495., 229., 10.],
                p.ink,
                Left,
            );
            a.text(
                "Event day and time",
                "Saturday\n8 PM — late",
                [216., 526., 230., 17.],
                Face::Sans,
                p.ink,
                Left,
            );
            a.line("Date divider", (197., 494.), (197., 583.), p.secondary, 1.);
            a.text(
                "Venue and entry",
                "THE LISTENING ROOM · FREE ENTRY",
                [48., 606., 402., 11.],
                Face::Sans,
                p.ink,
                Left,
            );
            a.text(
                "Venue address",
                "24 Mercer Street",
                [34., 668., 432., 10.],
                Face::Sans,
                p.accent,
                Left,
            );
        }
        VariantId::AfterHoursSpotlight => {
            a.outline("Cinematic frame", [25., 25., 450., 650.], p.secondary, 1.);
            a.ellipse("Spotlight glow", [87., 73., 326., 326.], p.accent);
            a.outline(
                "Inner cinematic frame",
                [33., 33., 434., 634.],
                p.secondary,
                0.5,
            );
            a.caps(
                "Series label",
                "THE LISTENING ROOM",
                [80., 46., 340., 9.],
                p.ink,
                Center,
            );
            a.text(
                "Event title",
                "AFTER\nHOURS",
                [96., 143., 308., 54.],
                Face::Display,
                p.background,
                Center,
            );
            a.caps(
                "Spotlight label",
                "MUSIC AFTER DARK",
                [121., 293., 258., 8.],
                p.background,
                Center,
            );
            a.text(
                "Event introduction",
                "Stay for one more record.",
                [59., 429., 382., 24.],
                Face::Italic,
                p.ink,
                Center,
            );
            a.line("Event ornament", (207., 483.), (293., 483.), p.accent, 1.);
            a.caps(
                "Event date",
                "SATURDAY · 18 SEPTEMBER",
                [61., 510., 378., 10.],
                p.ink,
                Center,
            );
            a.text(
                "Event time and entry",
                "8 PM — late  /  Free entry",
                [62., 546., 376., 16.],
                Face::Serif,
                p.accent,
                Center,
            );
            a.text(
                "Venue address",
                "24 MERCER STREET",
                [73., 609., 354., 11.],
                Face::Sans,
                p.ink,
                Center,
            );
            a.text(
                "Event closing",
                "Independent selectors. Shared discoveries.",
                [64., 640., 372., 11.],
                Face::Serif,
                p.accent,
                Center,
            );
        }
        _ => unreachable!("validated After Hours variant"),
    }
}

/// A small original print illustration assembled from individually editable shapes.
fn produce(a: &mut Art, x: f64, y: f64, scale: f64, p: Palette) {
    a.ellipse(
        "Tomato body",
        [x, y + 30. * scale, 105. * scale, 91. * scale],
        p.accent,
    );
    a.star(
        "Tomato crown",
        x + 52. * scale,
        y + 35. * scale,
        22. * scale,
        5,
        p.ink,
    );
    a.line(
        "Tomato stem",
        (x + 52. * scale, y + 37. * scale),
        (x + 62. * scale, y + 13. * scale),
        p.ink,
        3. * scale,
    );
    a.ellipse(
        "Pear body",
        [x + 130. * scale, y + 44. * scale, 89. * scale, 100. * scale],
        p.secondary,
    );
    a.ellipse(
        "Pear shoulder",
        [x + 147. * scale, y + 8. * scale, 57. * scale, 89. * scale],
        p.secondary,
    );
    a.line(
        "Pear stem",
        (x + 174. * scale, y + 13. * scale),
        (x + 182. * scale, y - 6. * scale),
        p.ink,
        3. * scale,
    );
    a.leaf(
        "Pear leaf",
        (x + 181. * scale, y + 2. * scale),
        (x + 211. * scale, y - 9. * scale),
        9. * scale,
        p.ink,
    );
    a.leaf(
        "Market leaf one",
        (x + 258. * scale, y + 125. * scale),
        (x + 251. * scale, y + 6. * scale),
        29. * scale,
        p.ink,
    );
    a.leaf(
        "Market leaf two",
        (x + 259. * scale, y + 124. * scale),
        (x + 311. * scale, y + 30. * scale),
        25. * scale,
        p.accent,
    );
    a.line(
        "Leaf stem",
        (x + 257. * scale, y + 141. * scale),
        (x + 268. * scale, y + 58. * scale),
        p.ink,
        2. * scale,
    );
}

fn market_day(a: &mut Art, variant: VariantId, p: Palette) {
    use Align::{Center, Left, Right};
    match variant {
        VariantId::MarketHarvest => {
            a.rect("Market paper", [22., 22., 456., 656.], p.surface);
            a.caps(
                "Community label",
                "GROWN LOCAL. MADE WITH CARE.",
                [43., 49., 414., 9.],
                p.ink,
                Left,
            );
            a.text(
                "Market title",
                "MARKET\nDAY",
                [39., 101., 424., 72.],
                Face::Display,
                p.accent,
                Left,
            );
            produce(a, 76., 330., 1.08, p);
            a.line("Harvest table", (43., 491.), (457., 491.), p.secondary, 1.5);
            a.text(
                "Market date",
                "SUNDAY 12 SEPTEMBER",
                [44., 515., 412., 16.],
                Face::Sans,
                p.ink,
                Left,
            );
            a.text(
                "Market hours",
                "9 AM — 2 PM",
                [44., 550., 210., 14.],
                Face::Sans,
                p.accent,
                Left,
            );
            a.text(
                "Market venue",
                "RIVERSIDE SQUARE",
                [44., 593., 412., 15.],
                Face::Sans,
                p.ink,
                Left,
            );
            a.text(
                "Market offer",
                "Seasonal produce · Fresh bakes · Local makers",
                [44., 632., 412., 11.],
                Face::Sans,
                p.ink,
                Left,
            );
        }
        VariantId::MarketGrid => {
            a.caps(
                "Community label",
                "YOUR SUNDAY STARTS HERE",
                [32., 30., 436., 9.],
                p.ink,
                Left,
            );
            a.line("Masthead rule", (32., 58.), (468., 58.), p.ink, 1.);
            a.text(
                "Market title first line",
                "MARKET",
                [30., 78., 440., 57.],
                Face::Display,
                p.accent,
                Left,
            );
            a.text(
                "Market title second line",
                "DAY",
                [31., 156., 279., 95.],
                Face::Display,
                p.ink,
                Left,
            );
            a.rect("Market date tile", [322., 160., 146., 129.], p.accent);
            a.text(
                "Market date number",
                "12",
                [337., 169., 116., 66.],
                Face::Serif,
                p.surface,
                Center,
            );
            a.caps(
                "Market month",
                "SEPT",
                [337., 255., 116., 10.],
                p.surface,
                Center,
            );
            a.rect("Produce tile", [32., 324., 202., 237.], p.secondary);
            a.ellipse("Market apple", [66., 383., 136., 130.], p.accent);
            a.leaf("Apple leaf", (128., 387.), (184., 351.), 18., p.ink);
            a.line("Apple stem", (130., 398.), (123., 359.), p.ink, 4.);
            a.caps(
                "Tile label",
                "FRESH & LOCAL",
                [45., 531., 176., 8.],
                p.ink,
                Center,
            );
            a.text(
                "Market offerings",
                "Fresh\nseasonal\nfinds.",
                [261., 333., 207., 31.],
                Face::Serif,
                p.ink,
                Left,
            );
            a.text(
                "Vendor list",
                "Produce & flowers\nBakes & preserves\nIndependent makers",
                [261., 467., 207., 12.],
                Face::Sans,
                p.ink,
                Left,
            );
            a.line("Market footer rule", (32., 590.), (468., 590.), p.ink, 1.);
            a.text(
                "Market venue",
                "RIVERSIDE SQUARE",
                [32., 615., 312., 15.],
                Face::Sans,
                p.ink,
                Left,
            );
            a.text(
                "Market hours",
                "9 AM — 2 PM",
                [334., 619., 134., 10.],
                Face::Sans,
                p.accent,
                Right,
            );
            a.text(
                "Market closing",
                "SUNDAY · FREE ENTRY · ALL ARE WELCOME",
                [32., 660., 436., 9.],
                Face::Sans,
                p.ink,
                Left,
            );
        }
        VariantId::MarketSunrise => {
            a.outline("Market border", [24., 24., 452., 652.], p.accent, 1.2);
            a.caps(
                "Community label",
                "A GOOD MORNING, TOGETHER",
                [61., 50., 378., 9.],
                p.ink,
                Center,
            );
            a.arch(
                "Market sunrise",
                [89., 105., 322., 188.],
                Some(p.secondary),
                None,
            );
            a.ellipse("Rising sun", [196., 142., 108., 108.], p.accent);
            a.line("Sunrise horizon", (69., 293.), (431., 293.), p.ink, 1.3);
            a.text(
                "Market title",
                "Market Day",
                [47., 323., 406., 55.],
                Face::Serif,
                p.ink,
                Center,
            );
            a.text(
                "Market promise",
                "Meet the people who make it.",
                [66., 399., 368., 19.],
                Face::Italic,
                p.accent,
                Center,
            );
            a.caps(
                "Market date",
                "SUNDAY · 12 SEPTEMBER",
                [66., 470., 368., 11.],
                p.ink,
                Center,
            );
            a.text(
                "Market hours",
                "9 AM — 2 PM",
                [80., 510., 340., 16.],
                Face::Sans,
                p.accent,
                Center,
            );
            a.text(
                "Market venue",
                "RIVERSIDE SQUARE",
                [65., 559., 370., 18.],
                Face::Serif,
                p.ink,
                Center,
            );
            a.text(
                "Market offer",
                "Fresh produce, beautiful flowers,\nand good things from local makers.",
                [62., 601., 376., 13.],
                Face::Sans,
                p.ink,
                Center,
            );
            a.star("Market closing ornament", 250., 658., 5., 4, p.accent);
        }
        _ => unreachable!("validated Market Day variant"),
    }
}
