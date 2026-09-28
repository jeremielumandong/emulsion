# Library follow-up implementation

Requested after the Library/Develop audit on 2026-09-28. Items require working UI,
shared rendering/persistence and automation where applicable, plus validation.
Partial implementations must remain listed here until their acceptance checks pass.

- [x] Visible 90-degree rotation controls; rotation, undo and persistence tests pass.
- [x] CMYK soft proofing with bidirectional printer-profile validation and alpha preservation.
- [ ] Native-resolution region processing and interruptible demosaicing.
- [x] Multi-guide perspective solver and guide workflow: solver and automation implemented;
  numerical and desktop save/undo tests pass. Guide overlays are
  session-only; the solved geometry persists.
- [x] Content-aware healing with portable nondestructive spot/stroke settings. The
  deterministic texture-fill engine has bounded search regions; preview and export
  can synthesize different texture at different resolutions.
- [ ] Sensor-channel highlight reconstruction with versioned rendering.
- [ ] Optional AI sensor denoise with an actual sensor-domain model.
- [ ] As-shot dual-illuminant profile interpolation from camera calibration.
- [ ] Wide-gamut RGB input and Photo working/output pipeline.
- [ ] Panorama alignment, projection, blending, export and catalog integration.
- [ ] Depth-aware blur with editable focus and portable depth assets.
- [ ] HDR rotation/perspective alignment, improved deghosting and measured larger-input support.
- [ ] Broader camera/profile/illuminant corpus and photographic quality measurements.

Existing RGB AI restoration is not sensor denoise. Existing translation-only HDR
merge is not panorama stitching. Synthetic tests alone do not establish photographic
quality or cross-camera equivalence.

Validation so far: two rotation UI tests, one guided-perspective desktop save/undo test,
two synthetic guide-solver tests, five
local-edit renderer tests, RGB viewing regression and the opt-in CMYK test using
`/usr/share/ghostscript/iccprofiles/default_cmyk.icc` passed. The guided-perspective
MCP request validation test, final workspace/all-target Clippy with warnings denied,
and Rust formatting checks passed. No installation or broad-camera quality
certification has been performed.
