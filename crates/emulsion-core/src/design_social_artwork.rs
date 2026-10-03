//! Original, editable carousel layouts on a 500 × 500 art-direction grid.
//! Each composition has a distinct cover, story, and call-to-action page.
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
        return Err("Social carousels have three pages".into());
    }
    let p = *selection.palette()?;
    let mut a = Art::with_grid(width, height, (500., 500.), p.background)?;
    match (selection.family, selection.variant) {
        (FamilyId::FieldNotes, VariantId::FieldJournal) => field_journal(&mut a, page, p),
        (FamilyId::FieldNotes, VariantId::FieldSidebar) => field_sidebar(&mut a, page, p),
        (FamilyId::FieldNotes, VariantId::FieldDiptych) => field_diptych(&mut a, page, p),
        (FamilyId::SignalStudio, VariantId::SignalGrid) => signal_grid(&mut a, page, p),
        (FamilyId::SignalStudio, VariantId::SignalOrbit) => signal_orbit(&mut a, page, p),
        (FamilyId::SignalStudio, VariantId::SignalBlocks) => signal_blocks(&mut a, page, p),
        _ => return Err("This selection is not a social carousel layout".into()),
    }
    a.finish()
}

fn page_number(a: &mut Art, page: usize, x: f64, y: f64, color: [u8; 4]) {
    let label = ["01 / 03", "02 / 03", "03 / 03"][page];
    a.text(
        "Carousel page number",
        label,
        [x, y, 64., 9.],
        Face::Sans,
        color,
        Align::Right,
    );
}

