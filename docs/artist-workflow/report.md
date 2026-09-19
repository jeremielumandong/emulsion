# Adaptable artist workflow — implementation report

Engineering: **PASS** for the changed feature paths. Full assistant suite: **FAIL**, with two unrelated provider-test mismatches in the existing working tree. Live artwork quality and blind comparison: **INCONCLUSIVE / unproven**. All changes are uncommitted.

## Delivered

- A consistent studio personality and three selectable, embedded playbooks: manga, Renaissance-inspired painting and watercolour. Each includes brush/layer choices, checkpoints and executable tool examples. The prompt bounds review to three checkpoints and two correction passes each, with earlier stop conditions. This is assistant guidance, not a runtime-enforced state machine.
- Typed critique context carries medium, stage, composition intent and user constraints. Metrics remain observations; suggestions are conditional. Intentional centring and symmetry are preserved. Preliminary-stage finishing observations receive lower priority.
- `critique` returns a full image and optional detail image, context, measurements and an explicit pending visual-review instruction. Anatomy, perspective and subject fidelity are reviewed by the calling assistant against images and references. No hidden vision model or claim of completed visual assessment is added. Optional Jev ranking still only ranks metric observations.
- `get_view.region` accepts an in-canvas integer `[x,y,width,height]` in document space. Output includes exact region, dimensions, origin, per-axis scale and pixel-centre convention. Rendering visits only intersecting mip tiles and bounds preview allocation. Small regions retain native detail; invalid regions return errors.
- `list_brushes` returns actual preset settings, ranges, intended uses and a rendered swatch sheet. Name/category filtering and swatch pagination keep inspection manageable. Sampling conditions and material-simulation limits are explicit.
- SVG pen lifts now split independent strokes, with separate pressure envelopes, closure and playback length. `paint`/`hatch.sample_merged` explicitly opts into a frozen visible lower-layer backdrop; default remains current-layer sampling. The backdrop respects transforms, masks, opacity and hierarchy. Immediate rendering and animated playback share stroke setup.
- Image-bearing inspection tools run against document snapshots off the UI thread. Native paths and layered editing remain the workflow.

## Verification evidence

| Check | Outcome |
| --- | --- |
| Pre-fix `cargo test -p emulsion-mcp paint_regression` | **FAIL as expected**: 2/2 reproductions. Gap pixel alpha was 65535 instead of 0; wet paint stayed pure blue instead of sampling red below. |
| Final `cargo test -p emulsion-mcp --lib` | **PASS: 33/33**, including five paint regressions, four preview tests, image-bearing JSON-RPC roundtrip, critique context/images and brush swatches. Test execution 0.60 s. |
| `cargo test -p emulsion-mcp -p emulsion-ai --lib` | **PASS**: AI 22/22; MCP 31/31 at that point. Two additional MCP boundary tests passed in the final run above. |
| `cargo test -p emulsion-assistant --lib launch::tests::playbook_examples_execute_with_current_tools` | **PASS: 1/1**, executing 33 example calls through real tools, checking ROI mapping and separate editable layers. |
| `cargo check -p emulsion-ui` | **PASS**. |
| `cargo clippy -p emulsion-mcp -p emulsion-ai -p emulsion-assistant -p emulsion-ui --lib -- -D warnings` | **PASS** across affected libraries. |
| Targeted `rustfmt --edition 2024 --check` and `git diff --check` | **PASS** for modified Rust sources and patch whitespace. |
| `cargo test -p emulsion-assistant --lib` | **FAIL: 10 passed, 2 unrelated failures**, detailed below. |

The assistant failures are existing provider behavior/test conflicts. `launch::tests::one_shot_argv_and_configs` still expects `approval_policy="never"`, which the pre-existing launcher changes stopped forcing. `protocol::tests::opencode_stream_maps_to_events` indexes an immediate result after `step-finish`, while the independently changed parser accumulates usage instead. Those provider changes and assertions were not altered by this feature. Existing `examples/providers.rs` also prevented whole-package formatting from being clean; targeted formatting passed.

## Remaining evidence boundary

No live provider drawing trials, live Jev ranking, desktop playback session or blind human comparison was performed. Tool execution and compilation do not prove that a model follows the playbooks or produces better drawings. The [evaluation protocol](../artist-evaluation.md) fixes six briefs, including intentional centring/symmetry, and prescribes 36 baseline/candidate drawings, blind ratings, native-editability checks and measured time/cost. Evaluate before adding more media.

## File manifest

- `crates/emulsion-assistant/src/launch.rs`: embeds prompts and executes worked examples in a regression test; preserves prior provider changes.
- `crates/emulsion-assistant/src/prompts/studio.md`: personality, selection and bounded review policy.
- `crates/emulsion-assistant/src/prompts/manga.md`: manga playbook.
- `crates/emulsion-assistant/src/prompts/renaissance.md`: Renaissance-inspired playbook.
- `crates/emulsion-assistant/src/prompts/watercolour.md`: watercolour playbook.
- `crates/emulsion-ai/src/critique.rs`: typed context, conditional observations, ranking policy and regressions.
- `crates/emulsion-mcp/src/tools.rs`: schemas and descriptions.
- `crates/emulsion-mcp/src/lib.rs`: internal module registration.
- `crates/emulsion-mcp/src/exec.rs`: tool dispatch, pen lifts, backdrop and paint regressions.
- `crates/emulsion-mcp/src/preview.rs`: region rendering, mapping and tests.
- `crates/emulsion-mcp/src/review.rs`: image/context review response and tool-boundary tests.
- `crates/emulsion-mcp/src/brush_discovery.rs`: preset metadata, swatches and test.
- `crates/emulsion-ui/src/assistant.rs`: shared playback stroke initialization and asynchronous inspection.
- `docs/artist-evaluation.md`: fixed briefs, blind evaluation and source reference.
- `docs/artist-workflow/shape.md`: working contract.
- `docs/artist-workflow/report.md`: this report.

Unrelated `.gitignore`, assistant `protocol.rs`/`session.rs`, `examples/providers.rs`, `AGENTS.md` and AgentOps metadata were left untouched by this implementation.
