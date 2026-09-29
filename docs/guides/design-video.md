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

**Elements → Video playback setup…** provides runtime setup without adding a browser to Emulsion's download. On supported Arch and Debian/Ubuntu native installations, **Install playback packages…** invokes the system package manager through the operating system's administrator-authentication dialog. It installs only the listed WebKitGTK/GStreamer packages; cancellation or installation errors leave the design untouched. This action runs only when the user presses the installation button. If the authentication helper is unavailable, the dialog lists the packages for manual installation. The installer itself is not exercised by automated tests.

Windows setup opens Microsoft's [official WebView2 installer page](https://developer.microsoft.com/en-us/microsoft-edge/webview2/#download-section); macOS uses built-in WebKit and needs no separate runtime installer. The package lists follow [Arch's WebKitGTK package dependencies](https://archlinux.org/packages/extra/x86_64/webkit2gtk-4.1/) and [Ubuntu's WebKitGTK package](https://packages.ubuntu.com/jammy-updates/libwebkit2gtk-4.1-0). Other Linux distributions receive manual setup guidance. Flatpak setup directs users to update the shared runtime and does not attempt to install host packages.

Emulsion does not bundle Chromium or another browser engine. It uses an installed
system web runtime when playback starts:

| Platform | Playback runtime |
| --- | --- |
| macOS | Built-in WKWebView. |
| Windows | Microsoft Edge WebView2 Runtime. The runtime must be installed separately if unavailable on the machine. |
| Linux native/AppImage | WebKitGTK 4.1 and GStreamer video decoders. The small Emulsion adapter uses these system libraries. |
| Linux Flatpak | Shared GNOME 50 runtime supplies GTK3, WebKitGTK 4.1 and GStreamer decoders. Audio uses the scoped PulseAudio socket. |

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

The Flatpak manifest uses `org.gnome.Platform//50`, whose [platform definition](https://raw.githubusercontent.com/GNOME/gnome-build-meta/gnome-50/elements/sdk-platform.bst) includes GTK3 and WebKitGTK 4.1. Its [SDK base](https://raw.githubusercontent.com/GNOME/gnome-build-meta/gnome-50/elements/freedesktop-sdk.bst) uses freedesktop 25.08, including the matching Rust SDK extension. Emulsion embeds only the small capture adapter. The build fails early if the required development libraries are missing.

Linux sandbox acceptance compiled that adapter in GNOME SDK 50 and ran it in GNOME Platform 50 using the manifest's network, display, audio and GPU permissions. An H.264 MP4 played through the actual native media page/control adapter, producing 303 BGRA frames; pause, seek to 2500 ms while paused, resume, and trim completion passed. A separate official YouTube iframe test produced 1,282 frames, active uncorked audio, keyboard playback control and a resize from 640×360 to 800×450 without crashing. This verifies shared-runtime playback, not a complete packaged-app build or Windows/macOS behavior. GTK's offscreen Wayland capture still emits monitor/origin warnings; the player remains operational.

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