fn field_journal(a: &mut Art, page: usize, p: Palette) {
    use Align::{Center, Left, Right};
    a.rect("Journal paper", [24., 24., 452., 452.], p.surface);
    a.outline(
        "Journal hairline frame",
        [34., 34., 432., 432.],
        p.secondary,
        0.7,
    );
    a.caps(
        "Journal masthead",
        "FIELD NOTES",
        [50., 50., 250., 10.],
        p.ink,
        Left,
    );
    a.text(
        "Journal edition",
        "VOL. 01",
        [358., 52., 92., 9.],
        Face::Sans,
        p.accent,
        Right,
    );
    a.line("Masthead rule", (50., 78.), (450., 78.), p.secondary, 0.8);
    match page {
        0 => {
            a.text(
                "Cover headline",
                "Make room\nfor slow.",
                [48., 115., 394., 55.],
                Face::Serif,
                p.ink,
                Left,
            );
            a.text(
                "Cover introduction",
                "Small rituals. A lighter day.",
                [51., 256., 318., 16.],
                Face::Serif,
                p.ink,
                Left,
            );
            a.sprig(391., 405., 1.23, -1., p.accent);
            a.line(
                "Specimen baseline",
                (351., 410.),
                (429., 410.),
                p.secondary,
                0.8,
            );
            a.text(
                "Specimen annotation",
                "a little room to grow",
                [305., 421., 138., 10.],
                Face::Italic,
                p.accent,
                Center,
            );
            a.caps(
                "Cover topic",
                "EVERYDAY RITUALS",
                [51., 355., 264., 8.5],
                p.accent,
                Left,
            );
            a.text(
                "Cover editorial note",
                "For the things\nthat make us feel at home.",
                [50., 378., 254., 17.],
                Face::Serif,
                p.ink,
                Left,
            );
        }
        1 => {
            a.text(
                "Story headline",
                "The art of\npaying attention",
                [48., 98., 400., 38.],
                Face::Serif,
                p.ink,
                Left,
            );
            a.caps(
                "First ritual number",
                "01",
                [51., 212., 30., 9.],
                p.accent,
                Left,
            );
            a.text(
                "First ritual title",
                "Open the window",
                [94., 204., 346., 23.],
                Face::Serif,
                p.ink,
                Left,
            );
            a.text(
                "First ritual detail",
                "Let the morning arrive before the day.",
                [95., 239., 345., 11.5],
                Face::Sans,
                p.ink,
                Left,
            );
            a.line(
                "First ritual divider",
                (51., 269.),
                (449., 269.),
                p.secondary,
                0.7,
            );
            a.caps(
                "Second ritual number",
                "02",
                [51., 291., 30., 9.],
                p.accent,
                Left,
            );
            a.text(
                "Second ritual title",
                "Make something by hand",
                [94., 283., 346., 23.],
                Face::Serif,
                p.ink,
                Left,
            );
            a.text(
                "Second ritual detail",
                "A cup of tea. A sketch. A little good food.",
                [95., 318., 345., 11.5],
                Face::Sans,
                p.ink,
                Left,
            );
            a.line(
                "Second ritual divider",
                (51., 348.),
                (449., 348.),
                p.secondary,
                0.7,
            );
            a.caps(
                "Third ritual number",
                "03",
                [51., 371., 30., 9.],
                p.accent,
                Left,
            );
            a.text(
                "Third ritual title",
                "Leave a little space",
                [94., 363., 346., 23.],
                Face::Serif,
                p.ink,
                Left,
            );
            a.text(
                "Third ritual detail",
                "Keep one moment of the day unplanned.",
                [95., 398., 345., 11.5],
                Face::Sans,
                p.ink,
                Left,
            );
        }
        _ => {
            a.text(
                "Call to action headline",
                "Begin with\none small thing.",
                [48., 118., 394., 48.],
                Face::Serif,
                p.ink,
                Left,
            );
            a.text(
                "Call to action detail",
                "Pick one ritual.\nGive it ten unhurried minutes.",
                [51., 253., 298., 17.],
                Face::Serif,
                p.ink,
                Left,
            );
            a.rect("Save prompt panel", [50., 340., 294., 50.], p.accent);
            a.caps(
                "Save prompt",
                "SAVE FOR A SLOWER MORNING",
                [67., 359., 260., 8.],
                p.surface,
                Center,
            );
            a.sprig(398., 394., 0.72, -1., p.accent);
            a.text(
                "Closing note",
                "A gentler pace is a practice.",
                [51., 413., 309., 13.],
                Face::Italic,
                p.accent,
                Left,
            );
        }
    }
    a.text(
        "Journal account",
        "@fieldnotes",
        [51., 445., 286., 9.],
        Face::Sans,
        p.ink,
        Left,
    );
    page_number(a, page, 385., 445., p.ink);
}

