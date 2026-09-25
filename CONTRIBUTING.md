# Contributing to Emulsion

Contributions through pull requests are welcome. For a substantial feature or
architecture change, open an issue first to discuss the intended behavior and
scope. For a bug, include your operating system, Emulsion version or commit,
reproduction steps, and expected and actual results. Attach logs captured as
described in [Logs](docs/files-and-environment.md#logs). Remove credentials and
private images or paths from logs and screenshots.

Report suspected vulnerabilities privately using the [security policy](SECURITY.md).

## Development

Fork the repository, create a branch from `main`, and keep each pull request
focused on one change. See the [README build instructions](README.md#build) for
platform dependencies. Rust is selected by `rust-toolchain.toml`.

From the repository root, run:

```sh
cargo run --locked -p emulsion-app
```

For website changes, see the [website README](site/README.md):

```sh
cd site
npm ci
npm run dev
npm run build
```

## Validation

For Rust changes, run the checks used by [CI](.github/workflows/ci.yml):

```sh
python3 scripts/check-gpui-vendor.py
python3 scripts/test-license-staging.py
bash scripts/test-renderer-policy.sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked -- --test-threads=1
```

The Lavapipe compute checks and the renderer smoke run that follow in CI are
described in [rendering support](docs/rendering.md) and
[GPU image processing](docs/gpu-rendering.md); on macOS and Windows CI runs
`cargo check --workspace --all-targets --locked`.

UI fixtures modify process environment, so run tests serially as shown. Add or
update regression coverage when changing behavior. See [tool testing](docs/tool-testing.md)
and [rendering support](docs/rendering.md) for headless tests and checks requiring
a graphics environment. CI also validates vendored licenses and software GPU
rendering on Linux, and compilation on macOS and Windows.

For documentation changes, check links and commands. For website changes, run
`npm run build` in `site/` and check the affected pages in a browser. Describe any
checks you could not run and why in the pull request.

CI skips Rust compilation and tests when changes are limited to `site/`, the
root README, contributing or security guides, and Markdown or images in `docs/`.
The website check still runs. Code, dependencies, packaging, licenses, workflow
changes, and unknown paths retain the full Rust checks. Mixed changes run the
full checks whenever any changed file requires them.

## Pull requests

- Explain the problem and resulting behavior, and link related issues.
- Include test results and screenshots for visible changes.
- Keep credentials, private files, generated build output, and unrelated changes
  out of the diff.
- Preserve license notices. When changing vendored code, follow the
  [GPUI maintenance notes](vendor/gpui/README.md) and update provenance as required.
- Explain dependency, workflow, or permission changes so reviewers can assess
  their effect.

Maintainers review contributions before merging. Outside-contributor CI may wait
for a maintainer to review and approve the workflow run. Keep discussion respectful
and constructive, and respond to review feedback on the same pull request.

Emulsion's original code uses the [MIT License](LICENSE); contributions to that
code are made under the same license. Only submit code and assets you have the
right to contribute. Vendored code retains its own licenses, listed in the
[third-party notices](THIRD_PARTY_NOTICES.md).
