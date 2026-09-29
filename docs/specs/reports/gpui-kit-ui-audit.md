# GPUI Kit UI audit — 2026-09-28

## Version and scope

The [official 0.7.0 release](https://github.com/longbridge/gpui-kit/releases/tag/v0.7.0)
is newer than Emulsion's pinned GPUI Kit 0.6.4 / GPUI 0.3.5. The site's unversioned
release index was still reporting 0.6.6 during this audit; the GitHub release and
published crate manifests were used to confirm 0.7.0. This change imports the new
Base Toolbar behavior and improves application composition. It does **not** label
the entire framework as upgraded.

The 0.7.0 crate manifests require GPUI 0.3.7, including a new
`gpui-pre-reqwest-client` dependency. Replacing only our top-level version pin
would not preserve our matching vendored family or rendering patches.

## Findings and implemented improvements

| Surface | Finding in Emulsion | Change |
| --- | --- | --- |
| Home file controls | A long row of individual buttons offers no arrow-key traversal or command grouping. | Separate document-type and view/sort toolbars. Left/Right moves focus, wraps within the group, and retains explicit activation. Controls wrap at narrow widths. |
| Template creation | Templates/Blank canvas are related choices rendered as independent controls. | Use the same keyboard toolbar behavior for the creation-mode controls. |
| Cloud controls | Provider filters and actions lack a semantic command container. | Named toolbar groups them and provides bounded arrow-key navigation. |
| Theme | Kit `accent` was mapped to the solid brand color although ghost controls use it for hover. Several normal and hovered surfaces were identical. | Use a neutral hover surface, a tinted selection surface, readable foregrounds, and preserve solid primary actions. Colors still project into component tokens and Base. |
| Local empty states | A plain sentence conflates first use, an empty project, Trash, and filtered results. | Distinguish each case; offer Open/New for empty work and Clear filters for no results. Clearing filters keeps project/Trash scope. |
| Cloud empty states | A single message conflates no connected drive, filtering, and an in-flight check. | Contextual connection, clear-filter, or local-browser actions. Suppress premature empty-state actions during a cloud check. |
| Cloud feedback | A disabled Refresh button does not explain current activity; empty results still render 0–0 pagination. | Show Syncing while busy and show file pagination only when more than one page exists. |
| Icon actions | Home grid/list controls have accessible names but no pointer explanation. | Add matching tooltips. |

No new account, network fetch during render, JavaScript runtime, or background
animation is introduced. Home continues to page by 48 and template previews by
12. The renderer, display scaling, saved projects, sync transport, and document
formats are unchanged by this UI pass.

## Backport provenance

The Base Toolbar implementation and its upstream tests come from the
checksum-verified gpui-base 0.7.0 crate (Apache-2.0). Application-owned layout
keeps Emulsion's appearance rather than importing the styled toolbar's fixed
height. Package version 0.6.4 remains truthful; the patch is documented in
[gpui-base changes](../../../vendor/gpui/gpui-base/EMULSION_CHANGES.md) and shipped in
the license notices.

## Next improvements, in priority order

1. **Full matched 0.7 migration.** Import verified archives for the entire GPUI
   dependency family; reconcile the inventory, lockfile, notices and local patch
   set. Compare each external-texture, software-rendering, list-height and layout
   patch with 0.3.7 before porting. Migrate window Root/overlay ownership in main,
   presenter windows, benchmarks and test harnesses together. Use Theme::update
   for atomic theme projection after the migration. Verify menu dismissal/focus,
   scrolling, input cursor idling, popovers and accessibility against upstream's
   changed behavior.
2. **Editor toolbar adoption.** Extend semantic groups to Design/Diagram and
   Photo/Paint option rows. Verify arrows stay with text/number inputs and sliders
   where expected; do not indiscriminately capture canvas shortcuts. Preserve
   per-tool state and selection while controls move into overflow layouts.
3. **Gallery navigation at scale.** Evaluate numbered/jump pagination and
   keyboard selection against the existing thousand-file fixtures. A virtual
   gallery needs thumbnail lifetime and selection/focus behavior established
   before replacing bounded paging. Virtualization already exists for Layers;
   it is not a new benefit to claim merely from updating the package.
4. **Consistent forms and progress.** Adopt shared field groups for units,
   validation and errors in export/print/new-document settings. Add byte-level
   transfer progress only after the sync engine supplies it; current busy and
   saved-version states must not be presented as a percentage.

Full migration acceptance requires Linux runtime review plus macOS/Windows
renderer checks. Existing Emulsion patches include platform-specific surface
sharing and frame-completion behavior; a successful Linux compile alone does
not validate those paths. Performance claims need measured idle and interaction
frame timings, not assumptions from new component availability.

## Validation

- Vendor inventory, lock/patch consistency and preserved licenses: passed.
- License-staging regression checks: 3 passed.
- Renderer policy checks: 10 passed.
- Focused application interaction tests: 30 passed. Coverage includes Home and
  cloud thousand-file fixtures, type-filter keyboard activation, focus wrapping
  with 150 external controls, skipping disabled controls, a 680px window, theme
  projection, project-preserving filter reset, cloud empty states, and all seven
  new-document regressions. Tested files match the main workspace byte for byte.
- The first shared-tree build was blocked by concurrent, unrelated IO importer
  edits. A clean HEAD snapshot with only this UI patch is used for independent
  verification; those importer sources are not modified by this work. A second
  shared-tree check reached the UI crate but remained blocked in concurrent
  `batch/advanced.rs` and smart-source integration code. No new AppImage is
  installed by this pass.
- No physical-window visual review or non-Linux runtime validation is implied.
