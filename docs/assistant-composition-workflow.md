# Assistant composition workflow

This change adapts workflow ideas from the supplied
`C:\development\github\photoshop-uxp` checkout: inspect rendered pixels together
with structured state, preserve editable sources, and plan typography and image
placement deliberately. The source's `SKILL.md`,
`references/reading-results.md`, `references/layers-and-composition.md` and
`evals/evals.json` informed the work. Photoshop scripts and APIs are not runtime
dependencies.

The shared assistant prompt now includes a native composition guide for posters,
covers, supplied-image layouts and photo grading. It explains proportional type
and margins, available fonts, native multiline text, cover/contain arithmetic,
adjustment scope and inspection after meaningful edits. It respects the current
session's document rather than inventing open/create/import tools. Existing
typography and collage guidance points to this workflow.

`describe_document` retains its existing fields and adds numeric `source_size`,
signed scale factors and flip flags in `placement`, and `source_bounds` for pixel
and smart layers. Bounds enclose the transformed source rectangle in document
pixels, rounded outward; they do not describe visible alpha or filter effects.
The existing `placement.scale` remains a percentage; the new `scale_x` and
`scale_y` are factors. This lets the assistant diagnose placement without parsing
a display string or mistaking nonuniform scaling for uniform scaling.

The composition guide includes an executable editable layout study. Render it
with `cargo run -p emulsion-assistant --example playbook -- --guide composition
--out target/composition-studies`. The existing shared-prompt test executes its
calls with the rest of the shipped examples. Geometry regressions cover the new
readback independently of model behavior.

For a live assistant comparison, use the same provider, model and budget before
and after rebuilding and starting a new session:

- Create a social graphic: check exact copy, hierarchy, glyphs, wrapping, margins
  and delivery-size legibility, with editable text and shapes retained.
- Arrange a supplied photograph as a cover: check cover versus contain, the
  subject's crop, text contrast and unchanged source pixels. Include an existing
  flipped or stretched layer to check that its placement is inspected first.
- Grade an existing photograph to monochrome: check recoverable adjustments,
  intended group scope, retained tonal detail and unchanged original pixels.
  For an editable RAW, use native development settings first and verify that
  supported global corrections leave the layer stack unchanged.
- Diagnose dark text on a dark background, a hidden headline and an off-canvas
  asset: check state and pixels before correcting the specific cause. Obtain a
  new full preview after the final change.

Tool execution and geometry tests establish compatibility and readback accuracy;
they do not establish better artistic judgment by a live model.

## RAW-first editing

When an editable RAW source is attached, ordinary photo edits should develop that
source before adding layers. `describe_document.raw` exposes the source node and
current recipe; `describe_raw` supplies the detailed settings and source status.
The assistant uses `develop_raw` for supported global tone, white balance and
color changes, preserves omitted settings, then inspects the result. Monochrome
can use native saturation rather than an immediate black-and-white layer.

The quick-command planner defers RAW adjustment requests to this assistant
workflow, and layer-only adjustment suggestions are suppressed on RAW documents.
Explicit layer operations remain available. Layers are appropriate for requested
layer work or operations that RAW controls cannot perform, such as localized
retouching and compositing; the assistant should explain that need. An active
selection does not make RAW development local. Missing originals should be
reported or relinked rather than silently substituting adjustment layers.

RAW follow-up validation: 40 AI, 33 assistant and 120 MCP library tests passed.
Regressions cover quick-command deferral, RAW suggestion suppression, source
discovery and development without extra layers, preserved original bytes and
undo. Clippy with warnings denied, affected-crate formatting and diff checks
passed. Live model behavior still needs evaluation in a new assistant session.

## Validation

- MCP library: 119 tests passed, including transformed-source readback and its
  read-only contract.
- Assistant library: 32 tests passed on the first run, including all shipped
  playbook examples. The existing Windows PowerShell version-probe test failed
  once and passed when rerun separately (33 distinct tests passed overall).
- Clippy passed for the assistant and MCP libraries, tests and playbook example
  with warnings denied. Formatting passed for both affected crates; the broader
  workspace check reported unrelated formatting differences in IO, raster and
  UI files. Diff whitespace checks passed for the affected files.
- Rendered and visually inspected `target/composition-studies/study-01.png`:
  multiline copy is legible, high-contrast and unclipped, with the intended
  margins and hierarchy. The runner also saved editable `study-01.ora`.
- The installed application was not replaced or restarted. Load the rebuilt app
  and start a new assistant session to use the new shared prompt.
