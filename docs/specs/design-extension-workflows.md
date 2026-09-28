# Local extension workflow design

Emulsion already supports reusable templates, stencils, brand packs, fonts,
components, variables, saved styles and MCP automation. Local files and supported
GitHub URLs exchange validated data packages. Their manifests and artwork never
execute repository scripts, installation commands, JavaScript or native libraries.

Current authoring extensions should use these existing mechanisms:

- Share artwork and page layouts with `.emutemplate` packs.
- Share editable diagram parts with stencil packages.
- Share portable typography, palettes and logos with brand packs.
- Automate documented native operations through MCP, retaining object validation,
  page ownership, cancellation checks and native Undo.
- Use CSV bindings for repeatable local data generation.

A scripting/plugin runtime is a separate product boundary. Before adding one,
define versioned commands and capability declarations; explicit file/network
scopes; package identity/version pinning; revocation; resource budgets; isolated
execution; cancellation; and atomic document transactions. UI extensions must
use the shared controls and accessibility contracts. Repository installation
cannot grant execution rights simply because a template URL was accepted.

No runtime is bundled by this design. The current data-only package workflows
remain available on Linux, Windows and macOS, and do not require a browser bundle.
