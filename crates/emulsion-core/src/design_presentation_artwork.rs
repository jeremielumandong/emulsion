//! Editable, purpose-built three-slide decks on a 960 × 540 design grid.
use super::native_art::{Art, Face};
use super::{FamilyId, Palette, Selection, VariantId};
use crate::{Document, text::Align};

pub(super) fn create(
    selection: Selection,
    page: usize,
    width: u32,
    height: u32,
) -> Result<Document, String> {
    if page >= 3 {
        return Err("Unknown presentation page.".into());
    }
    let p = *selection.palette()?;
    let mut a = Art::with_grid(width, height, (960., 540.), p.background)?;
    match (selection.family, selection.variant) {
        (FamilyId::StudioBrief, VariantId::StudioSplit) => studio_split(&mut a, page, p),
        (FamilyId::StudioBrief, VariantId::StudioType) => studio_type(&mut a, page, p),
        (FamilyId::StudioBrief, VariantId::StudioFrame) => studio_frame(&mut a, page, p),
        (FamilyId::MomentumDeck, VariantId::MomentumDashboard) => dashboard(&mut a, page, p),
        (FamilyId::MomentumDeck, VariantId::MomentumRail) => rail(&mut a, page, p),
        (FamilyId::MomentumDeck, VariantId::MomentumBlueprint) => blueprint(&mut a, page, p),
        _ => return Err("This layout is not a presentation family.".into()),
    }
    a.finish()
}

fn text(a: &mut Art, name: &str, copy: &str, r: [f64; 4], face: Face, p: Palette) {
    a.text(name, copy, r, face, p.ink, Align::Left);
}

fn label(a: &mut Art, name: &str, copy: &str, r: [f64; 4], p: Palette) {
    a.caps(name, copy, r, p.ink, Align::Left);
}

fn page_name(page: usize) -> &'static str {
    ["TITLE", "OVERVIEW", "NEXT STEPS"][page]
}

fn footer(a: &mut Art, family: &str, page: usize, p: Palette) {
    label(a, "Deck identity", family, [54., 501., 580., 9.], p);
    a.caps(
        "Slide index",
        &format!("{}  /  0{}", page_name(page), page + 1),
        [678., 501., 226., 9.],
        p.ink,
        Align::Right,
    );
}

