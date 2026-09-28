# Printing

Open a document and choose **File → Print…**, or press **Ctrl+P** on Windows/Linux
and **Cmd+P** on macOS. The dialog prints a snapshot of the edited document,
including unsaved changes. Printing does not flatten or resize your project.

Choose an installed printer or **Save PDF…**. The dialog reads printer paper
sizes and minimum margins. Borderless variants appear in the paper list when
reported by the driver. Set **Extra margin** to zero to use the entire available
printable area; selecting a borderless size alone does not remove an extra margin.
Refresh reloads queues and capabilities. A missing printer does not silently
redirect the job to another device.

The dialog matches the active page to an available paper size and orientation
when possible (including whole-pixel rounding at 300 PPI). Manual paper choices
remain yours. Preview shows the source size, stored PPI, size on paper and scale;
the sheet dimensions follow the currently displayed preview page.

**Save PDF → Document page sizes · no scaling** is the PDF default. It preserves
each selected page's physical dimensions and orientation, including mixed-size
projects. Paper, placement, scale and margin controls are hidden for this layout;
no extra margin or fit scaling is applied. Choose a sheet layout for imposition,
contact sheets or poster tiling. Switching to a physical printer restores sheet
layout, because the device must use paper it actually supports. Fit remains the
initial device placement; use Actual size at 100% when dimensions must be exact,
and check any cropping warning against the printer's margins.

New bundled print templates have 300 PPI metadata: A4 flyers, 3.5 × 2 inch
business cards, 18 × 24 inch posters and 5 × 7 inch invitations. Existing saved
files keep their original resolution. Screen-oriented designs keep 72 PPI.

MCP `preview_print_job` supports `layout: "document"` without a printer and
returns `width_mm` / `height_mm` for the requested sheet. This layout is rejected
for device submission. The Flatpak system dialog receives the selected orientation.

## Layout controls

- Current page/canvas, all document pages, or a range such as `1-3, 5`.
- One page per sheet; a contact sheet of selected document pages; a grid repeating
  the first selected image; or a tiled poster of one selected source page. Contact
  and repeat grids have 1–20 rows/columns and a 0–100 mm gutter (default 2 × 3, 5 mm).
- Fit keeps the whole image. Fill crops it to the available area. Actual size uses
  the document's pixel dimensions and resolution; Scale changes that physical
  size. The screen's canvas zoom has no effect on print dimensions.
- Custom artwork width/height define a finished box in millimeters, independently
  of paper size. Fit adds paper around the image where proportions differ; Fill
  crops to the box. Actual size retains the chosen source scale within the box.
  Oversized boxes are rejected instead of silently reduced.
- Horizontal/vertical position (0–100%) moves the image within its box, including
  the crop in Fill mode. Zero aligns left/top, 100 aligns right/bottom; Center
  artwork / crop restores both to 50%.
- Poster uses actual size × Scale, or fits proportionally within custom artwork
  dimensions, with a configurable overlap. Tiles are ordered
  left to right, then top to bottom. Each tile can be inspected in the preview.
- Portrait/landscape, extra margins, copies, color/grayscale, and available device
  choices for media, source tray, quality and duplex.

The preview and print output use the same physical sheet composition. PDF export
saves one set of the composed sheets; copies are a physical printer setting.
Transparent regions use white paper. The dialog reports cropping and an advisory
resolution estimate for documents containing raster content. Paper and driver
color behavior can still differ from a monitor; this is not a color proof.

Simple shadows are emitted separately from sharp foreground text and shapes.
Text outlines and inside/outside shape borders retain vector geometry.
The document's vector paths and outlined text are retained in color PDF output
where the existing document exporter supports them. Unsupported document effects
use that exporter's rendered fallback. Grayscale output and the Windows print
bridge render the composed sheet at 300 PPI. Sheets exceeding the render budget
produce an error rather than silently lowering resolution.

## Bleed, crop marks and presets

Bleed adds up to 20 mm outside the finished trim. It reveals artwork already
extending beyond the document edge; it does not stretch pixels or generate new
artwork. Uncovered bleed remains paper white. Check the preview and extend
backgrounds or images past the page edge in the editor when needed.

Crop marks are 5 mm long, separated from the bleed edge by 2 mm. Sheet layouts
reserve space for both bleed and marks inside each cell's printable area. Custom
artwork must fit with that space. Document-size PDF preserves the original trim
size and enlarges the PDF sheet to accommodate bleed/marks. Single-artwork PDFs
include TrimBox and BleedBox; multi-up output draws marks for each item. Poster
tiles use overlap instead of these design trim controls; a poster layout with
bleed or crop marks is rejected with "Bleed and crop marks require a single-page,
contact or repeat layout".

**Saved layout presets** loads a named local layout. Enter a name and choose
**Save preset** to create or replace it; **Delete preset** removes that name.
Presets retain artwork sizing, layout, placement, margins, crop, grid, bleed,
marks, orientation and grayscale preferences. They never save the destination,
page range, copies, media, tray, duplex or quality. Loading revalidates paper
against the current printer and keeps its actual hardware margins. Unavailable
paper is reported rather than selected silently. Presets are stored atomically in
`print-presets.json` in Emulsion's application data directory.