fn field_sidebar(a: &mut Art, page: usize, p: Palette) {
    use Align::{Center, Left};
    a.rect("Botanical sidebar", [0., 0., 118., 500.], p.accent);
    a.line("Sidebar stem one", (46., 167.), (69., 30.), p.surface, 0.9);
    a.sprig(52., 287., 0.89, 1., p.surface);
    a.sprig(76., 465., 0.87, -1., p.secondary);
    a.leaf(
        "Sidebar top leaf one",
        (59., 94.),
        (28., 56.),
        8.,
        p.surface,
    );
    a.leaf(
        "Sidebar top leaf two",
        (62., 76.),
        (91., 42.),
        8.,
        p.secondary,
    );
    a.caps(
        "Sidebar masthead",
        "FIELD NOTES",
        [153., 43., 290., 10.],
        p.ink,
        Left,
    );
    a.line(
        "Sidebar masthead rule",
        (153., 71.),
        (456., 71.),
        p.secondary,
        0.8,
    );
    match page {
        0 => {
            a.caps(
                "Cover issue label",
                "SMALL RITUALS / NO. 01",
                [154., 101., 300., 8.],
                p.accent,
                Left,
            );
            a.text(
                "Cover headline",
                "A slower\nkind of day",
                [150., 145., 306., 48.],
                Face::Serif,
                p.ink,
                Left,
            );
            a.text(
                "Cover introduction",
                "Good things grow\nin the space we leave.",
                [154., 277., 294., 19.],
                Face::Serif,
                p.ink,
                Left,
            );
            a.line(
                "Cover reflection rule",
                (154., 340.),
                (195., 340.),
                p.accent,
                1.,
            );
            a.text(
                "Cover reflection",
                "Notice more.\nNeed less.",
                [152., 358., 298., 28.],
                Face::Italic,
                p.accent,
                Left,
            );
        }
        1 => {
            a.text(
                "Story headline",
                "Three ways\nto begin.",
                [151., 102., 302., 40.],
                Face::Serif,
                p.ink,
                Left,
            );
            a.caps(
                "First ritual title",
                "01 / STEP OUTSIDE",
                [154., 223., 295., 9.],
                p.accent,
                Left,
            );
            a.text(
                "First ritual detail",
                "Find a little light.\nLet your eyes wander.",
                [153., 247., 297., 17.],
                Face::Serif,
                p.ink,
                Left,
            );
            a.line(
                "First ritual divider",
                (154., 293.),
                (454., 293.),
                p.secondary,
                0.7,
            );
            a.caps(
                "Second ritual title",
                "02 / MAKE TIME",
                [154., 309., 295., 9.],
                p.accent,
                Left,
            );
            a.text(
                "Second ritual detail",
                "Do one thing slowly.\nGive it your attention.",
                [153., 333., 297., 17.],
                Face::Serif,
                p.ink,
                Left,
            );
            a.line(
                "Second ritual divider",
                (154., 379.),
                (454., 379.),
                p.secondary,
                0.7,
            );
            a.caps(
                "Third ritual title",
                "03 / KEEP IT SIMPLE",
                [154., 397., 295., 9.],
                p.accent,
                Left,
            );
            a.text(
                "Third ritual detail",
                "Enough can be a lovely thing.",
                [153., 420., 297., 17.],
                Face::Serif,
                p.ink,
                Left,
            );
        }
        _ => {
            a.text(
                "Call to action headline",
                "Keep a\nlittle space.",
                [151., 117., 304., 47.],
                Face::Serif,
                p.ink,
                Left,
            );
            a.text(
                "Call to action detail",
                "For a walk. For a cup of tea.\nFor an idea that takes its time.",
                [154., 252., 294., 16.],
                Face::Serif,
                p.ink,
                Left,
            );
            a.ellipse("Small beginning seal", [155., 332., 82., 82.], p.accent);
            a.text(
                "Small beginning seal text",
                "START\nSMALL",
                [170., 359., 52., 10.],
                Face::Sans,
                p.surface,
                Center,
            );
            a.text(
                "Closing invitation",
                "Your next ritual\nstarts today.",
                [258., 347., 193., 21.],
                Face::Serif,
                p.ink,
                Left,
            );
        }
    }
    a.line(
        "Sidebar footer rule",
        (153., 452.),
        (456., 452.),
        p.secondary,
        0.8,
    );
    a.text(
        "Sidebar account",
        "@fieldnotes",
        [154., 465., 184., 9.],
        Face::Sans,
        p.ink,
        Left,
    );
    page_number(a, page, 391., 465., p.ink);
}

