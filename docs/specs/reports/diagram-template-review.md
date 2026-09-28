# Diagram template refresh

Reference collection: `/home/arkane/Downloads/Diagram` (September 28, 2026).

The catalog has 18 templates: the original eight stable IDs plus ten additions.
Saved webpages were inspected as local references; their scripts, stock imagery,
logos and branding are not distributed. The compositions use Emulsion's existing
editable vector stencils, native text and graph connectors.

| Local reference family | Gallery template / stable ID |
| --- | --- |
| Business process flow examples | Business process / `business-process` |
| Purchase process flowchart | Purchase approval / `purchase-process` |
| Family tree screenshot | Family tree / `family-tree` |
| Cause-and-effect / fishbone examples | Cause and effect / `fishbone` |
| Organizational chart infographic | Team directory / `org-profiles` |
| Five-element tree chart | Strategy tree / `branching-tree` |
| Three-step cycle infographic | Improvement cycle / `improvement-cycle` |
| Branching flow infographic | Project roadmap / `infographic-flow` |
| Genogram reference | Relationship map / `genogram` |
| Existing infrastructure/cloud stencils | Cloud architecture / `cloud-architecture`; refreshed `network` |

The existing `concept-map` starter remains available. The similarly named HTML
reference is an Emulsion presentation containing three responsive SVG views;
it is not a draw.io source graph.

Visual inspection covered all new layouts at page scale. It caught and corrected
opaque lane fills hiding connectors, lane headings overlapping vertical routes,
icon captions crossed by connectors and implicit fills on open line-only symbols.
The original swimlane starter benefits from the stacking and heading fixes too.

Reproduce the visual proofs and native round trips:

```sh
cargo run --offline -p emulsion-io --example diagram_template_preview -- target/diagram-template-review
```

The generated index links PNG previews, vector SVG exports and editable `.emu`
projects for all 18 templates. The command rejects raster export fallback and
checks document equality after native save/reopen. No generated bitmap is used
as a template in the application.

Regression coverage includes every template's graph serialization, moving each
sample object and undoing it, stencil catalog identity, page bounds, theme undo,
container/connector stacking, all-template insertion through MCP with undo/redo,
and UI search → template insertion → reusable document stencils → undo.

Final validation:

- 26 core diagram tests passed.
- 16 diagram MCP tests passed, including all 18 template insertions and undo/redo.
- 14 diagram UI workflow tests passed, including the new multiword template search.
- All 18 native save/reopen comparisons and vector-only SVG exports passed.
- The optimized application build completed successfully.
