# Adaptable artist workflow — style expansion report

Feature engineering: **PASS in an isolated snapshot**. Shared-checkout integration attempt: **FAIL** on unrelated concurrent RAW/lens-import compilation errors. Live drawing and blind artistic comparison: **INCONCLUSIVE / unproven**. All changes made for this follow-up are uncommitted.

## Delivered

The user's follow-up expands the original three-playbook scope. Selection now accepts any named, custom, reference-defined or hybrid visual style. The catalogue is intentionally non-exhaustive; it never rejects a style because its name is absent.

- **16 technique playbooks**: the existing manga, Renaissance-inspired and watercolour playbooks plus graphite, charcoal, coloured pencil, pastel, pen-and-ink, oil, acrylic, gouache, digital painting, vector/flat illustration, pixel art, collage/mixed media and printmaking-inspired workflows. Each has concrete tool/brush choices, editable layer/node structure, checkpoints and executable worked calls.
- **39 visual-style families**, covering representational, historical, abstract, comic, graphic, regional/reference-based and contemporary directions. A separate table specifies possible visual traits and brief-relative review questions. Custom/hybrid guidance combines medium technique with shape, edge, palette, space and detail choices. These are directions to select from, not definitions of whole traditions.
- **Optional free-text critique style**, independent of medium. The schema has no enum. Exact custom wording and constraints survive the actual JSON-RPC tool boundary; omitted style defaults to empty for older callers. Review explicitly preserves deliberate abstraction, invented proportions, flat space and symmetry.
- **17 worked studies / 128 tool calls**, including one custom cubist-watercolour hybrid. The regression executes every JSON array actually embedded in the shared provider prompt, validates tool argument names and required fields, checks separate editable nodes, cleared selections, image responses, region mapping and returned critique context.
- The [evaluation protocol](../artist-evaluation.md) retains the original six briefs and adds fifteen fixed briefs for the expansion, including invented style intent, hybrids, flat symmetric graphics and exact pixel cells. Three repeats per condition require 126 drawings for the full 21-brief panel (90 for the extension alone). No drawings or human ratings were generated in this task.

The studio remains observant, decisive, patient and self-critical. It selects tools for the picture rather than tool-count targets. The existing bounded three-checkpoint/two-correction-pass policy, native editable workflow, brush swatches, pen-lift preservation, explicit backdrop sampling and full/detail review remain in place. Workflow selection is assistant guidance, not a deterministic style selector or a new UI menu.

## Verification evidence

The shared checkout changed independently during this session. Its HEAD advanced to `1588642c648892b6da6168d81ba3be16433164a2`, containing the earlier artist implementation and provider fixes. Concurrent RAW/lens-import changes then prevented the assistant test build: missing `ureq`, private rawler imports and an unavailable XML text method in `emulsion-io/src/{exif,lensfun}.rs`. Those edits were preserved.

To verify this feature without modifying unrelated work, archive that committed baseline into `/tmp/emulsion-styles-verify-ok88cnx6` and overlay only the seven feature source files listed below plus the updated shape and evaluation documents. All seven tested feature source files were subsequently compared byte-for-byte with the working tree and matched. Isolated Cargo commands used `CARGO_TARGET_DIR=/home/arkane/Projects/emulsion/target`.

| Check | Result |
| --- | --- |
| Custom-style JSON-RPC regression before adding the field/schema | **FAIL as expected**: missing style schema. |
| `cargo test -p emulsion-assistant --lib launch::tests::playbook_examples_execute_with_current_tools -- --nocapture` in the shared checkout | **FAIL to compile**, due to unrelated concurrent I/O changes above. |
| Same worked-example command in the isolated snapshot | **PASS: 128 calls / 17 studies**, 1 test, 1.04 s test execution. |
| `cargo test -p emulsion-assistant -p emulsion-ai -p emulsion-mcp --lib` in the isolated snapshot | **PASS: 67/67** (assistant 12, AI 22, MCP 33). Includes real JSON-RPC custom-style and legacy context requests, original pen-lift/backdrop regressions and ROI mapping. |
| `cargo clippy -p emulsion-assistant -p emulsion-ai -p emulsion-mcp --lib -- -D warnings` in the isolated snapshot | **PASS**. |
| Scoped `rustfmt --edition 2024 --check` and `git diff --check` in the working tree | **PASS**. |
| Embedded prompt source/JSON inspection | **PASS**: 16 playbooks, 39 style-family rows, 17 valid JSON studies, all included in `SYSTEM_PROMPT`. |
| Live model adherence, artwork quality, human blind comparison, actual provider time/cost | **INCONCLUSIVE / unproven**, not exercised. |

Prompt SHA-256 (studio, manga, renaissance, watercolour, media, styles in embedding order): `5683d650e89c2605645ee73631a6614a2bd18e13eded1a9c4c3869e31edea43b`.

## Limits and cost

A finite catalogue cannot enumerate every possible visual style. Open-ended brief handling removes the shortlist restriction; it does not establish mastery of untested styles. Digital oil, gouache, charcoal and other recipes remain approximations using available tools, not new physical material engines. Image review remains a task for the calling assistant, not a completed assessment by the metric analyzer.

The embedded prompt grows from 14,473 to **46,252 UTF-8 bytes**. This can increase provider input tokens and cost; actual token counts, cache effects, latency and quality are unmeasured. Providers retain the existing shared prompt delivery paths. No image-generation backend, style dropdown, medium simulation engine or automatic claim of artistic validation is added.

The user's explicit request supersedes the earlier restriction on adding guidance before evaluation. Evaluation still gates claims of improved quality. The original focused paint reproductions previously failed on gap alpha (65535 rather than 0) and missing lower-layer colour pickup; their fixed regressions continue to pass in this snapshot.

## Changed files in this follow-up

- `crates/emulsion-assistant/src/prompts/studio.md`: open-ended medium/style selection and style-aware review contract.
- `crates/emulsion-assistant/src/prompts/media.md` (new): thirteen additional technique playbooks and 86 worked calls.
- `crates/emulsion-assistant/src/prompts/styles.md` (new): thirty-nine style families, custom/hybrid workflow and nine worked calls.
- `crates/emulsion-assistant/src/launch.rs`: embed the expanded catalogue; execute every embedded study in the regression.
- `crates/emulsion-ai/src/critique.rs`: optional free-text style and style-aware review policy.
- `crates/emulsion-mcp/src/tools.rs`: backward-compatible critique style schema.
- `crates/emulsion-mcp/src/review.rs`: style-aware visual instruction and real JSON-RPC regression.
- `docs/artist-evaluation.md`: extended fixed briefs, style fidelity and prompt-cost measurement.
- `docs/artist-workflow/shape.md`: updated scope and acceptance contract.
- `docs/artist-workflow/report.md`: this report.

The original three playbook files, paint/preview implementations and unrelated concurrent changes were not edited in this follow-up. No commits were made by this task.
