# Artist workflow evaluation

Status: protocol, not results. Improved artwork remains **unproven** until the comparisons below are run. Passing tool regressions establishes tool behaviour, not artistic ability.

Design reference: [Krita's brush-engine catalogue](https://docs.krita.org/en/reference_manual/brushes/brush_engines.html) separates brush behaviours into engines including bristle, colour smudge, hatching and pixel brushes. Emulsion's first-release playbooks distinguish technique and disclose its own procedural preset limits; they do not claim equivalence to those engines.

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

## Acceptance and expansion gate

Predeclare acceptance thresholds before viewing outputs: no tool errors or unintended pen-lift bridges in the fixed studies; native editability passes; no automatic request to move M2, R2 or W2 off centre; and candidate preference exceeds baseline preference overall without worsening median subject fidelity or the relevant anatomy/perspective score in any medium. Report time/cost against the predeclared budget alongside quality. Mark the result **INCONCLUSIVE** if missing runs, rater disagreement or unavailable costs prevent the agreed comparison. Do not expand to more media on prompt wording alone.

Required engineering regressions before live trials:

- Disconnected SVG subpaths leave no bridge and restart stroke state across pen lifts.
- Painting's explicit backdrop sampling mode observes lower-layer colour as documented, preserves the lower layers, and differs from sampling only the target layer in a controlled mixing test.
- Region previews preserve document-space origin, dimensions and pixel mapping for full and node views, including downsampling and canvas edges; invalid/out-of-bounds requests are handled consistently with the schema.
- Centred/symmetric content with intentional composition remains an observation, not an unconditional correction; omitted intent remains unknown.
- All worked playbook calls execute against current tool schemas and brush presets. Passing these examples does not establish that the assistant follows the workflow or produces better art.

Completion report fields: condition fingerprints, artifact locations, engineering PASS/FAIL, live drawing PASS/FAIL/INCONCLUSIVE, blind evaluation PASS/FAIL/INCONCLUSIVE, scores by brief/medium, editability results, time/cost, failures and limitations.
