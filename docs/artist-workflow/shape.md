# Adaptable artist workflow

Feature implementation with focused paint bug reproduction. Preserve pre-existing assistant/provider edits and leave all work uncommitted.

## Contract

- Embed selectable manga, Renaissance-inspired painting and watercolour playbooks with executable examples, brush choices, layers, checkpoints and bounded corrections.
- Carry medium, stage, composition intent and user constraints in typed critique context. Treat low-resolution metrics as observations; return images for assistant visual review rather than claiming metrics establish anatomy, perspective or subject fidelity.
- Validate document-space preview regions and return image-to-document coordinate mapping. Keep preview memory bounded and preserve native detail for close views.
- Expose brush uses, actual settings and rendered swatches without claiming faithful material simulation.
- Reproduce disconnected SVG strokes and lower-layer sampling, then add regressions and explicit sampling behavior.
- Define fixed briefs and repeated blind human evaluation before expanding media. Artwork improvement remains unproven until those trials run.

## Structure and verification

Static named playbooks govern technique; typed critique context separates intent from measurements; a validated preview region is the boundary between document coordinates and image coordinates. Rendering stays on native layers and paths. There is no image-generation backend.

Parallel ownership: assistant prompts/evaluation; AI critique; paint path/backdrop; MCP previews/discovery/integration. Verify affected crates with tests, schema/tool-boundary checks and linting. Record executable evidence and remaining human evaluation in report.md.

The named TStack feature-dev, blast-radius and verification-before-completion skills were absent from the available libraries. Repository guidance applies.
