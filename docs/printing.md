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

## Layout controls

- Current page/canvas, all document pages, or a range such as `1-3, 5`.
- One page per sheet; a 2 × 3 contact sheet of selected document pages; six copies
  of the first selected image; or a tiled poster of one selected source page.
- Fit keeps the whole image. Fill crops it to the available area. Actual size uses
  the document's pixel dimensions and resolution; Scale changes that physical
  size. The screen's canvas zoom has no effect on print dimensions.
- Poster uses actual size × Scale and a configurable overlap. Tiles are ordered
  left to right, then top to bottom. Each tile can be inspected in the preview.
- Portrait/landscape, extra margins, copies, color/grayscale, and available device
  choices for media, source tray, quality and duplex.

The preview and print output use the same physical sheet composition. PDF export
saves one set of the composed sheets; copies are a physical printer setting.
Transparent regions use white paper. The dialog reports cropping and an advisory
resolution estimate for documents containing raster content. Paper and driver
color behavior can still differ from a monitor; this is not a color proof.

The document's vector paths and outlined text are retained in color PDF output
where the existing document exporter supports them. Unsupported document effects
use that exporter's rendered fallback. Grayscale output and the Windows print
bridge render the composed sheet at 300 PPI. Sheets exceeding the render budget
produce an error rather than silently lowering resolution.

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

The Linux adapter has been exercised against an HP ENVY Photo 7800 series for
queue discovery, default selection, paper dimensions/margins and supported
options. Automated checks cover geometry, page ranges, preview composition,
grayscale and atomic/canceled PDF output. A 100 mm proof was generated and its
PDF page bounds inspected. These checks do not constitute a physical print trial.

The Windows and macOS adapter sources have isolated Rust cross-target compilation
checks. Native driver execution, physical measurements and packaging acceptance
on Windows/macOS are still required. Full Windows cross-compilation on the Linux
work machine requires a MinGW C toolchain for existing dependencies. The Flatpak
portal path also requires runtime acceptance on a desktop with a Print backend.

This implementation covers open document/project pages. Printing a photo-library
selection directly, frame/storyboard picking, adjustable contact-sheet grids,
custom artwork width/height fields, crop repositioning, saved presets, crop marks,
bleed controls, app-managed ICC output and press-specific PDF standards remain
follow-up work from the [design plan](print-dialog-plan.md). Linked video objects
print their stored poster artwork; video/audio playback is not printable.

For a read-only printer probe, run:

```sh
cargo run --locked -p emulsion-io --example print_probe
```

Adding an output PDF path also generates a 100 mm vector proof and a PNG preview.
The probe never submits a print job.
