# Adaptable artist workflow

*Snapshot from 2026-09-19. For current behavior see [brush-workflow.md](../brush-workflow.md).*

Feature implementation with focused paint bug reproduction. Preserve pre-existing assistant/provider edits and leave all work uncommitted.

## Contract

- Embed selectable medium playbooks with executable examples, brush choices, layers, checkpoints and bounded corrections. Extend the original manga, Renaissance-inspired painting and watercolour guidance to dry media, ink, opaque paint, digital, vector, pixel, collage and printmaking workflows.
- Accept any requested visual style, including arbitrary custom names and hybrids. A non-exhaustive visual-style lookup table supplies working traits and review questions independently of the medium; neither axis is an enum or whitelist.
- Carry medium, style, stage, composition intent and user constraints in typed critique context. The optional free-text style defaults to empty for older callers. Treat low-resolution metrics as observations; return images for assistant visual review rather than claiming metrics establish anatomy, perspective or subject fidelity.
- Validate document-space preview regions and return image-to-document coordinate mapping. Keep preview memory bounded and preserve native detail for close views.
- Expose brush uses, actual settings and rendered swatches without claiming faithful material simulation.
- Reproduce disconnected SVG strokes and lower-layer sampling, then add regressions and explicit sampling behavior.
- Define fixed briefs and repeated blind human evaluation for every claimed area of effectiveness. At the user's explicit request, broaden style/medium guidance now; coverage is not evidence of artistic success. Artwork improvement remains unproven until those trials run.

## Structure and verification

Static named playbooks govern technique; a separate style lookup and custom workflow govern visual traits. Typed critique context separates intent from measurements without making styles mutually exclusive or limiting names. A validated preview region is the boundary between document coordinates and image coordinates. Rendering stays on native layers and paths. There is no image-generation backend.

Follow-up scope: expand the existing implementation, preserving earlier work. Parallel ownership: additional medium playbooks; free-text critique style and boundary regression; parent owns studio/style catalogue, prompt integration, example execution and evaluation documentation. Existing paint and ROI implementation need no redesign. Verify every worked JSON study embedded in the provider prompt through real tools, and custom/legacy critique requests through the JSON-RPC boundary. Record executable evidence and remaining human evaluation in report.md.

The named TStack feature-dev, blast-radius and verification-before-completion skills were absent from the available libraries. Repository guidance applies.
