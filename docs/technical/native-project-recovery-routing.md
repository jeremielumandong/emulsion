# Native project recovery routing

Native `.emu` reads have two explicit contracts. `project::read` and
`read_from` are strict: a project that needs a recovery diagnostic returns
`IoError::ProjectRecoveryRequired { report }`. `read_with_report` and
`read_from_with_report` may return `OpenedProject { project, report }` only
when the native preservation guard permits recovery. They are not permissive
fallback decoders for corrupt or unsupported protected content.

## Report-aware entry points

- `emulsion-ui/src/workspace/projects.rs`: ordinary native opens and recovery
  opens decode the whole project before installation, then display the report.
  Recovery keeps the durable recovery file until a later successful save. A
  recovery warning is installed after recovery bookkeeping so it cannot be
  replaced by “Recovered all pages.”
- `emulsion-mcp/src/project_tools.rs::load_pages`: the `.emu` branch returns
  the report's warnings using its existing warning vector. Other importer
  warnings keep their existing contract. Its asynchronous page-import host in
  `emulsion-ui/src/assistant/project_mcp.rs` already returns that vector in the
  tool response; it inserts pages into an existing project, rather than
  installing a new project editor.
- `emulsion-mcp/src/workspace_tools.rs::load_file`: normal project opens use
  `load_pages`; the template-copy branch directly carries the report. Copies
  remain unbound to the source path.
- `emulsion-ui/src/workspace/mcp_lifecycle.rs::install_mcp_file`: the complete
  warning vector is retained in the MCP result and in the installed editor's
  import/export notes. It also sets a visible warning status, including the
  first diagnostic and the total warning count. Activating an already open
  tab does not replay the new read's warnings or clear the existing status.
- `emulsion-ui/src/workspace/new_canvas_templates.rs`: real local-template
  submission returns the session and report together, installs through
  `install_created`, then shows warnings in the resulting editor. Built-ins
  return empty reports. Local previews deliberately remain strict, and their
  success or failure is never used as admission evidence for the real open.

The shared post-install presenter keeps every warning in the existing notes
list and points to Import / export notes in the Export menu. Clean opens do
not create a warning status. Nothing in these routes automatically saves a
recovered project or installs pages from a failed decode.

## Deliberately strict readers

These routes retain the bare API. Recovery-aware behavior would require a
separate, explicit report and presentation contract:

- Design-only template application: UI `editor/design_template_ui.rs`
  (`Source::Local`). It still requires a Design project after strict reading.
- Catalog registration and installation: UI `editor/creative_ui.rs` and MCP
  `creative_catalog_tools.rs` (`install_design_template`).
- Packages, including GitHub downloads: I/O `template_pack.rs` reads the
  embedded `project.emu` strictly. UI `workspace/projects.rs` package import,
  UI creative-pack routes, and MCP `project_tools.rs` package/GitHub branches
  inherit that strictness. Recovery-needed packages must fail instead of
  returning an empty warning vector alongside a recovered project.
- Stencil insertion: UI `editor/creative_pack_ui.rs::use_local_stencil_at`
  and MCP `diagram_project_tools.rs::insert_diagram_pack_entry`.
- Storyboard scene import: MCP `storyboard_tools/board.rs::import`.
- Cloud upload and merge: I/O `cloud.rs::Native::read`, UI
  `cloud_shared_ui.rs` base/theirs reads, and MCP
  `storyboard_tools/sharing.rs::merge_storyboard_revision`.
- Extract/sharing reads: I/O `storyboard_extract.rs::read_extract`; its
  callers inherit strict failure before extract validation or merging.
- Linked `.emu` Smart sources: I/O `smart_source.rs::decode` uses strict
  `read_from` before taking its single page.
- Benchmark, preview/re-export examples and export readback validation:
  UI `editor/canvas_benchmark.rs`; I/O examples `design_finishing_workflows`,
  `design_export_fidelity`, `design_invitation_preview`,
  `diagram_source_preview`, `diagram_reexport`, `svg_navigation`,
  `diagram_template_preview`, `design_family_preview`, and `drawio_preview`.
  Existing native export round-trip tests continue using bare reads.

MCP `storyboard_tools/library.rs::list_storyboard_templates` also performs a
strict read for optional catalog metadata. A failed read omits that optional
metadata; it does not install a recovered project. Thumbnails/covers and
native graph-free library drawings are limited readers, not proof that an
entire project's history has passed admission.

## Regression coverage

Caller tests write real storyboard projects containing a board version and a
removed panel. The recovery case retains the version's panel reference while
omitting only its retired graph before writing, producing a genuinely missing
`history/board/<panel>.ora` entry. The clean case retains the graph. No decoder
or recovery report is mocked.

Coverage includes:

- File/reader strict versus report-aware results, with typed diagnostic and
  `ProjectRecoveryRequired` assertions.
- Native UI open and recovery completion, with visible status-strip elements,
  a single occurrence of the diagnostic in status, intact source bytes, and
  no partial installation on corrupt input.
- MCP page loading, native file loading, template copies, actual editor
  installation, combined importer/recovery warnings, and duplicate-tab reuse.
- Built-in and clean local-template empty reports; a strict recovery-needed
  preview followed by successful actual local-template submission and visible
  warning delivery after creation.
- Actual Design `Source::load`, cloud packing, extract reads, and catalog
  installation remaining strict. The companion I/O preservation change owns
  package-admission fixtures: the ordinary template-pack writer deliberately
  removes private history, so caller fixtures cannot use it to manufacture a
  recovery-needed embedded project.

This caller change was prepared under a source-only runtime freeze. Direct
Rust formatting and whitespace checks were performed; compilation, typecheck,
unit/integration tests, and GUI/GPU execution were not run in this worktree.