// A quiet editorial spread: the right-hand column is a distinct content panel.
fn studio_split(a: &mut Art, page: usize, p: Palette) {
    a.rect("Editorial side panel", [620., 0., 340., 540.], p.surface);
    a.line("Column rule", (620., 40.), (620., 474.), p.secondary, 1.);
    label(
        a,
        "Studio identifier",
        "STUDIO / DESIGN BRIEF",
        [54., 43., 515., 11.],
        p,
    );
    match page {
        0 => {
            text(
                a,
                "Cover title",
                "A clearer\npoint of view.",
                [54., 128., 530., 73.],
                Face::Serif,
                p,
            );
            text(
                a,
                "Cover statement",
                "A creative direction for a brand\nready to make its next move.",
                [58., 327., 490., 22.],
                Face::Sans,
                p,
            );
            a.line("Cover accent", (58., 416.), (129., 416.), p.accent, 3.);
            label(
                a,
                "Cover edition",
                "CREATIVE STRATEGY  /  2027",
                [58., 443., 510., 10.],
                p,
            );
            a.rect("Composition base", [684., 123., 212., 220.], p.secondary);
            a.ellipse("Composition circle", [704., 91., 172., 172.], p.accent);
            a.rect("Composition inset", [754., 247., 142., 96.], p.background);
            a.line("Composition axis", (667., 370.), (910., 370.), p.ink, 1.);
            label(
                a,
                "Composition caption",
                "CLARITY / CHARACTER / CRAFT",
                [660., 395., 265., 9.],
                p,
            );
            text(
                a,
                "Composition note",
                "Built around one\nrecognizable idea.",
                [660., 427., 258., 21.],
                Face::Serif,
                p,
            );
        }
        1 => {
            text(
                a,
                "Overview title",
                "The creative\ndirection",
                [54., 91., 530., 53.],
                Face::Serif,
                p,
            );
            text(
                a,
                "Overview introduction",
                "Make every choice reinforce the same idea.",
                [58., 221., 511., 17.],
                Face::Sans,
                p,
            );
            for (i, (title, body)) in [
                (
                    "Start with the essential",
                    "A focused story with a clear reason to care.",
                ),
                (
                    "Create a distinct voice",
                    "Warm language, confident type, thoughtful space.",
                ),
                (
                    "Build a flexible system",
                    "A shared visual rhythm across every touchpoint.",
                ),
            ]
            .into_iter()
            .enumerate()
            {
                let y = 272. + i as f64 * 70.;
                label(
                    a,
                    "Direction number",
                    &format!("0{}", i + 1),
                    [58., y + 5., 40., 11.],
                    p,
                );
                text(
                    a,
                    "Direction heading",
                    title,
                    [111., y, 465., 23.],
                    Face::Serif,
                    p,
                );
                text(
                    a,
                    "Direction description",
                    body,
                    [111., y + 31., 465., 13.5],
                    Face::Sans,
                    p,
                );
                if i < 2 {
                    a.line(
                        "Direction divider",
                        (111., y + 58.),
                        (568., y + 58.),
                        p.secondary,
                        0.7,
                    );
                }
            }
            label(
                a,
                "North star label",
                "THE NORTH STAR",
                [660., 103., 256., 10.],
                p,
            );
            text(
                a,
                "North star statement",
                "Fewer things.\nMore meaning.",
                [659., 166., 261., 38.],
                Face::Serif,
                p,
            );
            a.rect("North star swatch", [662., 286., 60., 60.], p.accent);
            a.rect(
                "North star companion swatch",
                [736., 286., 60., 60.],
                p.secondary,
            );
            a.outline(
                "North star paper swatch",
                [810., 286., 60., 60.],
                p.ink,
                0.8,
            );
            text(
                a,
                "North star criteria",
                "Clear at a glance.\nRecognizable in detail.\nUseful in the real world.",
                [661., 381., 258., 17.],
                Face::Sans,
                p,
            );
        }
        _ => {
            text(
                a,
                "Next steps title",
                "From direction\nto design",
                [54., 91., 530., 53.],
                Face::Serif,
                p,
            );
            for (i, (when, title, body)) in [
                (
                    "WEEK 01",
                    "Align",
                    "Agree on the audience, story and success criteria.",
                ),
                (
                    "WEEK 02",
                    "Explore",
                    "Develop two routes and test the strongest ideas.",
                ),
                (
                    "WEEK 03",
                    "Refine",
                    "Resolve one direction into a practical design kit.",
                ),
            ]
            .into_iter()
            .enumerate()
            {
                let y = 244. + i as f64 * 78.;
                label(a, "Step timing", when, [58., y + 7., 107., 9.], p);
                text(
                    a,
                    "Step heading",
                    title,
                    [189., y, 380., 27.],
                    Face::Serif,
                    p,
                );
                text(
                    a,
                    "Step description",
                    body,
                    [189., y + 35., 387., 13.],
                    Face::Sans,
                    p,
                );
                a.line(
                    "Step divider",
                    (58., y + 64.),
                    (570., y + 64.),
                    p.secondary,
                    0.7,
                );
            }
            label(
                a,
                "Deliverables label",
                "WHAT WE WILL MAKE",
                [660., 103., 262., 10.],
                p,
            );
            text(
                a,
                "Deliverables heading",
                "A working\ncreative toolkit",
                [660., 151., 259., 35.],
                Face::Serif,
                p,
            );
            for (i, item) in [
                "Core story & messages",
                "Type & color system",
                "Key application studies",
                "A concise usage guide",
            ]
            .into_iter()
            .enumerate()
            {
                let y = 277. + i as f64 * 41.;
                a.rect("Deliverable bullet", [662., y + 5., 6., 6.], p.accent);
                text(a, "Deliverable", item, [684., y, 230., 15.], Face::Sans, p);
            }
            label(
                a,
                "Decision label",
                "FIRST DECISION: ALIGN ON THE BRIEF",
                [660., 459., 266., 8.],
                p,
            );
        }
    }
    footer(a, "STUDIO BRIEF", page, p);
}

