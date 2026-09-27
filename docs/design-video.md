# YouTube in Design presentations

Design pages can contain editable YouTube video objects. The video plays inside
Emulsion's presentation canvas using YouTube's official embedded player. Emulsion
saves a video link and native poster artwork; it does not download or extract the
video stream.

## Authoring and presenting

1. Open **Elements → Embed YouTube video…** in Design and paste a YouTube link.
   Watch, `youtu.be`, shorts and embed URLs are supported, including start times
   such as `?t=90` or `?t=1m30s`.
2. Move or resize the inserted poster with the usual selection tools. Its frame,
   play symbol and label remain native editable objects.
3. Select the video group and use **Edit video link…** to change its source while
   preserving the poster and placement. **Keep poster only** removes the video
   association and keeps the artwork.
4. Choose **Present** above the canvas (or **Animate → Present pages**), then
   choose **Play YouTube video** on the poster.
   YouTube's controls appear inside the design. Use **Stop video**, leave the
   presentation or change pages to close playback. Only one video plays at a time.

Slides advance manually by default. Use Previous/Next, the left/right arrows,
Space/Shift+Space or Page Down/Page Up. **Fullscreen** expands the presentation;
**Exit presentation** or Escape returns to editing. Optional **Auto advance**
uses each page's animation duration and waits while an embedded video is open.
Close the video with **Stop video** to resume automatic advance. While the video
has focus, its own playback keys take precedence over slide navigation; Escape
first closes the player.

Insertion, changing the link and detaching the video are undoable. Duplicate,
copy/paste and native project saves preserve the video association. Opening or
editing a saved project does not start a browser or contact YouTube. A recipient
can continue editing the poster and link without an internet connection.

## Platforms and application size

Emulsion does not bundle Chromium or another browser engine. It uses an installed
system web runtime when playback starts:

| Platform | Playback runtime |
| --- | --- |
| macOS | Built-in WKWebView. |
| Windows | Microsoft Edge WebView2 Runtime. The runtime must be installed separately if unavailable on the machine. |
| Linux native/AppImage | WebKitGTK 4.1 and GStreamer video decoders. The small Emulsion adapter uses these system libraries. |
| Current Linux Flatpak | Playback unavailable: the selected freedesktop 24.08 runtime has GTK3 but not WebKitGTK 4.1. Link/poster authoring and project sharing remain available. |

On Arch-based Linux the runtime packages are `webkit2gtk-4.1`,
`gst-plugins-good` and `gst-libav`. On Ubuntu the corresponding packages are
`libwebkit2gtk-4.1-0`, `gstreamer1.0-plugins-good` and `gstreamer1.0-libav`.
Distribution versions and decoder availability can differ. Building the Linux
adapter also requires the WebKitGTK development headers. Missing runtime or
codec support produces a playback error; the document remains editable.

System runtimes consume their own disk space. This approach avoids adding a
complete browser engine to Emulsion's application download. Runtime updates are
provided by the operating system or its package manager. Windows and macOS still
require validation on their respective machines; a successful Linux test does
not establish playback compatibility on those systems.

The checked-in Flatpak manifest uses `org.freedesktop.Platform//24.08`; its
[SDK source tree](https://gitlab.com/freedesktop-sdk/freedesktop-sdk/-/tree/release/24.08/elements)
contains GTK3 but no WebKitGTK component. The sandbox cannot use a host-installed
WebKit library, and the manifest does not build one into the application. The
helper therefore remains unavailable in that build. The existing manifest also
has no audio socket permission. Supporting playback there requires a separately
validated shared runtime with the compatible GTK3/WebKitGTK 4.1 ABI and codecs,
plus scoped audio access. No browser bundle, extra host filesystem access or
host-process escape has been added for playback. Use the native/AppImage package
when in-app video is required. Flatpak itself is not installed on the verification
machine; this is a manifest and upstream runtime-source audit, not a Flatpak
playback test.

## Frame and playback constraints

- The visible player must be at least **200 × 200 pixels**. A smaller source
  object may remain on the page, but presentation must display it large enough
  before playback starts. The whole player must fit inside the presentation canvas.
- During playback, the rectangular web player appears above the page artwork.
  Arbitrary masks and artwork layered over the live video are not composited
  into the web player; static exports still follow the poster's normal layer order.
- Linked video frames support translation and axis-aligned resizing. Rotation,
  shear, flips and arbitrary boundary reshaping are rejected while linked.
  Detach the video first to transform its poster without these restrictions.
- Hidden video objects and hidden ancestors do not play. Locked objects remain
  playable; their links must be unlocked before editing or detaching.
- Playback needs internet access. YouTube can restrict embedding, geographic
  access, age-restricted content or individual videos. The official player may
  display consent, advertising or an unavailable-video message.
- The browser is a constrained presentation player, not an arbitrary website or
  HTML embed feature. A private loopback page supplies the official
  `youtube-nocookie.com` iframe. No executable HTML is stored in the project.

## Sharing and exports

Share the native `.emu` project, or an editable template package, to retain the
video link and poster. Recipients need a supported runtime and internet access to
play it. The saved project contains no cached video media.

PNG/JPEG, SVG, PDF and other static exports contain the poster artwork. Animated
exports render authored Design motion; they do not capture YouTube playback or
audio. Offline video embedding, local video/audio tracks, trimming, compositing
video into exports and interactive HTML export are separate capabilities.

## Verification

Core regression coverage checks URL validation, start times, visibility, locks,
unsupported transforms, Undo, deletion, duplication, clipboard remapping and
branch-merge remapping. IO coverage checks native project round trips and the
loopback server's exact route, host checks and shutdown lifetime. UI and runtime
checks are separate from these model tests.

Linux runtime verification on Wayland (September 27, 2026) exercised the compiled
adapter against both a local H.264/AAC test clip and YouTube's official embedded
sample video (`M7lc1UVf-VE`). The sample produced decoded moving video and captions
at about 29–30 frames per second, with an active, unmuted system audio stream.
Pointer input started playback; keyboard input paused and resumed it; resizing
changed output from 640 × 360 to 800 × 450. Captured frames stayed identical while
paused. Ending the helper removed its audio stream and child processes.

The test machine lacked the required GStreamer codecs. Verification used official
Arch codec packages extracted into a temporary directory with a process-scoped
`GST_PLUGIN_PATH`; no system packages or desktop settings were changed. Without
those codecs the adapter reports a missing-decoder error. The stripped Linux
adapter is about 31 KiB; WebKit and GStreamer remain external system dependencies.
The application's complete installer-size comparison is a separate build check.

The Linux adapter transports bounded BGRA frames from a separate GTK process to
GPUI. A GTK toplevel with a GDK offscreen drawing surface lets WebKit start media
without creating a desktop window. Plain `GtkOffscreenWindow` suspends media
loading and cannot be substituted. Some WebKitGTK/GDK versions log monitor-lookup
diagnostics for this offscreen surface; these did not interrupt the verified
playback. The adapter caps output at 1,920 pixels per side and 2,073,600 pixels,
keeps only the newest frame and stops its process group when playback closes.
A forced 2× display-scale check normalized a 1,280 × 720 WebKit snapshot to the
negotiated 640 × 360 output without cropping or double-applying the Cairo scale.
The 44-second playback/resize check emitted four startup monitor diagnostics
(568 bytes total), with no per-frame diagnostic growth.
These checks validate this Linux runtime, not every distribution or codec stack.
