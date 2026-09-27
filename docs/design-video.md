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
4. Enter presentation mode and choose **Play YouTube video** on the poster.
   YouTube's controls appear inside the design. Use **Stop video**, leave the
   presentation or change pages to close playback. Only one video plays at a time.

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
| Linux | WebKitGTK 4.1 and GStreamer video decoders. The small Emulsion adapter uses these system libraries. |

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

## Frame and playback constraints

- The visible player must be at least **200 × 200 pixels**. A smaller source
  object may remain on the page, but presentation must display it large enough
  before playback starts.
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