fn field_diptych(a: &mut Art, page: usize, p: Palette) {
    use Align::{Center, Left};
    a.caps(
        "Diptych masthead",
        "FIELD NOTES",
        [38., 37., 315., 10.],
        p.ink,
        Left,
    );
    a.text(
        "Diptych edition",
        "THE SLOW ISSUE",
        [335., 39., 127., 8.],
        Face::Sans,
        p.accent,
        Align::Right,
    );
    match page {
        0 => {
            a.text(
                "Cover headline",
                "Less hurry.\nMore wonder.",
                [36., 82., 426., 46.],
                Face::Serif,
                p.ink,
                Left,
            );
            a.rect("First specimen paper", [38., 211., 204., 166.], p.surface);
            a.rect(
                "Second specimen paper",
                [258., 211., 204., 166.],
                p.secondary,
            );
            a.sprig(129., 355., 0.91, 1., p.accent);
            a.sprig(371., 355., 0.91, -1., p.ink);
            a.line(
                "First specimen footing",
                (108., 355.),
                (168., 355.),
                p.secondary,
                0.75,
            );
            a.line(
                "Second specimen footing",
                (332., 355.),
                (392., 355.),
                p.accent,
                0.75,
            );
            a.caps(
                "First specimen caption",
                "01 / NOTICE",
                [38., 393., 204., 8.5],
                p.ink,
                Center,
            );
            a.caps(
                "Second specimen caption",
                "02 / NURTURE",
                [258., 393., 204., 8.5],
                p.ink,
                Center,
            );
            a.text(
                "Cover introduction",
                "A field guide to everyday calm.",
                [38., 425., 424., 15.],
                Face::Serif,
                p.ink,
                Center,
            );
        }
        1 => {
            a.text(
                "Story headline",
                "Two rituals.\nOne gentler rhythm.",
                [36., 82., 426., 39.],
                Face::Serif,
                p.ink,
                Left,
            );
            a.rect("Outside ritual panel", [38., 195., 204., 217.], p.surface);
            a.rect("Space ritual panel", [258., 195., 204., 217.], p.secondary);
            a.text(
                "Outside ritual number",
                "01",
                [54., 213., 40., 11.],
                Face::Sans,
                p.accent,
                Left,
            );
            a.text(
                "Space ritual number",
                "02",
                [274., 213., 40., 11.],
                Face::Sans,
                p.ink,
                Left,
            );
            a.leaf(
                "Outside ritual leaf",
                (147., 276.),
                (179., 219.),
                17.,
                p.accent,
            );
            a.line(
                "Outside ritual stem",
                (142., 284.),
                (166., 241.),
                p.ink,
                0.8,
            );
            a.leaf(
                "Space ritual leaf one",
                (355., 277.),
                (339., 230.),
                10.,
                p.ink,
            );
            a.leaf(
                "Space ritual leaf two",
                (355., 277.),
                (384., 240.),
                11.,
                p.accent,
            );
            a.text(
                "Outside ritual title",
                "Go outside",
                [54., 299., 170., 23.],
                Face::Serif,
                p.ink,
                Left,
            );
            a.text(
                "Space ritual title",
                "Make space",
                [274., 299., 170., 23.],
                Face::Serif,
                p.ink,
                Left,
            );
            a.text(
                "Outside ritual detail",
                "Follow the light.\nLeave your phone\nin your pocket.",
                [55., 341., 166., 13.],
                Face::Sans,
                p.ink,
                Left,
            );
            a.text(
                "Space ritual detail",
                "Clear one corner.\nKeep only what\nfeels useful.",
                [275., 341., 166., 13.],
                Face::Sans,
                p.ink,
                Left,
            );
            a.text(
                "Story closing note",
                "Small changes make room for more.",
                [38., 428., 424., 14.],
                Face::Serif,
                p.ink,
                Center,
            );
        }
        _ => {
            a.text(
                "Call to action headline",
                "Choose your\nsmall beginning.",
                [36., 86., 426., 45.],
                Face::Serif,
                p.ink,
                Left,
            );
            a.rect("Walk action panel", [38., 224., 204., 151.], p.surface);
            a.rect("Make action panel", [258., 224., 204., 151.], p.secondary);
            a.text(
                "Walk action number",
                "01",
                [54., 244., 170., 46.],
                Face::Serif,
                p.accent,
                Left,
            );
            a.text(
                "Make action number",
                "02",
                [274., 244., 170., 46.],
                Face::Serif,
                p.ink,
                Left,
            );
            a.line(
                "Walk action rule",
                (56., 308.),
                (222., 308.),
                p.secondary,
                0.8,
            );
            a.line(
                "Make action rule",
                (276., 308.),
                (442., 308.),
                p.accent,
                0.8,
            );
            a.caps(
                "Walk action label",
                "TAKE A WALK",
                [56., 333., 166., 8.5],
                p.ink,
                Left,
            );
            a.caps(
                "Make action label",
                "MAKE SOMETHING",
                [276., 333., 166., 8.5],
                p.ink,
                Left,
            );
            a.text(
                "Save invitation",
                "Save this guide.\nCome back when you need it.",
                [38., 403., 424., 16.],
                Face::Serif,
                p.ink,
                Center,
            );
        }
    }
    a.line(
        "Diptych footer rule",
        (38., 456.),
        (462., 456.),
        p.secondary,
        0.7,
    );
    a.text(
        "Diptych account",
        "@fieldnotes",
        [38., 469., 300., 9.],
        Face::Sans,
        p.ink,
        Left,
    );
    page_number(a, page, 398., 469., p.ink);
}

