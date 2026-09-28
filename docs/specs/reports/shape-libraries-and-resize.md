# Shape-library discovery and stencil resizing

The fixed **Add shapes…** button in the diagram drawer opens a searchable library modal. It offers library selection, paged vector previews, installed personal packs and explicit shape import. **Use selected shapes** applies the draft; Cancel does not change preferences or install packs.

Standard and Flowchart are enabled by default. Other native categories and installed packs are hidden until chosen. Preferences persist in application settings. Removing a group from the toolbox retains its installed files and all existing canvas objects. Explicit stencil import enables the imported group. Library installation runs sequentially in the background after Apply and reports import notes.

The chooser exposes all 49 families in the existing pinned draw.io archive (8,954 source definitions), including cloud, network, Android/iOS, geometric, flowchart, BPMN, process and value-stream families. Native choices include Containers, UML and original AI workflow symbols. This is the bundled catalog, not an exact copy of Lucidchart's current catalog or third-party AI provider logos. Unsupported individual definitions may produce import notes; family validation does not imply perfect fidelity for every source definition.

Resize repair: a visible corner handle now gets the pointer gesture before diagram hit-testing can mistake the empty corner of a nonrectangular stencil for blank canvas. Connector-specific handles and midpoint connection ports retain their existing behavior.

Validation:

- 31 diagram UI tests passed, including draft/Cancel/Apply library behavior and installed pack drag/drop.
- Real mouse-drag resizing grows and shrinks ellipse, diamond, Web Systems and imported Android stencils; two undos restore the original document.
- All 49 bundled families built a nonempty native-vector pack; per-entry preview installation also passed.
- Native stencil insertion, attachment and undo checks cover the built-in catalog.

Logs: `/tmp/emulsion-libraries-ui-final.log`, `/tmp/emulsion-libraries-pack-tests.log`, `/tmp/emulsion-libraries-native-final.log`.

Stencil export now prepares a validated copy: plain page backgrounds are removed only when they are not clipping bases, masked artwork or styled fills. Removed-node design metadata is pruned, and empty packs are rejected before opening the export dialog. Export filenames use the pack name, and file-picker failures are reported. The native file-picker → write → read → install → stencil placement test passes. The original reported failure has not been reproduced from a supplied pack, so this is not evidence that every pack-specific failure is fixed.

Folder references are integrated with diagram MCP through `attach_reference_folder` and `get_reference_attachments`. Snapshots prioritize documentation/manifests and skip generated dependencies and common credential files. Assistant instructions require source-backed editable diagrams, explicit inferences and rendered verification. This is bounded context for an AI provider, not a deterministic whole-repository analyzer. See `docs/guides/assistant-references.md` for limits and usage.

Codebase validation: the filesystem fixture verifies README/manifests and source inclusion, dependency/credential omission, bounded contents and immutable snapshots. A live local MCP relay test attaches a folder, retrieves its text without changing artwork, then inserts an editable diagram stencil. Both codebase tests passed (`/tmp/emulsion-codebase-mcp-tests.log`). No paid AI-provider turn or real-codebase architecture accuracy evaluation was run.
