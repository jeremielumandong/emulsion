# Print dialog design

Status: design snapshot, 2026-09-27. The shared dialog and first native adapters
are now implemented. The September 28 follow-up adds direct Library selections,
Design frames and timestamped storyboards, labels, ICC conversion and flattened
CMYK PDF/X-1a:2001 / PDF/X-3:2002. The sections below preserve the original plan;
see [Printing](../guides/printing.md) for current behavior and remaining
platform acceptance. Open [the interactive concept](print-dialog-prototype.html) in a
browser. Its printers and artwork are demonstration data; it cannot submit jobs.

Required platforms: **Windows, macOS and Linux in the first printing release**.
The user explicitly requires all three. Platform adapters are separate workstreams
in the delivery plan, not later optional ports. Shared creative controls must have
the same behavior; printer-specific capabilities and native setup UI may differ.

## Product direction

One **File → Print…** command (Ctrl+P / Cmd+P) across Photo, Draw, Design, and
Diagram. Open a large, resizable dialog with a paper preview on the left and a
scrollable settings column on the right. Keep the destination and final action
visible. The preview shows the physical sheet, printable area, artwork position,
cropping, and page count. Use Emulsion's existing theme and Geist typography.

Default to the current photo/canvas or current design page. A library selection
and selected project pages can open the same dialog with multiple sources.
Print the current edited appearance from a snapshot, including unsaved changes.
Never flatten or resize the source document to print it.

Considered alternatives: a system-only dialog is simpler but offers little
control over photo placement or poster tiling. Separate dialogs for each
workspace duplicate settings and produce inconsistent output. Recommend the
shared creative-layout dialog, with native printer integration and a system
dialog path when the platform requires it.

## Layout and interaction

At roughly 1100 × 780, give the preview about two thirds of the width and settings
about 350 px. On smaller windows, stack the sections and keep actions reachable.

- Header: Print, document name, selected source count.
- Preview toolbar: layout preset, fit-to-view / zoom, sheet navigation.
- Preview: white paper on a neutral surround, dimension labels, dashed printable
  boundary, optional bleed/trim overlays. Preview zoom never changes print size.
- Settings, in order: Destination; Content; Paper; Layout; Color & quality;
  expandable Advanced. Avoid tabs that hide the selected printer.
- Footer: sheets × copies, physical output size, relevant quality warning;
  Cancel and **Print**. For PDF use **Save PDF…**. For portal setup use
  **Choose printer…** until settings are returned.

Use labeled controls, visible keyboard focus, accessible descriptions for
unsupported settings, and text with every status indicator. Escape closes before
submission; keyboard focus stays in the dialog and returns to its opener.
Submitting disables repeated activation. Canceling generation must work without
blocking the UI; canceling an already submitted job is a separate action.

## What can be printed

| Source | Default | Additional layouts and options |
| --- | --- | --- |
| Photo / developed RAW | Current edited photo, fit within printable area | 4 × 6, 5 × 7, 8 × 10 inch artwork sizes; custom size; fill and reposition crop; repeated photos; contact sheet |
| Drawing / mixed canvas | Visible composite, fit within printable area | Actual size from document resolution; custom scale; selected region; tiled poster |
| Design / presentation | Current page | All or selected pages, ranges, actual size, multiple pages per sheet, bleed and crop marks |
| Diagram | Current page/canvas | Fit, actual size, tiled poster with overlap and assembly labels |
| Animation / local video | Explicitly selected still frame | Storyboard from selected frames, timestamps, columns and spacing |
| Linked video / other interactive objects | Existing printable poster artwork | List objects without printable artwork before submission; let user choose omission or a placeholder. Do not silently claim to print playback or audio. |

Photo sizes describe the artwork, while paper size describes the sheet: a 5 × 7
photo may be centered on Letter paper. A repeated-photo layout repeats one image;
a contact sheet lays out distinct selected sources. Keep these choices separate.
Poster mode divides one large artwork across physical sheets. Ordinary page
printing outputs one source page per sheet unless the user enables multiple-up.

## Options and defaults