MCP has the same creative controls through `artwork_width_mm` /
`artwork_height_mm` (provide both), `rows`, `columns`, `gutter_mm`,
`crop_x_percent`, `crop_y_percent`, `bleed_mm` and `crop_marks`.
`list_print_presets`, `save_print_preset`, and `delete_print_preset` manage local
presets. Preview/submission can load one using `preset_name`; explicit options
override it. A preset incompatible with the selected destination reports an error
so the caller can supply explicit settings. These tools do not install drivers.

## Platform connections

| Platform | Connection | Requirements and limits |
| --- | --- | --- |
| Linux native / AppImage | CUPS destination, media and job APIs | Loads `libcups.so.2` at runtime. A missing library/service leaves PDF output available. Uses configured and CUPS-discovered destinations. |
| Linux Flatpak | XDG Print portal | Choose a printer and paper in the system dialog, review the updated Emulsion preview, then Print. A working desktop Print portal is required. |
| macOS | System CUPS print service | Uses the OS-provided `/usr/lib/libcups.2.dylib` and configured macOS printers. Printer setup opens System Settings. |
| Windows | Installed drivers through .NET `PrintDocument` / GDI | Uses bundled helper code through Windows PowerShell and System.Drawing. No raw PDF is sent to an arbitrary printer. Additional driver-specific media properties are configured in Windows printer settings. |

Linux's **Printer setup…** opens the local CUPS printer page. Windows/macOS open
system printer settings. Configure drivers and newly connected printers there,
then Refresh in Emulsion. Emulsion does not change the system default printer.

For the portal, leave All pages, normal page order, 1 page per sheet and 100% scale
in the system dialog: source page selection and sheet composition happen in
Emulsion. Copies, duplex and device options belong to the system dialog. The
portal may show an independent window on some desktops. Selecting the system
print destination again reopens device setup. Backend cancellation/errors retain
the source document. An expired portal token may require choosing the printer
again; it is never automatically resubmitted.

After native submission, Emulsion reports acceptance by the queue, not physical
completion. CUPS supplies a job ID. Use the operating-system queue to inspect or
cancel an accepted job. Closing the dialog requests cancellation of preparation;
it cannot guarantee stopping pages already submitted to the printer. If submission
fails with an unknown outcome, inspect the queue before retrying.

## Validation and current scope

The creative print-controls follow-up passed 17 layout/render/preset tests,
5 MCP print tests, 3 native dialog tests and the MCP catalog registration check
in an isolated checkout. Tests cover exact custom trim dimensions, crop pixels,
grid ordering/pagination, preset replacement/deletion and device revalidation,
authored vector bleed, PDF page boxes, and unchanged source snapshots. A separate
PDF renderer and `pdfinfo -box` also checked the 100 × 60 mm proof. Clippy found
no warnings in the print changes; strict package-wide lint remains blocked by
existing warnings in other modules.

Generate the same proof without sending a print job:

```sh
cargo run --locked -p emulsion-io --example print_layout_proof -- /tmp/emulsion-print-proof
```

This writes document-size and A4 repeat-sheet PDFs plus matching PNG previews.
Print the A4 proof at **100% / actual size** and measure the trim rectangle:
**100 × 60 mm**, with **3 mm** of authored bleed. Physical measurement and
Windows/macOS printer execution remain separate acceptance checks.


The September 2026 template/print update was checked in an isolated checkout:
316 core tests, 265 I/O tests, 195 MCP tests and 13 native print/text UI tests.
One unrelated media-server shutdown test failed during the broad run and passed
when rerun alone. Checks include mixed-size PDF MediaBoxes, paper/orientation
matching, native template round trips, scalable export for all 154 supplied
starters, and pixel-based tracking at 20, 200 and 1600 px font sizes. Scoped
library/test Clippy and the native preview example pass with warnings denied.
Workspace-wide formatting and all-target Clippy still encounter pre-existing
issues outside this update. No physical print job was submitted for these checks.

The Linux adapter has been exercised against an HP ENVY Photo 7800 series for
queue discovery, default selection, paper dimensions/margins and supported
options, including validation without job submission. Six printing tests and a
native dialog test passed, covering geometry, page ranges, preview composition,
grayscale, atomic/canceled PDF output and dialog validation. The native dialog
test used an isolated source snapshot because unrelated work was changing in the
shared checkout. A 100 mm proof was generated and its PDF page bounds inspected.
These checks do not constitute a physical print trial.

The Windows and macOS adapter sources have isolated Rust cross-target compilation
checks. Native driver execution, physical measurements and packaging acceptance
on Windows/macOS are still required. Full Windows cross-compilation on the Linux
work machine requires a MinGW C toolchain for existing dependencies. The Flatpak
portal path also requires runtime acceptance on a desktop with a Print backend.

This implementation covers open document/project pages. Printing a photo-library
selection directly, frame/storyboard picking, contact-sheet labels,
app-managed ICC output and press-specific PDF standards remain
follow-up work from the [design plan](../specs/print-dialog-plan.md). Linked video objects
print their stored poster artwork; video/audio playback is not printable.

For a read-only printer probe, run:

```sh
cargo run --locked -p emulsion-io --example print_probe
```

Adding an output PDF path also generates a 100 mm vector proof and a PNG preview.
The probe never submits a print job.
