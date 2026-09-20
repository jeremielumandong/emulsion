# Making GPU brushes faster

The current GPU brush hook accelerates only final composition. CPU code still
builds every dab in `emulsion-raster/src/paint.rs::Stroke::stamp`, including up to
16 antialiasing samples per edge pixel. Moving only the cheap final blend to GPU
does not remove this work.

The original full-tile hook uploaded 24 bytes of accumulated paint, 8 bytes of
base color, and 4 bytes of selection coverage per pixel, then downloaded 8 bytes.
Four 256×256 tiles therefore transferred 11 MiB per update, excluding additional
CPU copies and GPUI image uploads. This also paid for pixels with no paint.

The transfer optimization in `emulsion-gpu/src/paint.rs` packs only pixels with
nonzero stroke coverage for sparse batches and reconstructs the result over the
original base. Dense batches retain contiguous packing to avoid scatter overhead.
Sparse uploads omit the unused thickness channel. Dense uploads retain the
original six-float layout to avoid extra per-pixel repacking.
Restoring the base, rather than the previous preview, matters when end taper or
QuickShape removes earlier paint. This remains an experimental final-blend path;
it does not move dab generation to GPU or eliminate synchronous readback.

On Linux/Intel Iris Plus, the final isolated release benchmark measured:

| Workload | CPU dab generation | CPU composition | GPU composition |
| --- | ---: | ---: | ---: |
| Sparse, four tiles | 0.49 ms | 1.34 ms | 2.61 ms |
| Dense, four tiles | 2.63 ms | 5.69 ms | 9.23 ms |
| Dense, sixteen tiles | 10.59 ms | 18.47 ms | 45.72 ms |

The earlier full-tile GPU baseline was 10.92, 10.14, and 47.66 ms respectively.
Sparse GPU composition improved about 4×, while CPU composition remains faster
in every measured case. Dense differences are small enough to treat cautiously
across separate runs. Keep the experimental opt-in; do not infer a whole-editor
speedup from this result. Numbers average six warm iterations, include packing,
transfer and tile replacement, and exclude GPUI presentation. CPU dab generation
is listed separately and is still required by both paths.

Hardware and Mesa software parity tests cover ordinary blending, erasing,
clipping, alpha lock, sparse reconstruction, all-zero coverage restoration and
invalid sparse coverage. Strict GPU-crate Clippy checks pass.

## Next implementation: persistent dab accumulation

Start with dry procedural color brushes and erasers, retaining the current UI
contract and CPU document/history storage:

1. Keep smoothing, pressure, spacing, symmetry and randomness on CPU. Record
   resolved dab commands containing geometry, flow and color, in order.
2. Allocate paint accumulation, base and clipping buffers once per touched tile.
   Upload base and clipping once; upload only newly resolved dabs on later updates.
3. Assign GPU work per tile pixel and apply intersecting dabs in stroke order.
   Preserve the existing falloff and edge sampling; do not approximate brush
   appearance merely to improve timing.
4. Batch pending dabs for each preview rather than dispatching per dab. With
   frame scheduling, retain all input samples but submit at most one preview batch
   per frame; flush the final batch on pointer-up. Blend accumulated paint with
   the base on GPU and download only updated result tiles once per render. Reuse
   bounded staging/readback buffers across updates.
5. Retain the resolved dab journal until commit. On GPU failure, reconstruct CPU
   accumulation from the same base, clipping and commands, then continue editing.

This first stage still reads back RGBA16 tiles for each preview. It removes CPU
per-dab pixel math and repeated paint-buffer uploads without changing the
editor's synchronous `Stroke::render -> Raster -> ReplacePixels` contract.

Do not use this first stage for wet/smudge/clone, textured tips, grain/relief, or
healing. Wet pickup reads accumulated paint synchronously; healing needs CPU
coverage. These require explicit materialization or separate GPU kernels.
Replay and end taper must reset GPU accumulation and dirty previously touched
tiles so removed preview marks are restored. Existing history transactions can
still combine intermediate preview replacements into one undo step.

## Later: preview directly from GPU textures

The larger gain is to keep the preview on GPU and download changed tiles only
for commit, eviction, or CPU consumers. That needs a rendering API change:
GPUI's `Window::paint_image` accepts CPU-backed `RenderImage` frames and uploads
through its atlas. Its `paint_surface` API is macOS-only; it is not a portable
wgpu texture bridge. The compute backend also currently owns a separate device.

A portable solution needs shared rendering resources or explicit texture
interoperability for Linux, Windows, and macOS, with synchronization and device
loss handling. Preview must participate at the correct layer position, including
masks and blend modes; drawing an overlay above the finished document would be
incorrect. Commits must preserve revision checks and prevent stale asynchronous
readbacks from overwriting later edits.

## Evidence and acceptance criteria

Measure complete point-plus-render time, not just kernel execution. Include CPU
dab generation, packing, allocations, uploads, dispatch, downloads, tile
replacement and eventually GPUI presentation. Test sparse and dense strokes,
large overlapping dabs, long strokes, small brushes, selection and alpha lock,
taper/QuickShape restoration, cancel/undo/redo, and device-loss CPU replay.
Enable automatic GPU routing only for workloads with a repeatable end-to-end win.

The wgpu 29 [queue performance documentation](https://docs.rs/wgpu/29.0.0/wgpu/struct.Queue.html#method.write_buffer)
confirms that ordinary writes allocate temporary staging memory on native
platforms and describes reusable staging as an alternative. Persistent buffers
and batching address that overhead; they do not by themselves remove readback.
GPU painting research also uses GPU tile copies to avoid frequent CPU transfers
for undo: [Baxter's dissertation](https://www.billbaxter.com/dissertation/Baxter-dissertation.pdf).