| Group | Controls | Rules |
| --- | --- | --- |
| Destination | Printer, status, Refresh, Printer setup…, Save PDF | Start with the system default; remember a chosen available queue by stable ID. Never silently substitute another printer. |
| Content | Current / all / selected pages, ranges, selected photos | Show only relevant choices. Display user ranges as 1-based. Resolve order before layout. |
| Paper | Size, portrait / landscape / auto, media type, source tray | Query supported combinations. A4/Letter are fallbacks for PDF, not assertions about a physical printer. Custom paper only when supported. |
| Layout | Fit, fill, actual size, custom dimensions or %, aspect lock, center / offsets | Fit is the safe first default. Fill visibly crops. Actual size uses document pixels / resolution, never screen zoom. |
| Margins | Printer minimum, extra margins, borderless | Respect asymmetric nonprintable margins. Borderless requires support for the selected size and media; warn about driver expansion if known. |
| Job | Copies, collate, one-sided / duplex long-edge / short-edge | Duplex only on supported combinations. Default one-sided, particularly for photo media. |
| Color & quality | Color / grayscale, draft / normal / high | Read available settings from the printer. Keep image PPI separate from device DPI. |
| Contact sheet | Rows, columns, gutter, labels, source order | Calculate image boxes after margins and labels; never distort aspect ratio. |
| Poster | Final artwork dimensions, overlap, cut marks, tile order | Show total sheets, tile coordinates and assembly preview before submission. |
| Design advanced | Bleed, crop marks, selected page boxes | Bleed needs actual artwork beyond trim; adding empty space is not generated bleed. Marks require paper space outside trim and bleed. |
| Color advanced (later) | Printer-managed / app-managed ICC, profile, intent, black-point compensation | App-managed output only when profile transform and driver color-management bypass are both verified. Never apply two color transforms. |

First release uses printer-managed color with a defined RGB output profile.
Do not expose approximate CMYK editing as a press-output guarantee. PDF/X,
spot colors, separations, overprint simulation and certified proofing are future
features, not implied by Save PDF. Soft proofing likewise needs a separate
verified display/profile pipeline.

Remember layout preferences separately from device options. Revalidate paper,
media, tray, sides and color after every destination change; explain any reset.
Offer named presets later (e.g. “4 × 6 glossy” and “A4 design proof”).

## Physical layout and quality

Store geometry in millimeters (or PDF points with explicit conversion), using
floating point until rasterization. Convert pixels to millimeters using
`pixels / document_ppi × 25.4`; use 72 points per inch in PDF.

Compute the printable rectangle from the selected paper's four hardware margins,
then subtract optional user margins. Fit uses the smaller of the two axis scales;
fill uses the larger and clips at the placement rectangle. Preserve aspect ratio.
Rotate printable bounds and margins consistently with orientation. Apply layout
once: disable additional driver fitting, rotation and multiple-up where the app
has already composed the sheets. Native-only controls need one clear owner.

Show effective PPI for placed raster content, accounting for the crop and actual
physical size. As a configurable product heuristic, flag below 150 PPI as likely
soft and 150–299 as below a 300 PPI photo target; these are advisory, not universal
quality guarantees. Vector-only objects have no finite source-PPI warning.
Raster effects require an explicit output rendering resolution. Setting “300 DPI”
metadata does not create detail.

Count sheets after source ranges, imposition and duplex, then apply copies.
Collated duplex copies each start on a new sheet; odd page counts may need a
blank back. Preflight impossible dimensions, empty ranges, unavailable sources,
clipped actual-size content and incompatible device settings. Warn visibly about
cropping or low resolution; block malformed jobs. Preview and output consume the
same resolved layout so there is no independent preview approximation in production.

## Connecting to available printers

Use operating-system queues for configured USB and network printers. “Available”
means reported by the print service; being on the same network does not ensure a
printer is installed or reachable. Refresh asynchronously with cancellation and
timeouts. Keep saved/offline printers distinguishable from ready destinations.
Open system printer setup for adding drivers or configuring a printer; do not
install drivers, modify the system default or perform broad network scans.

| Platform | Proposed adapter | Key boundary |
| --- | --- | --- |
| Linux native / AppImage | CUPS destination and job APIs; IPP capability queries | Enumerate queues/discovered destinations, query supported media and options, submit a supported document format, retain job ID. Package or detect libcups intentionally. |
| Linux Flatpak / portal path | XDG Print portal | Use system selection and page setup, then compose output from returned settings. The portal is not a general printer-enumeration API. |
| macOS | AppKit printing, NSPrinter / NSPrintInfo / NSPrintOperation | Reconcile custom layout with native paper bounds and native options. |
| Windows | Spooler enumeration and capabilities, native print settings, suitable rendering adapter | Do not send raw PDF bytes to arbitrary Windows queues. Choose and validate an XPS or GDI-based render path supported by the driver stack. |

