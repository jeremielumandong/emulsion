# Artist workflow evaluation

Status: protocol, not results. Improved artwork remains **unproven** until the comparisons below are run. Passing tool regressions establishes tool behaviour, not artistic ability.

Design reference: [Krita's brush-engine catalogue](https://docs.krita.org/en/reference_manual/brushes/brush_engines.html) separates brush behaviours into engines including bristle, colour smudge, hatching and pixel brushes (re-inspected 2026-09-19). Emulsion's playbooks distinguish technique and disclose its own procedural preset limits; they do not claim equivalence to those engines.

The expanded catalogue accepts custom styles and combines visual style with medium technique. This is broader guidance at the user's request, not evaluated coverage of every style. Use the original six briefs as a stable baseline and the extension panel below for the expanded workflow.

## Fixed first-release briefs

Use an 800×600 canvas with identical white paper, identical reference assets (if any), and the exact prompt below for both conditions. Keep each supplied reference with the run record and record its checksum. Do not add a reference to one condition only.

| ID | Exact brief |
| --- | --- |
| M1 | Draw a manga courier running left to right with a satchel, a readable full-body gesture and visible hands. Use expressive line weight, two black masses and restrained screentone. Keep the background mostly white. |
| M2 | Draw a deliberately centred, bilaterally symmetric manga portrait of a young adult wearing round glasses. Preserve stylized proportions, balanced black hair shapes and quiet screentone. Do not move the face off centre. |
| R1 | Paint a Renaissance-inspired still life of a clay jug, an apple and a folded cloth on a table, viewed slightly from above. Use coherent perspective, value underpainting, restrained glazing and light from the upper left. |
| R2 | Paint a Renaissance-inspired standing figure beneath a deliberately centred symmetric stone arch. Show both hands, coherent perspective and quiet modelling of the face. Preserve the central axis. |
| W1 | Paint a loose watercolour of a small sailboat on calm water. Reserve white paper for the sail and reflected light; use broad pale washes, a few darker pigment passages and minimal detail. |
| W2 | Paint a deliberately centred watercolour study of one pear with a small cast shadow. Leave at least half the paper quiet, preserve a white highlight and use a restrained value range. Do not add an ink outline. |

## Paired runs

1. Freeze baseline and candidate source revisions, including dirty patches, provider/model/version, prompt text, tool settings, machine, references and output resolution. Baseline is the implementation before this release; candidate includes the playbooks, intent context and detail inspection. Use separate checkouts or saved builds so neither overwrites unrelated work.
2. Run each brief **three times per condition** (6 briefs × 3 repeats × 2 conditions = 36 drawings). Use fresh documents and fresh conversations; fix seeds where supported and record when they are unavailable. Randomize condition order. Use the same maximum wall time, token/cost budget and approval handling for both. Predeclare these budgets before the first run.
3. Preserve the layered native document, exported PNG, full tool transcript, full/detail review images, critiques with context, elapsed time, input/output token counts, provider cost and any errors or skipped edits. Record actual measurements; use `unavailable` for missing billing, never a guessed zero. Record both requested and effective checkpoint/correction counts.
4. Stop a run at the declared budget or explicit stop condition. Keep failed and incomplete runs in the dataset. Do not silently regenerate a weak drawing. The existing `emulsion-assistant` draw example can exercise a live provider and save a PNG, but it does not by itself collect all of these artifacts or prove blind comparisons.

## Blind human comparison

Give at least three independent raters the brief and reference, with outputs labelled only by random ids. Randomize A/B placement for each pair. Hide model, condition, tool count, timing and transcripts during visual scoring. Rate these dimensions from 1 (poor match) to 5 (strong match): subject fidelity, composition relative to intent, anatomy/perspective where relevant, and medium character. Use `not applicable` for anatomy/perspective when the brief does not require it. Include overall A/B preference and a short reason; ties are allowed. Centring, symmetry, white space or flat values must never be penalized merely for being present.

After visual scoring, audit native editability separately: named layers survive save/reopen; sketch/tone/glaze can be hidden independently; path geometry remains editable when used; no unexpected flattening occurred. Record pass/fail with a concrete attempted edit. Record time and cost separately from artistic scores; extra tool calls are not a quality bonus.

Summarize paired preferences, medians and spread by brief and by medium, plus rater disagreement. Report all three repeats, including failures; do not choose representative winners. Keep intentionally centred/symmetric briefs separate in the report so an improvement elsewhere cannot conceal a regression in respecting intent. With only three repeats this is exploratory evidence, not a general claim of master-level ability.

## Acceptance and quality claims

Predeclare acceptance thresholds before viewing outputs: no tool errors or unintended pen-lift bridges in the fixed studies; native editability passes; no automatic request to move M2, R2 or W2 off centre; and candidate preference exceeds baseline preference overall without worsening median subject fidelity or the relevant anatomy/perspective score in any medium. Report time/cost against the predeclared budget alongside quality. Mark the result **INCONCLUSIVE** if missing runs, rater disagreement or unavailable costs prevent the agreed comparison. Adding guidance is permitted; do not promote a medium or style as artistically validated on prompt wording or executable mark studies alone.

## Expanded medium/style panel

For this follow-up, freeze the implemented three-playbook version as baseline and the expanded version as candidate, including exact dirty patches and embedded prompts. Use the same budgets and blind protocol above, including three repeats per condition. The full panel is 21 briefs (six original plus fifteen below), or **126 drawings**. The fifteen additions alone are **90 drawings**. Report the original and extension groups separately; do not imply that this finite panel proves all possible styles. Record prompt size and input tokens because the larger embedded catalogue can affect time/cost.

| ID | Exact brief |
| --- | --- |
| S1 | Draw a realistic graphite study of a left hand holding a small cube. Keep construction light, model form with hatching and leave the paper visible. |
| S2 | Draw an expressive charcoal portrait with a deliberately centred face, broad dark masses and three sharp accents. Preserve the elongated features as intentional. |
| S3 | Draw a botanical coloured-pencil study of a lemon cut in half. Keep distinct translucent-looking colour strokes and accurate visible segment structure. |
| S4 | Draw a pastel landscape of three trees at dusk. Use soft broad masses, broken colour and only one crisp focal tree edge. |
| S5 | Draw a pen-and-ink urban sketch of a corner café with two visible street directions. Use crosshatching, clear perspective and editable lettering reading CAFE. |
| S6 | Paint an oil-style impressionist study of a red umbrella on a wet street. Establish light masses before broken brush marks; keep the umbrella recognizable. |
| S7 | Paint an acrylic-style hard-edge geometric abstraction with three interlocking forms. Keep flat colours, deliberate symmetry and no simulated depth. |
| S8 | Paint a gouache-style storybook fox carrying a blue bag. Use opaque-looking flat shapes and a few dry accents; keep the bag distinct from the tail. |
| S9 | Make a cel-shaded digital illustration of a runner tying a shoe. Use two clear shadow values and readable hands; retain stylized proportions. |
| S10 | Make an art-deco-inspired vector emblem of a heron. Keep editable paths, deliberate bilateral symmetry and a palette of black, cream and gold. |
| S11 | Make a pixel-art lantern using a 16-by-16 logical grid inside the 800-by-600 canvas, with 8-document-pixel cells. Use four colours, hard cell boundaries and readable pixel clusters without gradients. |
| S12 | Make a cut-paper collage of a sailboat and sun using native editable shapes on separate layers. Keep overlaps visible and use no external images. |
| S13 | Make a linocut-inspired black-and-cream owl using deliberate carved-looking gaps, broad masses and directional hatching. Keep the owl centred. |
| S14 | Paint a custom cubist watercolour teapot with deliberately fragmented facets, reserved white paper and translucent-looking washes. Preserve the spout and handle so it remains identifiable. |
| S15 | Use my invented style, Quiet Orbit: two flat-colour central shapes, a thin dotted halo, one deliberately oversized leaf, no gradients and lots of empty paper. Keep this exact style intent without substituting realism or manga. |

Alongside medium character, score style fidelity against the explicit brief, including requested distortions, symmetry and flatness. Apply the same subject, anatomy/perspective, editability, time and cost criteria. For S11 inspect document pixels and cell boundaries; for S10/S12 attempt a path edit after saving and reopening; for S15 check that the arbitrary style text survives in critique context. A future quality claim about another named style requires its own fixed briefs and results.

Required engineering regressions before live trials:

- Disconnected SVG subpaths leave no bridge and restart stroke state across pen lifts.
- Painting's explicit backdrop sampling mode observes lower-layer colour as documented, preserves the lower layers, and differs from sampling only the target layer in a controlled mixing test.
- Region previews preserve document-space origin, dimensions and pixel mapping for full and node views, including downsampling and canvas edges; invalid/out-of-bounds requests are handled consistently with the schema.
- Centred/symmetric content with intentional composition remains an observation, not an unconditional correction; omitted intent remains unknown.
- All worked playbook calls execute against current tool schemas and brush presets. Passing these examples does not establish that the assistant follows the workflow or produces better art.

Completion report fields: condition fingerprints, artifact locations, engineering PASS/FAIL, live drawing PASS/FAIL/INCONCLUSIVE, blind evaluation PASS/FAIL/INCONCLUSIVE, scores by brief/medium, editability results, time/cost, failures and limitations.