// A typographic journal: expansive titles, ruled notes, and a clean action ledger.
fn studio_type(a: &mut Art, page: usize, p: Palette) {
    a.line("Masthead rule", (54., 71.), (906., 71.), p.ink, 0.8);
    label(
        a,
        "Journal masthead",
        "THE STUDIO JOURNAL",
        [54., 42., 550., 11.],
        p,
    );
    a.caps(
        "Journal section",
        page_name(page),
        [659., 42., 247., 10.],
        p.ink,
        Align::Right,
    );
    match page {
        0 => {
            text(
                a,
                "Cover title",
                "Make room\nfor what’s next.",
                [54., 119., 852., 84.],
                Face::Serif,
                p,
            );
            a.ellipse("Editorial punctuation", [806., 291., 67., 67.], p.accent);
            a.line("Cover lower rule", (54., 393.), (906., 393.), p.ink, 0.8);
            label(
                a,
                "Cover volume",
                "01 / A CREATIVE BRIEF",
                [54., 420., 360., 10.],
                p,
            );
            text(
                a,
                "Cover subtitle",
                "A focused story. A distinctive expression.\nA system that holds it all together.",
                [484., 416., 418., 19.],
                Face::Sans,
                p,
            );
        }
        1 => {
            text(
                a,
                "Overview title",
                "Three ideas. One voice.",
                [54., 107., 852., 58.],
                Face::Serif,
                p,
            );
            text(
                a,
                "Overview introduction",
                "A few considered principles make a stronger whole.",
                [57., 191., 830., 20.],
                Face::Sans,
                p,
            );
            for (i, (numeral, title, body, detail)) in [
                (
                    "I",
                    "Be clear",
                    "Lead with the idea.\nLeave the noise behind.",
                    "STORY / MESSAGE",
                ),
                (
                    "II",
                    "Be distinct",
                    "Pair expressive type\nwith a confident palette.",
                    "TYPE / COLOR",
                ),
                (
                    "III",
                    "Be coherent",
                    "Repeat the right details.\nAdapt to the context.",
                    "SYSTEM / APPLICATION",
                ),
            ]
            .into_iter()
            .enumerate()
            {
                let x = 55. + i as f64 * 294.;
                if i > 0 {
                    a.line(
                        "Principle column rule",
                        (x - 24., 266.),
                        (x - 24., 459.),
                        p.secondary,
                        0.8,
                    );
                }
                text(
                    a,
                    "Principle numeral",
                    numeral,
                    [x, 254., 245., 44.],
                    Face::Italic,
                    p,
                );
                text(
                    a,
                    "Principle title",
                    title,
                    [x, 318., 258., 29.],
                    Face::Serif,
                    p,
                );
                text(
                    a,
                    "Principle body",
                    body,
                    [x, 365., 258., 17.],
                    Face::Sans,
                    p,
                );
                label(a, "Principle scope", detail, [x, 442., 260., 9.], p);
            }
        }
        _ => {
            text(
                a,
                "Next steps title",
                "The work ahead.",
                [54., 107., 852., 62.],
                Face::Serif,
                p,
            );
            text(
                a,
                "Next steps introduction",
                "Move from a shared ambition to a usable creative system.",
                [57., 190., 830., 19.],
                Face::Sans,
                p,
            );
            label(a, "Ledger timing header", "WHEN", [56., 248., 120., 9.], p);
            label(
                a,
                "Ledger action header",
                "ACTION",
                [197., 248., 371., 9.],
                p,
            );
            label(
                a,
                "Ledger outcome header",
                "OUTCOME",
                [632., 248., 270., 9.],
                p,
            );
            a.line("Ledger header rule", (54., 271.), (906., 271.), p.ink, 1.);
            for (i, (when, action, outcome)) in [
                ("01 / ALIGN", "Confirm the brief", "One agreed direction"),
                (
                    "02 / EXPLORE",
                    "Make the ideas tangible",
                    "Two concept routes",
                ),
                (
                    "03 / REFINE",
                    "Build the chosen system",
                    "A ready-to-use design kit",
                ),
            ]
            .into_iter()
            .enumerate()
            {
                let y = 291. + i as f64 * 58.;
                label(a, "Ledger timing", when, [56., y + 5., 138., 9.], p);
                text(
                    a,
                    "Ledger action",
                    action,
                    [197., y, 410., 25.],
                    Face::Serif,
                    p,
                );
                text(
                    a,
                    "Ledger outcome",
                    outcome,
                    [632., y + 4., 270., 15.],
                    Face::Sans,
                    p,
                );
                a.line(
                    "Ledger row rule",
                    (54., y + 44.),
                    (906., y + 44.),
                    p.secondary,
                    0.7,
                );
            }
            label(
                a,
                "Ledger decision",
                "NEXT CONVERSATION / CHOOSE THE IDEA WORTH DEVELOPING",
                [56., 474., 843., 9.],
                p,
            );
        }
    }
    footer(a, "STUDIO BRIEF / EDITORIAL SERIES", page, p);
}