CUPS provides destination enumeration, option validation and job submission
APIs. Prefer those structured interfaces over parsing localized `lpstat` output
in the product. [CUPS programming reference](https://openprinting.github.io/cups/doc/cupspm.html).

The portal flow is **Choose printer → PreparePrint → apply returned page setup
and settings → preview → Print with the returned token and a file descriptor**.
Version-gate optional parameters; handle cancellation and expired tokens. Returned
settings must be reconciled with app-owned source ranges and imposition so neither
is applied twice. Reopen setup if device settings change. Portal success means
handoff, not confirmed physical output; its interface does not provide the same
queue monitoring as direct CUPS. [XDG Print portal reference](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Print.html).

Apple exposes printer information and print operations through
[AppKit printing](https://developer.apple.com/documentation/appkit/printing).
Windows exposes enumeration, device capabilities and queue operations through
[Print Spooler APIs](https://learn.microsoft.com/en-us/windows/win32/printdocs/printing-and-print-spooler-functions).
The exact Windows rendering bridge needs a feasibility spike before promising
feature parity.

During planning on 2026-09-27, the system initially reported no configured
destinations. After the user connected a printer, an unsandboxed `lpstat -p -d -v`
reported **HP-ENVY-Photo-7800-series**, idle and enabled, as the system default.
Its device URI uses DNS-SD discovery for an IPP network printer.

An unsandboxed `lpoptions -p HP-ENVY-Photo-7800-series -l` query reported:

| Capability | Reported choices |
| --- | --- |
| Paper | Letter (default), A4, Legal, A5/A6, 4 × 6, 5 × 7, 8 × 10 and other sizes; several borderless variants; custom size entry |
| Paper source | Main (default), Photo, Auto |
| Media | Stationery (default), photographic glossy, specialty glossy/matte, lightweight stationery |
| Color | RGB (default), grayscale variants |
| Sides | One-sided (default), duplex long-edge and short-edge modes |
| Quality | Draft, Normal (default), High |
| Content optimization | Auto (default), photo, graphics, text, text-and-graphics |
| Scaling | Auto (default), auto-fit, fill, fit, none |

This is an observed queue/driver capability snapshot, not a test of every option
combination. Revalidate size/media/tray/borderless/duplex combinations at runtime.
Use this HP device for the first Linux physical acceptance tests. Discovery and
capability reading succeeded; no print job was submitted and measured output
validation remains pending. Windows and macOS still require their own native
adapter and hardware checks. The prototype retains clearly labeled generic demo
printers rather than pretending to be connected to this queue.

## Empty states, errors and job progress

- Discovering: keep preview usable; show progress and a refresh action.
- No printers: “No printers found. Add a printer in system settings, then refresh.”
  Offer Printer setup, Refresh and Save PDF. Disable physical Print.
- Service unavailable: identify that state separately from an empty printer list.
- Offline or stopped: preserve settings, display the actual reported state and
  offer Refresh / choose another destination. If queueing offline is supported,
  use an explicit **Queue for later** action; never imply immediate printing.
- Unknown capabilities: use system setup instead of inventing paper support.
- Preparing: progress and Cancel; keep the editor responsive.
- Submitted: “Sent to [printer]” with job ID when available; View queue and Cancel
  job where supported. Only show “Completed” if the backend reports completion.
- Submission interrupted with unknown outcome: check the queue before offering
  retry; never auto-resubmit and risk duplicate prints.
- Authentication, paper-out, paused queue, disappeared printer: retain settings
  and give the corresponding recoverable action. Cancellation of a spooled job
  may not prevent pages already being printed.

## Existing implementation and proposed boundaries

Useful existing code:

- `crates/emulsion-core/src/document.rs`: document resolution and canvas size.
- `crates/emulsion-core/src/creation.rs`: physical-size creation presets.
- `crates/emulsion-io/src/project_export.rs`: selected-page PDF, point sizing,
  TrimBox/BleedBox, outlined text/vector export and rendered-page fallback.
- `crates/emulsion-io/src/export_workflow.rs`: edited raster/RAW export workflow.
- `crates/emulsion-ui/src/editor/export_ui.rs`: existing export-control patterns.
- `packaging/flatpak/app.emulsion.Emulsion.yml`: sandbox packaging to validate.

Project PDF export is a starting point, not a print compositor: it currently
exports document-sized pages, and bleed expands canvas geometry. Audit content
beyond trim and rendering fidelity before reusing this for print. Reuse rendering
and PDF primitives while adding explicit physical-sheet placement. Keep the
existing artwork export behavior intact.

Proposed new modules (not created yet):

- `emulsion-core::print_layout`: pure source selection, page placement,
  imposition, physical geometry, validation and preview metadata.
- `emulsion-io::print_document`: snapshot rendering, sheet PDF construction,
  temporary spool ownership, cancellation and bounded memory use.
- `emulsion-print` crate: feature-gated platform adapters and asynchronous
  printer discovery, capabilities, preparation, submission and optional status.
- `emulsion-ui::print_dialog`: shared dialog opened from workspace actions,
  editor, project selection and photo library.

Data flow: `PrintSourceSnapshot + PrintSettings + PrinterCapabilities →
ResolvedPrintLayout → preview / spool document → backend`.
Model native preparation separately from direct submission; capabilities can be
unknown and job tracking can be unsupported. Capability results and previews
carry request generations so stale asynchronous results cannot overwrite a new
selection. Snapshot identity stays fixed during generation; editing afterwards
requires a visible refresh to include changes. Clean temporary output on success,
failure and cancellation after the backend no longer needs it. Render pages or
tiles incrementally rather than allocating an entire high-resolution poster.

## Delivery sequence and acceptance

1. **Shared core and all three platform adapters.** One canvas / selected design pages, preview,
   fit/fill/actual size, paper and orientation, margins, copies, device color and
   quality, PDF destination and no-printer states. Deliver Linux CUPS and Flatpak
   portal support, macOS AppKit, and Windows queue discovery plus a verified
   rendering/submission path together. Start with one physical-size proof on
   each platform, then connect the shared controls. This phase is complete only
   when real printer acceptance passes on Windows, macOS and Linux.
2. **Photo and layout tools.** Custom photo sizes, crop positioning, repeated
   photos, contact sheets, multiple-up, device duplex, saved presets, design
   marks and bleed; preserve vectors where supported.
3. **Larger jobs and additional media.** Tiled posters,
   frame/storyboard sources, stronger preflight and job recovery. Run platform
   feasibility checks in phase 1 so advanced layouts build on validated adapters.
4. **Managed color and production output.** Verified ICC output and soft proofing;
   scope press workflows separately after hardware/profile validation.

Release gates:

- Geometry checks for A4/Letter and custom sizes, portrait/landscape, asymmetric
  margins, fit/fill/actual size, cropping, exact points conversion and sheet counts.
- Render checks using text, vectors, masks, transparent layers, RAW adjustments,
  multi-page designs and high-resolution raster content; inspect resulting PDF
  boxes and compare composed preview with rendered spool pages.
- Fake backend cases: empty list, offline device, capability conflict, disappearing
  destination, cancellation, timeout, unknown submission outcome and stale replies.
- Portal integration checks: parent window, supported versions, setup cancellation,
  returned paper changes, token reuse/expiry and no duplicate ranges/scaling.
- Physical trials: USB and network queues, color and grayscale, supported
  borderless media and duplex, a measured 100 mm object at actual size, page order,
  cancellation and job recovery. Record device/driver and differences from preview.
- Check native Linux, AppImage and Flatpak separately, and native packaged Windows
  and macOS builds. Each OS must discover a configured printer, report supported
  paper options, produce a measured actual-size page, handle cancellation and
  explain a missing/offline printer. Verify USB and network paths across the
  platform/device matrix. PDF-only validation does not close physical-printer
  acceptance. Cross-compilation alone does not close Windows/macOS acceptance.

## Prototype coverage

The browser concept explores destination states, supported paper choices,
orientation, fit/fill, sheet layouts, copies, color, borderless and duplex.
Its landscape is sample vector artwork. Poster shows an assembly overview;
design shows a sample page. It does not perform document rendering, OS discovery,
PDF generation or printing. Advanced controls and exact final-size workflows are
specified here for the native implementation, not implied by the prototype.