fn signal_header(a: &mut Art, p: Palette, edition: &str) {
    a.caps(
        "Studio masthead",
        "SIGNAL STUDIO",
        [38., 37., 293., 10.],
        p.ink,
        Align::Left,
    );
    a.text(
        "Studio series label",
        edition,
        [338., 39., 124., 8.],
        Face::Sans,
        p.ink,
        Align::Right,
    );
}

fn signal_footer(a: &mut Art, page: usize, p: Palette) {
    a.line("Studio footer rule", (38., 447.), (462., 447.), p.ink, 0.7);
    a.text(
        "Studio account",
        "@signalstudio",
        [38., 463., 290., 9.],
        Face::Sans,
        p.ink,
        Align::Left,
    );
    page_number(a, page, 398., 463., p.ink);
}

fn right_arrow(a: &mut Art, x: f64, y: f64, scale: f64, color: [u8; 4]) {
    a.polygon(
        "Forward arrow",
        &[
            (x, y + 12. * scale),
            (x + 34. * scale, y + 12. * scale),
            (x + 34. * scale, y),
            (x + 56. * scale, y + 22. * scale),
            (x + 34. * scale, y + 44. * scale),
            (x + 34. * scale, y + 32. * scale),
            (x, y + 32. * scale),
        ],
        color,
    );
}

