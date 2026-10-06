# Blend If renderer follow-up: evidence, not a legacy behavior change

Read-only inspection on 2026-10-05 found material differences between the
existing Emulsion renderer and Patchy's independently Photoshop-calibrated
behavior. These findings belong to the planned versioned Photoshop profile.
The fixture integration does not alter legacy rendering or claim pixel parity.

## Inclusive-byte split ramps

[Patchy's calibration record](https://github.com/SethRobinson/Patchy/blob/20f95a201c395213ce3e212f13d912e418d1cba6/docs/layer-effects-render.md#per-effect-calibration-notes)
describes a split black interval with endpoint bytes `a < b` as retaining
`(v - a + 1) / (b - a + 1)` for byte values `a <= v < b`. It reaches full
coverage at `b`; joined handles produce a hard cutoff. The white side has the
corresponding inclusive transition. Source SHA-256:
`bd6a730819faf9ebc3cb1942cba5819b33965555b32396def554ad34556d00fc`.

[Independent endpoint tests](https://github.com/SethRobinson/Patchy/blob/20f95a201c395213ce3e212f13d912e418d1cba6/tests/core/compositor_blend_if_tests.cpp#L235-L272)
pin black `10..13` at values 10, 11, 12, 13 to coverage
`1/4, 1/2, 3/4, 1`, and a white `20..23` transition to its mirrored values.
Test-source SHA-256:
`33bd67af7edbe1fef39b4a847509da2e9e07b3300ca62aa0b601e75ab9b2a28e`.

Emulsion's current `BlendRange::coverage` in
`crates/emulsion-raster/src/composite.rs` instead uses a continuous normalized
linear ramp, mathematically `(v - a) / (b - a)` before clamping. At the same
black byte endpoints it yields `0, 1/3, 2/3, 1` apart from floating-point
roundoff. This is a concrete formula difference, not an interchange index bug.
The potential per-pixel effect can be large for narrow split intervals.

## Other differences worth separately validating

The same calibration record uses rounded integer Gray weights
`299R + 590G + 111B` divided by 1000, and preserves transparent underlying
coverage with `(1 - destination_alpha) + destination_alpha * underlying_gate`.
Emulsion's inspected legacy code instead computes Gray with
`0.299R + 0.587G + 0.114B`, and directly multiplies the source and underlying
gate values. These are source-level differences; do not label their full
rendering consequences as measured Emulsion-vs-Photoshop test results.

The upstream original fixture contains simultaneous Gray/R/G/B gates, groups,
and an adjustment. Upstream compares its own full renderer with a Photoshop BMP
at tolerance two bytes per channel, not exact equality. The fixture is therefore
not an isolated pixel proof for every individual formula above.

## Bounded next validation

Use independent Photoshop-authored single-channel source/backdrop fixtures
covering narrow split endpoints, hard joined endpoints, transparent/fractional
backdrops, and asymmetric RGB Gray controls. Preserve original bytes and saved
reference pixels. Run the new versioned profile on the independent raw inputs,
without consuming the saved composite as renderer input. Keep legacy documents
on their existing semantics. Until these checks exist, describe the new tests
as independent metadata coverage and retain an explicit appearance fallback
for unrepresentable combinations; do not claim broad Blend If renderer parity.
