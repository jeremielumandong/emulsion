# Checked projective geometry foundation

`emulsion_raster::projective` is a checked geometry foundation. The separate
[retained-pixel renderer](projective-pixels.md) uses its restricted differential
support. It does not change Placement, Smart Objects, persistence or the Distort
UI. Existing affine arithmetic and the existing `warp` implementation remain
unchanged.

## Contract

- `Projective2` acts on column vectors with a row-major 3 by 3 matrix:
  `(u, v, w) = H * (x, y, 1)`, with Cartesian result `(u/w, v/w)`.
  `a.compose(b)` means `a * b`: apply `b` first.
- Construction divides by the signed, first largest-magnitude coefficient.
  That coefficient becomes +1; zeros have positive sign. The bottom-right
  coefficient may be zero. Equality compares these canonical floating-point
  coefficients exactly, not approximate geometry. Arbitrary floating-point
  rescaling can introduce rounding, so callers must not use equality as a
  geometric tolerance test. Identity composition preserves exact coefficients.
- A valid map has finite, representable coefficients and a checked finite
  projective inverse. Row and column equilibration precedes inversion. Pivot
  and two-sided residual checks apply in that balanced basis, not to a raw
  determinant or translation-sensitive matrix norm.
- Point evaluation can fail even for a valid map. Rectangle validation is a
  separate operation: corners must have consistently signed denominators,
  bounded cancellation and a minimum/maximum denominator ratio of at least
  `1e-12`. Since the denominator is linear, this excludes a horizon throughout
  the rectangle. A map with a horizon elsewhere remains a valid map.
- Point results and projected rectangle corners have a checked, propagated
  Cartesian rounding uncertainty of at most
  `1e-9 + 64 * f64::EPSILON * abs(coordinate)` per coordinate. A stable
  denominator alone is insufficient: numerator cancellation can instead
  produce `PrecisionLoss`. This budget concerns evaluation of the stored
  coefficients, not error already introduced by coefficient canonicalization,
  conversion or composition. It is an absolute/relative numerical policy, not
  a guarantee of subpixel precision at arbitrarily large world coordinates.
- Mapped rectangle bounds enclose the corner values with floating-point
  rounding allowances. With no horizon, each linear-fractional coordinate has
  its extrema at corners. Bounds may conservatively include extra pixels.
  Identity mapping preserves exact input bounds.
- Integer bounds require finite representable endpoints and an `i32` width
  and height, including representable right/bottom endpoints. No allocation is
  performed. Callers must separately enforce source/canvas size limits
  (currently 30,000 pixels per side and 400 MP), intersection with output tiles,
  and temporary-memory/work budgets. Integer representability is not permission
  to allocate the full bounding box.
- Rectangle-to-quad construction uses ordered TL, TR, BR, BL source corners
  and corresponding destination vertices. Either strictly convex winding is
  allowed; crossed, concave, repeated and collinear vertices are rejected.
  Independent axis normalization preserves ordinary anisotropic scales. Both
  forward and inverse corner reprojection are checked against the destination
  and source extents. A normalized reprojection tolerance that would exceed
  `1e-4` due to coordinate precision is rejected, not silently relaxed.
- Affine conversion is explicit and checked. It does not change or replace
  any legacy affine execution path. Extracting an affine requires exactly zero
  projective terms; no near-affine approximation is performed. Canonicalization
  and an affine round-trip may round coefficients. Retain the original affine
  value when exact legacy arithmetic is required.

## Conservative numerical limits

The implementation uses `f64`, not exact predicates or arbitrary precision.
Nonzero coefficients that become subnormal during canonicalization, and
nonzero products that underflow during matrix multiplication, are rejected.
Balanced pivots at or below `64 * f64::EPSILON` and inverse residuals above
`1e-10` are rejected. These checks deliberately exclude some mathematically
invertible maps that cannot be supported reliably by this representation.

Point and rectangle calculations use a common power-of-two homogeneous input
scale to avoid large-coordinate overflow without introducing normalization
roundoff. Scaling must reverse exactly; subnormal information loss is refused.
Compensated products (FMA) and sums retain cancelled low-order terms. Their
rounding bounds include correction accumulation and absolute subnormal errors;
quotient intervals are then checked against the Cartesian accuracy budget.
This is deliberately conservative: some exactly cancelling configurations can
be refused if amplified correction uncertainty exceeds that budget.

Denominator cancellation is independently checked against the sum of absolute
terms, with a `128 * f64::EPSILON` margin. Output overflow, unrepresentable
outward bounds and unreliable coordinate precision are explicit errors. Very
large offsets can therefore be valid maps while particular tiny domains, quad
construction, or integer bounds are unsupported. No global determinant cutoff
rejects translations or tiny/anisotropic scales merely because of their units.

## Integration gate and acceptance

The foundation remains unserialized. Future integration must separately review
authoritative placement, mask/source domain composition, sampling/Jacobian mip
policy, allocation budgets, v16 native/history encoding, lifecycle migrations,
atomic refusal and UI targeting before enabling native Smart Distort.

Focused tests live in `crates/emulsion-raster/src/projective_tests.rs`. They
cover affine/reflected and perspective maps, inverses/composition, homogeneous
rescaling, zero bottom-right coefficients, translation/scale extremes,
horizons, invalid quads, conservative bounds, dense interior samples and
independent reference derivatives. Run them with:

```sh
cargo test --locked -p emulsion-raster projective -- --test-threads=1
```

Then run the repository checks in `CONTRIBUTING.md`, including formatting,
workspace Clippy/tests and relevant downstream compilation. Creating these
tests is not evidence that they have run.