fn signal_grid(a: &mut Art, page: usize, p: Palette) {
    use Align::{Center, Left};
    signal_header(a, p, "IDEAS IN MOTION");
    match page {
        0 => {
            a.text(
                "Cover headline first line",
                "Ideas",
                [35., 94., 426., 79.],
                Face::Display,
                p.ink,
                Left,
            );
            a.text(
                "Cover headline second line",
                "in motion.",
                [35., 183., 426., 63.],
                Face::Display,
                p.ink,
                Left,
            );
            a.rect("Circle module", [38., 305., 96., 96.], p.accent);
            a.ellipse("Circle module shape", [55., 322., 62., 62.], p.surface);
            a.rect("Window module", [147., 305., 96., 96.], p.secondary);
            a.rect("Window module shape", [166., 324., 58., 58.], p.ink);
            a.rect("Star module", [256., 305., 96., 96.], p.ink);
            a.star("Star module shape", 304., 353., 36., 8, p.surface);
            a.rect("Stripe module", [365., 305., 97., 96.], p.surface);
            a.rect("Stripe one", [380., 318., 12., 70.], p.accent);
            a.rect("Stripe two", [408., 318., 12., 70.], p.accent);
            a.rect("Stripe three", [436., 318., 12., 70.], p.accent);
            a.caps(
                "Circle module label",
                "THINK",
                [38., 417., 96., 8.],
                p.ink,
                Center,
            );
            a.caps(
                "Window module label",
                "SHAPE",
                [147., 417., 96., 8.],
                p.ink,
                Center,
            );
            a.caps(
                "Star module label",
                "MOVE",
                [256., 417., 96., 8.],
                p.ink,
                Center,
            );
            a.caps(
                "Stripe module label",
                "REPEAT",
                [365., 417., 97., 8.],
                p.ink,
                Center,
            );
        }
        1 => {
            a.text(
                "Story headline",
                "Good work\nstarts here.",
                [36., 96., 426., 48.],
                Face::Display,
                p.ink,
                Left,
            );
            a.rect("Strategy row", [38., 225., 424., 57.], p.surface);
            a.rect("Identity row", [38., 298., 424., 57.], p.secondary);
            a.rect("Expression row", [38., 371., 424., 57.], p.surface);
            a.text(
                "Strategy number",
                "01",
                [52., 246., 26., 11.],
                Face::Sans,
                p.accent,
                Left,
            );
            a.text(
                "Strategy title",
                "Strategy",
                [92., 241., 130., 19.],
                Face::Sans,
                p.ink,
                Left,
            );
            a.text(
                "Strategy detail",
                "Find the idea.\nMake it matter.",
                [245., 239., 196., 12.],
                Face::Sans,
                p.ink,
                Left,
            );
            a.text(
                "Identity number",
                "02",
                [52., 319., 26., 11.],
                Face::Sans,
                p.ink,
                Left,
            );
            a.text(
                "Identity title",
                "Identity",
                [92., 314., 130., 19.],
                Face::Sans,
                p.ink,
                Left,
            );
            a.text(
                "Identity detail",
                "Build a world\nthat feels like you.",
                [245., 312., 196., 12.],
                Face::Sans,
                p.ink,
                Left,
            );
            a.text(
                "Expression number",
                "03",
                [52., 392., 26., 11.],
                Face::Sans,
                p.accent,
                Left,
            );
            a.text(
                "Expression title",
                "Expression",
                [92., 387., 137., 19.],
                Face::Sans,
                p.ink,
                Left,
            );
            a.text(
                "Expression detail",
                "Bring it to life.\nKeep it moving.",
                [245., 385., 196., 12.],
                Face::Sans,
                p.ink,
                Left,
            );
        }
        _ => {
            a.text(
                "Call to action headline",
                "Make your\nnext move.",
                [35., 102., 426., 53.],
                Face::Display,
                p.ink,
                Left,
            );
            a.rect("Contact action panel", [38., 271., 294., 57.], p.accent);
            a.caps(
                "Contact action label",
                "LET'S MAKE SOMETHING",
                [55., 293., 260., 8.5],
                p.surface,
                Center,
            );
            a.rect("Contact arrow panel", [349., 271., 113., 57.], p.secondary);
            right_arrow(a, 381., 281., 0.84, p.ink);
            a.text(
                "Call to action introduction",
                "Fresh thinking for your next chapter.",
                [38., 358., 424., 17.],
                Face::Sans,
                p.ink,
                Left,
            );
            a.ellipse("Contact circle motif", [38., 400., 24., 24.], p.accent);
            a.rect("Contact square motif", [80., 400., 24., 24.], p.ink);
            a.star("Contact star motif", 134., 412., 15., 8, p.accent);
            a.line(
                "Contact motif extension",
                (165., 412.),
                (461., 412.),
                p.secondary,
                1.2,
            );
        }
    }
    signal_footer(a, page, p);
}

