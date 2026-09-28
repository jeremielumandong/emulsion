# Responsive layout performance

Ordinary Design edits reflow only affected responsive frame roots. A node, sibling order, or frame settings change marks its outermost responsive ancestor; nested container breakpoints are resolved through that ancestor. Inserting, removing, or reparenting frames also refreshes newly independent roots.

Canvas size, resolution, portable fonts, variable bindings, diagram synchronization, and movement links retain the full layout pass because their dependencies can cross frame boundaries. All frame settings and the resulting document are still validated. Locked dependent artwork can reject the entire operation, and Undo retains the complete previous document.

Geometry transforms prepare copies of affected nodes rather than cloning the whole document for each child. Nodes are published only after every transform and mask adjustment succeeds, so a later unsupported transform cannot leave earlier objects partly changed.

Run the bounded release workload with:

```sh
cargo run --locked --release -p emulsion-io --example design_layout_bench -- 1000
```

The workload contains 1,000 editable objects in 10 responsive grids and measures repeated spacing changes in one frame. It emits median, p95, and maximum elapsed milliseconds, validates the result, and checks repeated layout is idempotent. The optional object count accepts 10–4,000. Results depend on the machine and concurrent build/render activity; this is a reproducible local workload, not a guarantee of interaction speed in every document.

Core regressions compare incremental results against a full pass for nested frames, container breakpoints, variables, movement links, canvas resizing, reorder/insert/remove operations, and protected descendants. Transform regressions cover linked masks and failure after a preceding candidate has already been prepared.

## Linux measurement, 2026-09-28

Same release workload on this workspace machine, with concurrent compiler work:

| 1,000 objects / 10 frames / 20 measured edits | Median | p95 / maximum |
| --- | ---: | ---: |
| Before affected-root and candidate-transform changes | 40.47 ms | 61.92 ms |
| After changes | 16.29 ms | 17.84 ms |

Both runs validated the resulting document and repeated-layout idempotence. This
is about a 60% reduction in measured median edit time for this workload; it does
not represent every document, renderer cost, or another operating system.
