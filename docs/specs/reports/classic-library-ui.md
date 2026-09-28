# Classic-style Library and Develop UI

The supplied eight screenshots informed the layout and control density. Emulsion's
existing GPUI Kit components and Library commands implement the interface; the
reference photographs, branding and application assets are not bundled.

## Delivered UI

- Library/Develop module header and controls use Emulsion’s application palette.
  The layout follows the reference; thumbnail cards, colors and accents follow the app.
- Navigator in both modules. Library has Catalog, a virtualized folder list,
  Collections and attribute filters. The folder list is cached by catalog revision
  and navigates catalog references without rescanning original files on each frame.
- A Library contact sheet with application-themed rounded selection frames and contained images,
  preserving the complete photograph instead of cropping it to fill each card.
- Histogram in Library and Develop. Develop keeps its overlapping RGB histogram,
  editing toolbar and Sync/Reset footer outside the scrolling adjustments.
- GPUI Kit icon buttons select Crop, Heal, Linear Gradient, Radial Gradient, Brush
  and Guided Transform. Their existing canvas gestures and saved edits remain shared.
- Dense adjustment rows place the label, native GPUI slider, numeric entry and reset
  together. Basic groups White Balance, Tone, Presence and Effects; detail controls
  remain in Detail. Tone Curve has a larger graph with its histogram behind the curve.
- Application-themed filmstrip frames preserve the complete image. Import/export and selection
  status sit above the filmstrip. Develop's image toolbar provides Before/After,
  soft-proof/display controls and Done for active canvas tools.
- Library retains Quick Develop, Metadata, Keywording, recipe browsing and export.
  Recipe browsing temporarily collapses Quick Develop and the histogram to leave
  usable space on shorter windows.

The new components use the project's vendored GPUI Kit. The upstream
[Button](https://gpui-kit.com/component/button/) and
[Slider](https://gpui-kit.com/component/slider/) documentation informed component
styling; layout retains GPUI Kit's resizable panels and virtual lists.

## Automation

`get_library.layout` includes `canvas_tool` and `develop_section` alongside existing
layout state. `set_library_view.canvas_tool` accepts `none`, `brush`, `erase`, `heal`,
`clone`, `crop`, `straighten`, `perspective`, `radial` and `linear`. Use
`mode: "develop"` to display the tool. Invalid tool names fail before view mutation.
The numeric inspection IDs follow that same ordered list, starting at zero.

## Scope

The original layout update did not include HDR merge or a profile-thumbnail browser.
These are implemented in the subsequent [HDR/profile update](library-hdr-profiles-validation.md),
along with application theming, orientation controls and smooth point curves.
Map, Book, Slideshow and Web modules are outside this workspace. Rendering limitations from the
[desktop validation report](library-develop-desktop-validation.md) still apply.

Layout assertions cover 1440 × 900 and 1280 × 720, including histogram/tool/footer
visibility while the adjustment panel scrolls. These headless checks are not a
pixel-for-pixel screenshot comparison or a full visual acceptance review.

## Validation results

- 19 UI tests passed: Library navigation, selection, checkbox behavior, metadata,
  RAW save/export, presets, recipe browsing, bounded thumbnail loading and the new
  fixed-panel layout checks. The local-camera test is ignored in the ordinary suite.
- 5 MCP Library contract tests passed.
- The expanded layout/tool test passed separately, including MCP radial-tool
  selection, atomic rejection of an invalid tool request and the Done button.
- The opt-in Nikon folder test passed in 9.01 seconds: thumbnails loaded and each
  of the supplied folder's 12 RAW files could be selected and inspected.

Validation uses an isolated source snapshot, Cargo target and temporary application
data directories, excluding unrelated work in progress. Original photos and the
running application are left untouched.

The offline release build passed without warnings. The separate package is
`target/classic-library-ui-appimage/Emulsion-0.0.3-x86_64.AppImage` (54 MiB).
Extraction-mode `--version` returned `emulsion 0.0.3` without opening an editor window.
SHA-256: `a2ded632c564f566a9a3e8019e3af52dbe816fdbdd524bb092f60e80a4cfe8dc`.

## Rotation controls beside the photo

The preview toolbar has 90° left/right icon buttons and a Straighten toggle.
Straighten reveals a −45° to +45° slider (0.1° steps), exact numeric entry, an
alignment grid and Done. Reset rotation restores the camera orientation and
zeroes the fine angle while retaining exposure, crop and other development edits.
The controls use the existing sidecar, thumbnail/export and undo pipeline.

Rotation and angle changes leave the 100% detail crop and fit the whole photo,
including square images whose dimensions do not change on a quarter turn. A
slider drag is one undo step. The Transform section retains crop, perspective
and line-straightening tools, without duplicating the orientation buttons.

Rotation validation: the final native UI test build passed in an isolated checkout
of `b17e83c` plus these changes, excluding concurrent Diagram work. All six targeted
rotation, Library layout, RAW save/export and MCP development tests passed.
Coverage includes dragging to a nonzero angle, one undo step per drag, leaving
detail view, resetting only rotation, undo/restored sidecar values, rotated
thumbnail dimensions and unchanged original bytes. Formatting and whitespace
checks passed. No running-app screenshot comparison was performed.