// A gallery frame: centered cover, curatorial overview, and a triptych work plan.
fn studio_frame(a: &mut Art, page: usize, p: Palette) {
    a.outline("Gallery outer frame", [27., 26., 906., 460.], p.ink, 1.);
    a.outline(
        "Gallery inner frame",
        [34., 33., 892., 446.],
        p.secondary,
        0.7,
    );
    a.caps(
        "Gallery identifier",
        "STUDIO BRIEF / CREATIVE DIRECTION",
        [91., 62., 778., 10.],
        p.ink,
        Align::Center,
    );
    match page {
        0 => {
            a.rect("Gallery emblem square", [461., 107., 38., 38.], p.secondary);
            a.ellipse("Gallery emblem circle", [470., 98., 38., 38.], p.accent);
            a.text(
                "Cover title",
                "A considered\npoint of view.",
                [116., 170., 728., 70.],
                Face::Serif,
                p.ink,
                Align::Center,
            );
            a.line(
                "Gallery title accent",
                (447., 346.),
                (513., 346.),
                p.accent,
                2.,
            );
            a.text(
                "Cover subtitle",
                "An identity shaped by clarity, character and care.",
                [119., 377., 722., 20.],
                Face::Sans,
                p.ink,
                Align::Center,
            );
            a.caps(
                "Cover edition",
                "CONCEPT PRESENTATION / 2027",
                [171., 434., 618., 9.],
                p.ink,
                Align::Center,
            );
        }
        1 => {
            text(
                a,
                "Overview title",
                "One idea.\nEvery detail.",
                [75., 146., 283., 45.],
                Face::Serif,
                p,
            );
            text(
                a,
                "Overview introduction",
                "A small set of principles\nto guide the whole system.",
                [79., 278., 263., 17.],
                Face::Sans,
                p,
            );
            a.line(
                "Gallery column rule",
                (368., 130.),
                (368., 440.),
                p.secondary,
                0.8,
            );
            for (i, (num, title, body)) in [
                ("01", "The story", "A human message, told with precision.\nMake the benefit easy to understand."),
                ("02", "The expression", "Distinctive type and a considered palette.\nBuild recognition through simple details."),
                ("03", "The system", "Clear rules with room for variation.\nKeep every application part of the same story."),
            ].into_iter().enumerate() {
                let y = 127. + i as f64 * 106.;
                label(a, "Curatorial number", num, [402., y + 8., 36., 10.], p);
                text(a, "Curatorial heading", title, [451., y, 425., 28.], Face::Serif, p);
                text(a, "Curatorial detail", body, [451., y + 42., 425., 15.], Face::Sans, p);
                if i < 2 { a.line("Curatorial divider", (402., y + 87.), (879., y + 87.), p.secondary, 0.7); }
            }
        }
        _ => {
            a.text(
                "Next steps title",
                "Make the direction real.",
                [95., 111., 770., 49.],
                Face::Serif,
                p.ink,
                Align::Center,
            );
            a.text(
                "Next steps introduction",
                "Three focused stages, with a decision at each one.",
                [105., 181., 750., 18.],
                Face::Sans,
                p.ink,
                Align::Center,
            );
            for (i, (when, title, body, result)) in [
                (
                    "01 / ALIGN",
                    "Define",
                    "Audience, ambition\nand the core story.",
                    "AN AGREED BRIEF",
                ),
                (
                    "02 / EXPLORE",
                    "Develop",
                    "Two creative routes\nand key applications.",
                    "A CHOSEN DIRECTION",
                ),
                (
                    "03 / REFINE",
                    "Deliver",
                    "A resolved system\nand a practical guide.",
                    "A WORKING TOOLKIT",
                ),
            ]
            .into_iter()
            .enumerate()
            {
                let x = 74. + i as f64 * 274.;
                a.rect("Work plan panel", [x, 239., 264., 190.], p.surface);
                a.line(
                    "Work plan panel accent",
                    (x + 18., 254.),
                    (x + 61., 254.),
                    p.accent,
                    2.,
                );
                label(a, "Work plan stage", when, [x + 18., 273., 224., 9.], p);
                text(
                    a,
                    "Work plan title",
                    title,
                    [x + 18., 307., 226., 31.],
                    Face::Serif,
                    p,
                );
                text(
                    a,
                    "Work plan detail",
                    body,
                    [x + 18., 354., 226., 16.],
                    Face::Sans,
                    p,
                );
                label(a, "Work plan outcome", result, [x + 18., 407., 228., 8.], p);
            }
            a.caps(
                "Next decision",
                "FIRST DECISION / CONFIRM THE STORY WE WANT TO TELL",
                [89., 449., 784., 8.],
                p.ink,
                Align::Center,
            );
        }
    }
    footer(a, "STUDIO BRIEF / GALLERY SERIES", page, p);
}