fn signal_orbit(a: &mut Art, page: usize, p: Palette) {
    use Align::{Center, Left};
    signal_header(a, p, "THE NEXT CHAPTER");
    match page {
        0 => {
            a.ellipse("Outer orbit", [63., 79., 374., 352.], p.secondary);
            a.ellipse("Outer orbit counter", [78., 94., 344., 322.], p.background);
            a.ellipse("Inner orbit", [96., 109., 308., 292.], p.accent);
            a.ellipse(
                "Inner orbit counter",
                [97.5, 110.5, 305., 289.],
                p.background,
            );
            a.ellipse("Upper orbit satellite", [350., 90., 41., 41.], p.accent);
            a.ellipse("Lower orbit satellite", [64., 333., 30., 30.], p.ink);
            a.star("Orbit spark", 414., 282., 16., 8, p.accent);
            a.caps(
                "Cover orbit label",
                "GOOD IDEAS GO PLACES",
                [110., 145., 280., 8.],
                p.ink,
                Center,
            );
            a.text(
                "Cover headline",
                "Ideas in\nmotion.",
                [92., 190., 316., 52.],
                Face::Display,
                p.ink,
                Center,
            );
            a.text(
                "Cover introduction",
                "A creative studio for what's next.",
                [99., 324., 302., 12.5],
                Face::Sans,
                p.ink,
                Center,
            );
        }
        1 => {
            a.text(
                "Story headline",
                "From spark\nto signal.",
                [35., 94., 427., 47.],
                Face::Display,
                p.ink,
                Left,
            );
            a.ellipse("Story orbit", [297., 260., 165., 165.], p.secondary);
            a.ellipse(
                "Story orbit counter",
                [311., 274., 137., 137.],
                p.background,
            );
            a.ellipse("Story inner orbit", [329., 292., 101., 101.], p.accent);
            a.ellipse("Story inner counter", [331., 294., 97., 97.], p.background);
            a.star("Story central spark", 379.5, 342.5, 31., 8, p.ink);
            a.ellipse("Story orbit satellite", [422., 273., 24., 24.], p.accent);
            a.caps(
                "First orbit step number",
                "01",
                [39., 249., 31., 8.],
                p.accent,
                Left,
            );
            a.text(
                "First orbit step title",
                "Find your point of view",
                [39., 270., 238., 16.],
                Face::Sans,
                p.ink,
                Left,
            );
            a.text(
                "First orbit step detail",
                "Every good story starts with one.",
                [39., 298., 238., 10.5],
                Face::Sans,
                p.ink,
                Left,
            );
            a.caps(
                "Second orbit step number",
                "02",
                [39., 330., 31., 8.],
                p.accent,
                Left,
            );
            a.text(
                "Second orbit step title",
                "Give it a world",
                [39., 351., 238., 16.],
                Face::Sans,
                p.ink,
                Left,
            );
            a.text(
                "Second orbit step detail",
                "A voice, a shape, a way to move.",
                [39., 379., 238., 10.5],
                Face::Sans,
                p.ink,
                Left,
            );
            a.caps(
                "Story closing label",
                "THEN PUT IT INTO MOTION",
                [39., 413., 246., 7.5],
                p.ink,
                Left,
            );
        }
        _ => {
            a.ellipse("Contact orbit outline", [70., 77., 360., 360.], p.secondary);
            a.ellipse(
                "Contact orbit outline counter",
                [73., 80., 354., 354.],
                p.background,
            );
            a.ellipse("Contact orbit center", [83., 90., 334., 334.], p.accent);
            a.ellipse("Contact orbit satellite", [391., 127., 33., 33.], p.ink);
            a.ellipse(
                "Contact orbit small satellite",
                [59., 306., 21., 21.],
                p.accent,
            );
            a.caps(
                "Contact orbit invitation",
                "READY WHEN YOU ARE",
                [136., 152., 228., 8.],
                p.surface,
                Center,
            );
            a.text(
                "Call to action headline",
                "Let's make\nsome noise.",
                [112., 203., 276., 40.],
                Face::Display,
                p.surface,
                Center,
            );
            a.rect(
                "Contact orbit prompt panel",
                [147., 335., 206., 37.],
                p.surface,
            );
            a.caps(
                "Contact orbit prompt",
                "START A CONVERSATION",
                [155., 350., 190., 7.],
                p.ink,
                Center,
            );
        }
    }
    signal_footer(a, page, p);
}

