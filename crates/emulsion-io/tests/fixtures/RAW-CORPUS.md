The camera corpus is opt-in and is not bundled with the repository. The adjacent
`raw-corpus.json` records three samples from https://raw.pixls.us/ whose catalog
records specify CC0, with exact download URLs and SHA-256 hashes. Catalog checked
2026-09-22. The downloads total 46,968,853 bytes. Camera sample metadata describes
these files only and does not imply support for every mode of the same camera.

Download the files into `target/raw-corpus`, retaining the manifest filenames,
then run from the repository root in PowerShell:

```powershell
$env:EMULSION_RAW_CORPUS = (Resolve-Path target/raw-corpus).Path
cargo test -p emulsion-io --test raw_corpus -- --ignored --nocapture
```

The test verifies source hashes before decoding, exercises Bayer NEF, X-Trans
RAF, and CRAW CR3, develops exposure/white-balance/highlight edits, saves and
reopens the recipe, exports at full resolution, compares exported pixels with
development pixels, and verifies original hashes afterward. Temporary project
and export artifacts are removed after each successful case; failed cases keep
them beside the samples for diagnosis. The test reports elapsed time but does
not impose hardware-dependent timing thresholds. This is a regression smoke
test, not a calibrated color or image-quality reference. Other vendors and
compression modes remain unverified until additional licensed fixtures are run.

Verified on Windows with rawler 0.8.0 on 2026-09-22:

| Sample | Developed size | Open/edit/save/reopen/export | Elapsed full roundtrip |
| --- | --- | --- | --- |
| Nikon D50, 12-bit lossy NEF, Bayer | 3008 × 2000 | Passed; pixel-identical PNG and unchanged original | 3.43 s |
| Fujifilm X-Pro1, 12-bit uncompressed RAF, X-Trans | 4896 × 3264 | Passed; pixel-identical PNG and unchanged original | 13.86 s |
| Canon EOS M50, CRAW CR3, Bayer | 6000 × 4000 | Passed; pixel-identical PNG and unchanged original | 13.80 s |

These timings include hash checks and disk I/O and are observations, not performance
guarantees. The runtime metadata exposes the sensor and bit depth after decode.
Compression details that rawler does not expose remain `unknown` (RAF/CR3) or
explicitly unverified (Nikon compressed submode); the sample manifest records the
catalog's more specific descriptions separately.