// A modular dashboard: clear hierarchy, a strategy scorecard, then a delivery plan.
fn dashboard(a: &mut Art, page: usize, p: Palette) {
    a.rect("Dashboard brand mark", [54., 40., 17., 17.], p.accent);
    label(
        a,
        "Dashboard identifier",
        "MOMENTUM / STRATEGY DECK",
        [85., 43., 681., 10.],
        p,
    );
    a.line(
        "Dashboard footer rule",
        (54., 480.),
        (906., 480.),
        p.secondary,
        0.8,
    );
    match page {
        0 => {
            text(
                a,
                "Cover title",
                "Move the\nwork forward.",
                [53., 126., 538., 59.],
                Face::Sans,
                p,
            );
            text(
                a,
                "Cover proposition",
                "A shared direction. A focused plan.\nA better way to turn ideas into progress.",
                [57., 293., 511., 19.],
                Face::Sans,
                p,
            );
            label(
                a,
                "Cover quarter",
                "PROJECT STRATEGY / 2027",
                [58., 404., 499., 10.],
                p,
            );
            a.rect("Momentum cover tile", [616., 101., 290., 340.], p.surface);
            a.rect("Momentum step one", [651., 271., 55., 76.], p.secondary);
            a.rect("Momentum step two", [728., 218., 55., 129.], p.accent);
            a.rect("Momentum step three", [805., 158., 55., 189.], p.ink);
            a.line(
                "Momentum step baseline",
                (647., 359.),
                (872., 359.),
                p.ink,
                1.,
            );
            text(
                a,
                "Momentum tile caption",
                "Three moves.\nOne shared ambition.",
                [647., 384., 234., 19.],
                Face::Sans,
                p,
            );
            label(
                a,
                "Momentum tile label",
                "ALIGN / FOCUS / DELIVER",
                [646., 125., 241., 8.],
                p,
            );
        }
        1 => {
            text(
                a,
                "Overview title",
                "One strategy. Three priorities.",
                [53., 97., 858., 40.],
                Face::Sans,
                p,
            );
            text(
                a,
                "Overview introduction",
                "Concentrate effort where it creates the most useful progress.",
                [57., 162., 842., 18.],
                Face::Sans,
                p,
            );
            for (i, (num, title, body, measure)) in [
                (
                    "01",
                    "Align the team",
                    "Agree on the problem,\nthe audience and the goal.",
                    "OUTPUT / SHARED BRIEF",
                ),
                (
                    "02",
                    "Focus the work",
                    "Choose the smallest set\nof high-value priorities.",
                    "OUTPUT / PRIORITY MAP",
                ),
                (
                    "03",
                    "Deliver & learn",
                    "Ship a useful first version.\nUse feedback to improve it.",
                    "OUTPUT / TESTED RELEASE",
                ),
            ]
            .into_iter()
            .enumerate()
            {
                let x = 54. + i as f64 * 290.;
                a.rect("Priority dashboard tile", [x, 222., 272., 222.], p.surface);
                a.rect(
                    "Priority accent",
                    [x, 222., 272., 5.],
                    if i == 1 { p.accent } else { p.secondary },
                );
                text(
                    a,
                    "Priority number",
                    num,
                    [x + 21., 244., 220., 41.],
                    Face::Sans,
                    p,
                );
                text(
                    a,
                    "Priority title",
                    title,
                    [x + 21., 310., 239., 23.],
                    Face::Sans,
                    p,
                );
                text(
                    a,
                    "Priority detail",
                    body,
                    [x + 21., 351., 238., 15.5],
                    Face::Sans,
                    p,
                );
                label(a, "Priority output", measure, [x + 21., 419., 239., 7.5], p);
            }
        }
        _ => {
            text(
                a,
                "Next steps title",
                "A plan for the next six weeks.",
                [53., 97., 858., 39.],
                Face::Sans,
                p,
            );
            text(
                a,
                "Next steps introduction",
                "Give each stage an owner, a clear output and a decision point.",
                [57., 161., 842., 18.],
                Face::Sans,
                p,
            );
            a.rect("Plan table header", [54., 217., 852., 37.], p.surface);
            for (copy, x, w) in [
                ("STAGE", 70., 170.),
                ("OWNER", 326., 145.),
                ("TIMING", 528., 130.),
                ("DELIVERABLE", 690., 202.),
            ] {
                label(a, "Plan column heading", copy, [x, 230., w, 8.], p);
            }
            for (i, (stage, owner, timing, output)) in [
                ("01  Align", "Project lead", "Weeks 1–2", "Approved brief"),
                ("02  Build", "Core team", "Weeks 3–4", "Working prototype"),
                (
                    "03  Learn",
                    "Team + users",
                    "Weeks 5–6",
                    "Tested next release",
                ),
            ]
            .into_iter()
            .enumerate()
            {
                let y = 278. + i as f64 * 55.;
                text(a, "Plan stage", stage, [70., y, 231., 21.], Face::Sans, p);
                text(
                    a,
                    "Plan owner",
                    owner,
                    [326., y + 3., 176., 16.],
                    Face::Sans,
                    p,
                );
                text(
                    a,
                    "Plan timing",
                    timing,
                    [528., y + 3., 148., 16.],
                    Face::Sans,
                    p,
                );
                text(
                    a,
                    "Plan deliverable",
                    output,
                    [690., y + 3., 203., 16.],
                    Face::Sans,
                    p,
                );
                a.line(
                    "Plan row divider",
                    (54., y + 39.),
                    (906., y + 39.),
                    p.secondary,
                    0.7,
                );
            }
            a.rect("Decision marker", [56., 449., 7., 7.], p.accent);
            label(
                a,
                "Decision request",
                "DECIDE TODAY / CONFIRM THE GOAL AND NAME THE PROJECT LEAD",
                [78., 449., 817., 8.],
                p,
            );
        }
    }
    footer(a, "MOMENTUM DECK", page, p);
}