fn signal_blocks(a: &mut Art, page: usize, p: Palette) {
    use Align::Left;
    signal_header(a, p, "FOR THE BOLD");
    match page {
        0 => {
            a.rect("Make headline block", [28., 96., 319., 103.], p.accent);
            a.text(
                "Cover headline first block",
                "MAKE",
                [48., 114., 277., 66.],
                Face::Display,
                p.surface,
                Left,
            );
            a.ellipse("Cover circle module", [371., 101., 101., 101.], p.secondary);
            a.star("Cover circle spark", 421.5, 151.5, 33., 8, p.ink);
            a.rect("Waves headline block", [143., 216., 329., 103.], p.ink);
            a.text(
                "Cover headline second block",
                "WAVES.",
                [163., 234., 289., 60.],
                Face::Display,
                p.surface,
                Left,
            );
            right_arrow(a, 42., 241., 1.35, p.accent);
            a.text(
                "Cover introduction",
                "Fresh thinking.\nLasting impact.",
                [38., 356., 424., 25.],
                Face::Sans,
                p.ink,
                Left,
            );
        }
        1 => {
            a.text(
                "Story headline",
                "Built for\nthe bold.",
                [35., 95., 427., 51.],
                Face::Display,
                p.ink,
                Left,
            );
            a.rect("Strategy service block", [38., 253., 128., 171.], p.accent);
            a.rect(
                "Identity service block",
                [186., 253., 128., 171.],
                p.secondary,
            );
            a.rect("Campaign service block", [334., 253., 128., 171.], p.ink);
            a.text(
                "Strategy service number",
                "01",
                [53., 275., 98., 35.],
                Face::Display,
                p.surface,
                Left,
            );
            a.text(
                "Identity service number",
                "02",
                [201., 275., 98., 35.],
                Face::Display,
                p.ink,
                Left,
            );
            a.text(
                "Campaign service number",
                "03",
                [349., 275., 98., 35.],
                Face::Display,
                p.surface,
                Left,
            );
            a.text(
                "Strategy service title",
                "Strategy",
                [53., 337., 99., 14.],
                Face::Sans,
                p.surface,
                Left,
            );
            a.text(
                "Identity service title",
                "Identity",
                [201., 337., 99., 14.],
                Face::Sans,
                p.ink,
                Left,
            );
            a.text(
                "Campaign service title",
                "Campaigns",
                [349., 337., 99., 14.],
                Face::Sans,
                p.surface,
                Left,
            );
            a.text(
                "Strategy service detail",
                "A clear\npoint of view.",
                [53., 372., 99., 11.],
                Face::Sans,
                p.surface,
                Left,
            );
            a.text(
                "Identity service detail",
                "A look\nall your own.",
                [201., 372., 99., 11.],
                Face::Sans,
                p.ink,
                Left,
            );
            a.text(
                "Campaign service detail",
                "A story\nworth sharing.",
                [349., 372., 99., 11.],
                Face::Sans,
                p.surface,
                Left,
            );
        }
        _ => {
            a.rect("Impact headline block", [28., 95., 444., 174.], p.accent);
            a.text(
                "Call to action headline",
                "Let's make\nan impact.",
                [48., 120., 403., 54.],
                Face::Display,
                p.surface,
                Left,
            );
            a.rect("Contact statement block", [28., 288., 267., 136.], p.ink);
            a.caps(
                "Contact statement label",
                "YOUR NEXT CHAPTER",
                [49., 317., 225., 8.5],
                p.surface,
                Left,
            );
            a.text(
                "Contact statement",
                "Starts with hello.",
                [49., 355., 225., 20.],
                Face::Sans,
                p.surface,
                Left,
            );
            a.rect("Contact arrow block", [312., 288., 160., 136.], p.secondary);
            right_arrow(a, 347., 321., 1.62, p.ink);
        }
    }
    signal_footer(a, page, p);
}