// A fixed navigation rail gives every page a clear place in the project story.
fn rail(a: &mut Art, page: usize, p: Palette) {
    a.rect("Navigation rail", [31., 31., 182., 449.], p.surface);
    a.rect("Rail brand mark", [55., 54., 19., 19.], p.accent);
    label(a, "Rail identity", "MOMENTUM", [55., 94., 139., 10.], p);
    text(
        a,
        "Rail page numeral",
        &format!("0{}", page + 1),
        [52., 141., 140., 74.],
        Face::Sans,
        p,
    );
    label(
        a,
        "Rail page name",
        page_name(page),
        [55., 240., 141., 9.],
        p,
    );
    a.line(
        "Rail progress track",
        (58., 305.),
        (58., 404.),
        p.secondary,
        1.,
    );
    for i in 0..3 {
        let y = 305. + i as f64 * 49.5;
        a.ellipse(
            "Rail progress stop",
            [54., y - 4., 8., 8.],
            if i == page { p.accent } else { p.secondary },
        );
        text(
            a,
            "Rail navigation item",
            ["Direction", "Priorities", "Action"][i],
            [75., y - 8., 122., 13.],
            Face::Sans,
            p,
        );
    }
    label(
        a,
        "Rail edition",
        "STRATEGY / 2027",
        [55., 445., 139., 7.5],
        p,
    );
    match page {
        0 => {
            label(
                a,
                "Cover category",
                "A SHARED PLAN FOR WHAT COMES NEXT",
                [257., 66., 647., 9.],
                p,
            );
            text(
                a,
                "Cover title",
                "Build the\nnext chapter.",
                [253., 134., 651., 69.],
                Face::Sans,
                p,
            );
            text(
                a,
                "Cover proposition",
                "Choose a direction. Connect the work.\nCreate momentum together.",
                [259., 318., 626., 22.],
                Face::Sans,
                p,
            );
            a.line(
                "Forward motion shaft",
                (261., 428.),
                (884., 428.),
                p.accent,
                3.,
            );
            a.polygon(
                "Forward motion arrow",
                &[(871., 415.), (889., 428.), (871., 441.)],
                p.accent,
            );
        }
        1 => {
            text(
                a,
                "Overview title",
                "Put the right\nwork in motion.",
                [254., 63., 650., 48.],
                Face::Sans,
                p,
            );
            for (i, (title, body, tag)) in [
                (
                    "Align around a real need",
                    "Start with the user problem and define a useful outcome.",
                    "DIRECTION",
                ),
                (
                    "Choose a focused scope",
                    "Prioritize the work that can make the biggest difference.",
                    "FOCUS",
                ),
                (
                    "Create a learning rhythm",
                    "Share progress early, gather evidence and adjust together.",
                    "DELIVERY",
                ),
            ]
            .into_iter()
            .enumerate()
            {
                let y = 213. + i as f64 * 80.;
                a.rect(
                    "Priority lane marker",
                    [258., y + 5., 31., 31.],
                    if i == 1 { p.accent } else { p.secondary },
                );
                text(
                    a,
                    "Priority lane heading",
                    title,
                    [306., y, 599., 23.],
                    Face::Sans,
                    p,
                );
                text(
                    a,
                    "Priority lane detail",
                    body,
                    [308., y + 35., 593., 15.],
                    Face::Sans,
                    p,
                );
                label(
                    a,
                    "Priority lane category",
                    tag,
                    [307., y + 61., 590., 7.],
                    p,
                );
                if i < 2 {
                    a.line(
                        "Priority lane divider",
                        (257., y + 73.),
                        (904., y + 73.),
                        p.secondary,
                        0.7,
                    );
                }
            }
        }
        _ => {
            text(
                a,
                "Next steps title",
                "Clear steps.\nShared ownership.",
                [254., 63., 650., 46.],
                Face::Sans,
                p,
            );
            for (i, (time, action, detail)) in [
                (
                    "NOW",
                    "Set the direction",
                    "Project lead / confirm the goal and decision makers.",
                ),
                (
                    "NEXT",
                    "Build the first version",
                    "Core team / turn the priority into a working prototype.",
                ),
                (
                    "THEN",
                    "Test, learn and improve",
                    "Team + users / review the evidence and agree the next move.",
                ),
            ]
            .into_iter()
            .enumerate()
            {
                let y = 220. + i as f64 * 77.;
                label(a, "Action timing", time, [258., y + 6., 89., 10.], p);
                a.line(
                    "Action sequence connector",
                    (354., y + 14.),
                    (378., y + 14.),
                    p.accent,
                    2.,
                );
                text(
                    a,
                    "Action heading",
                    action,
                    [398., y, 506., 23.],
                    Face::Sans,
                    p,
                );
                text(
                    a,
                    "Action ownership",
                    detail,
                    [400., y + 34., 502., 14.],
                    Face::Sans,
                    p,
                );
                if i < 2 {
                    a.line(
                        "Action divider",
                        (257., y + 62.),
                        (904., y + 62.),
                        p.secondary,
                        0.7,
                    );
                }
            }
            label(
                a,
                "Action decision",
                "TODAY’S DECISION / AGREE THE FIRST PRIORITY",
                [259., 461., 644., 8.],
                p,
            );
        }
    }
    footer(a, "MOMENTUM DECK / PROJECT SERIES", page, p);
}

// A restrained technical grid: a systems diagram becomes a roadmap and work plan.
fn blueprint(a: &mut Art, page: usize, p: Palette) {
    for x in (48..=912).step_by(48) {
        a.line(
            "Blueprint grid vertical",
            (x as f64, 34.),
            (x as f64, 478.),
            p.surface,
            0.55,
        );
    }
    for y in (46..=478).step_by(48) {
        a.line(
            "Blueprint grid horizontal",
            (48., y as f64),
            (912., y as f64),
            p.surface,
            0.55,
        );
    }
    for (x, y) in [(48., 34.), (912., 34.), (48., 478.), (912., 478.)] {
        a.line(
            "Registration mark horizontal",
            (x - 6., y),
            (x + 6., y),
            p.secondary,
            0.8,
        );
        a.line(
            "Registration mark vertical",
            (x, y - 6.),
            (x, y + 6.),
            p.secondary,
            0.8,
        );
    }
    label(
        a,
        "Blueprint masthead",
        "MOMENTUM / A BLUEPRINT FOR PROGRESS",
        [70., 57., 824., 9.],
        p,
    );
    match page {
        0 => {
            text(
                a,
                "Cover title",
                "Good strategy.\nBuilt to work.",
                [69., 126., 541., 62.],
                Face::Sans,
                p,
            );
            text(
                a,
                "Cover proposition",
                "Connect the ambition to the everyday.\nTurn a clear idea into a useful system.",
                [74., 299., 510., 19.],
                Face::Sans,
                p,
            );
            label(
                a,
                "Cover edition",
                "DIRECTION / EXECUTION / LEARNING",
                [75., 423., 515., 9.],
                p,
            );
            a.outline("System boundary", [657., 133., 223., 283.], p.ink, 1.);
            for (i, (copy, x, y, w)) in [
                ("FOCUS", 679., 158., 178.),
                ("BUILD", 695., 244., 146.),
                ("LEARN", 711., 330., 114.),
            ]
            .into_iter()
            .enumerate()
            {
                a.rect(
                    "System module",
                    [x, y, w, 59.],
                    if i == 1 { p.secondary } else { p.surface },
                );
                a.outline("System module outline", [x, y, w, 59.], p.accent, 1.);
                a.caps(
                    "System module label",
                    copy,
                    [x + 8., y + 23., w - 16., 10.],
                    p.ink,
                    Align::Center,
                );
                if i < 2 {
                    a.line(
                        "System connection",
                        (768., y + 59.),
                        (768., y + 82.),
                        p.accent,
                        1.5,
                    );
                    a.polygon(
                        "System connection arrow",
                        &[(764., y + 77.), (768., y + 83.), (772., y + 77.)],
                        p.accent,
                    );
                }
            }
            label(
                a,
                "System figure label",
                "FIG. 01 / THE OPERATING LOOP",
                [654., 443., 244., 7.5],
                p,
            );
        }
        1 => {
            text(
                a,
                "Overview title",
                "A system for moving forward.",
                [69., 100., 824., 40.],
                Face::Sans,
                p,
            );
            text(
                a,
                "Overview introduction",
                "Keep the connection between evidence, choices and delivery visible.",
                [74., 165., 816., 17.],
                Face::Sans,
                p,
            );
            for (i, (num, title, body, output)) in [
                (
                    "01 / INPUT",
                    "Evidence",
                    "Understand the need.\nFind the real constraint.",
                    "A CLEAR PROBLEM",
                ),
                (
                    "02 / CHOICE",
                    "Focus",
                    "Set a useful goal.\nChoose what matters most.",
                    "A SHARED PRIORITY",
                ),
                (
                    "03 / OUTPUT",
                    "Delivery",
                    "Build a small first version.\nTest it with real users.",
                    "A LEARNING CYCLE",
                ),
            ]
            .into_iter()
            .enumerate()
            {
                let x = 72. + i as f64 * 286.;
                a.rect(
                    "Blueprint process module",
                    [x, 234., 241., 196.],
                    p.background,
                );
                a.outline(
                    "Blueprint process boundary",
                    [x, 234., 241., 196.],
                    p.ink,
                    0.9,
                );
                label(a, "Process stage", num, [x + 16., 252., 211., 8.], p);
                text(
                    a,
                    "Process title",
                    title,
                    [x + 16., 289., 211., 28.],
                    Face::Sans,
                    p,
                );
                text(
                    a,
                    "Process body",
                    body,
                    [x + 16., 339., 214., 15.],
                    Face::Sans,
                    p,
                );
                label(a, "Process output", output, [x + 16., 407., 212., 7.5], p);
                if i < 2 {
                    a.line(
                        "Process connection",
                        (x + 247., 325.),
                        (x + 277., 325.),
                        p.accent,
                        1.7,
                    );
                    a.polygon(
                        "Process arrow",
                        &[(x + 272., 320.), (x + 278., 325.), (x + 272., 330.)],
                        p.accent,
                    );
                }
            }
            label(
                a,
                "Process feedback note",
                "FEEDBACK RETURNS TO THE BRIEF / THE SYSTEM GETS SMARTER WITH USE",
                [75., 457., 814., 8.],
                p,
            );
        }
        _ => {
            text(
                a,
                "Next steps title",
                "Build, test, then improve.",
                [69., 100., 824., 43.],
                Face::Sans,
                p,
            );
            text(
                a,
                "Next steps introduction",
                "A practical sequence with a clear handoff at every stage.",
                [74., 166., 817., 17.],
                Face::Sans,
                p,
            );
            a.rect(
                "Blueprint work plan ground",
                [71., 220., 814., 209.],
                p.background,
            );
            a.outline(
                "Blueprint work plan boundary",
                [71., 220., 814., 209.],
                p.ink,
                0.9,
            );
            a.line(
                "Blueprint time column",
                (191., 220.),
                (191., 429.),
                p.secondary,
                0.8,
            );
            a.line(
                "Blueprint result column",
                (630., 220.),
                (630., 429.),
                p.secondary,
                0.8,
            );
            for (i, (time, title, detail, result)) in [
                (
                    "WEEK 01",
                    "Frame the opportunity",
                    "Project lead / agree the problem and success criteria.",
                    "APPROVED BRIEF",
                ),
                (
                    "WEEKS 02–03",
                    "Make a testable version",
                    "Core team / build the smallest useful expression.",
                    "WORKING PROTOTYPE",
                ),
                (
                    "WEEK 04",
                    "Review the evidence",
                    "Team + users / decide what to refine or expand.",
                    "NEXT ITERATION",
                ),
            ]
            .into_iter()
            .enumerate()
            {
                let y = 237. + i as f64 * 69.;
                label(
                    a,
                    "Blueprint step timing",
                    time,
                    [83., y + 12., 101., 7.],
                    p,
                );
                text(
                    a,
                    "Blueprint step action",
                    title,
                    [209., y, 405., 20.],
                    Face::Sans,
                    p,
                );
                text(
                    a,
                    "Blueprint step ownership",
                    detail,
                    [210., y + 31., 403., 11.5],
                    Face::Sans,
                    p,
                );
                a.rect(
                    "Blueprint deliverable indicator",
                    [648., y + 10., 6., 6.],
                    p.accent,
                );
                label(
                    a,
                    "Blueprint deliverable",
                    result,
                    [666., y + 11., 203., 7.5],
                    p,
                );
                if i < 2 {
                    a.line(
                        "Blueprint work plan divider",
                        (71., y + 52.),
                        (885., y + 52.),
                        p.secondary,
                        0.8,
                    );
                }
            }
            label(
                a,
                "Blueprint next decision",
                "DECISION 01 / CONFIRM THE BRIEF BEFORE MOVING INTO PRODUCTION",
                [75., 457., 811., 8.],
                p,
            );
        }
    }
    footer(a, "MOMENTUM DECK / BLUEPRINT SERIES", page, p);
}
