# Emulsion website

A responsive product website and on-page documentation for the Emulsion desktop
creative app, built with Vite, vanilla JavaScript, and CSS. Feature descriptions
and limits follow the [application README](../README.md).

## Development

Use Node.js 24 (also used by the container build):

```sh
cd site
npm ci
npm run dev
```

`npm run build` produces `dist/`; `npm run preview` previews that build.

## macOS downloads

The macOS button downloads the Apple silicon disk image from GitHub's permanent
latest-release asset URL:

`https://github.com/jeremielumandong/emulsion/releases/latest/download/Emulsion-macos-arm64.dmg`

This asset is available in v0.1.0. Future published releases must include the same
stable filename; GitHub redirects the link to the latest release automatically.
Users open the disk image and drag Emulsion to Applications. No local app build
is required.

Changes limited to `site/`, `docs/`, or the root `README.md`, `CONTRIBUTING.md`, and
`SECURITY.md` skip the Rust and platform checks in CI and build only the website
container. The **Website container** workflow can also be
run manually to publish a site image from `main` without building the app.
The macOS release workflow runs separately on manual dispatch. Updating the site
or publishing a new app release does not require rebuilding the other.

## Windows downloads

The Windows button links to GitHub's permanent latest-release asset URL:

`https://github.com/jeremielumandong/emulsion/releases/latest/download/Emulsion-windows-x64-setup.exe`

The Windows release workflow uploads this stable filename alongside the versioned
signed installer and checksums. Both platform workflows contribute to the same
draft release; publish it as latest only after both finish successfully. See
[release instructions](../docs/technical/releases.md).

Deploy this site update once. Later Windows releases require no website rebuild,
container restart, mounted version file, or R2 upload. GitHub redirects the link
to the asset on its latest published release. Drafts and prereleases do not update
the public download; the latest release must include the stable Windows asset.

## Content and images

`index.html` contains the Emulsion concept, five workspace showcases, capability
cards, six practical workflows, and the documentation guide. Keep it aligned with the application
README when features change. Verify shortcuts and menu paths against the UI source,
not only README wording; see [the content audit](CONTENT_AUDIT.md) for checked sources. The guide has dedicated Photo, Paint, Library, Design, and Diagram sections, followed by RAW development, recipes,
batch export, printing, history, file formats, optional AI setup, workspace controls, and links to troubleshooting and further guides.
`src/style.css` provides the responsive layout; `src/main.js` handles the workspace
tour, screenshot lightbox, illustrative colour previews, and installation dialog.

Existing imagery lives in `public/assets/`. The earlier Photo (`editor.png`) and Paint (`drawing.png`)
captures are preserved at 2530×1377. The Home capture (`home.png`) uses the supplied
September 30 dashboard screenshot, copied unchanged at 2530×1377. The current Photo showcase, tour, and retouching workflow use the
supplied September 30 `portrait.png`, copied unchanged as `photo-portrait.png`
at the same resolution. Its captions describe recipe previews and adjustment layers.
The Paint capture also illustrates the assistant workflow. The Library Develop
capture (`library-presets.png`) illustrates the RAW workflow. The older
`raw-development.png` capture is no longer used to describe the current workflow. Workflow screenshots have dedicated
`figure[data-media-slot]` elements in `index.html`:

| Slot | Current capture |
| --- | --- |
| `photo-editing` | Portrait recipe previews and Photo adjustment layers |
| `raw-development` | Library Develop, presets, tone controls, and Before / After |
| `assistant-editing` | An assistant request, Apply/Skip controls, and resulting layers |
| `printing` | Library Print dialog, paper preview, printer, and placement controls (`library-printing.png`) |

To update a workflow screenshot, replace its asset or update the image and
full-size link in its figure, retaining the caption and descriptive alt text. For example:

```html
<img src="/assets/library-presets.png"
     alt="Library Develop with presets, tone controls, and Before / After"
     width="2530" height="1377" loading="lazy">
```

Use real image dimensions and descriptive alt text. Existing workspace tour image
paths and descriptions are in `src/main.js`. The tour starts with Home, showing project actions and recent files, followed by
the five workspaces. Photo and Paint
use application captures; Library uses the supplied `library-panel.png` catalog capture.
Design and Diagram use the supplied `design-poster.png` and `diagram.png` captures. To add a
capture, set that view’s `image` and `alt` fields and update its caption; the tour
will automatically show the image and enable the full-size button. Update the initial screenshot and
lightbox markup in `index.html` too. The hero uses `public/assets/splash.png`, copied unchanged from the app’s
`assets/landing/splash.png`. The illustrative history preview continues to use
`public/assets/hero.jpg`. Google Fonts has system fallbacks.

## Workspace showcase slots

The page introduces Photo, Paint, Library, Design, and Diagram as distinct tools
working together in one application. Each has an anchored section with supplied application screenshots, descriptive
captions, and full-size links.

| Section / media slot | Current showcase |
| --- | --- |
| `showcase-photo` | Portrait editing, recipe previews, and adjustment layers (`photo-portrait.png`) |
| `showcase-paint` | Supplied Paint workspace screenshot (`painting.png`) |
| `showcase-library` | Library catalog, folders, metadata, and export (`library-panel.png`) |
| `library-presets` | Imported presets and Before / After in Develop (`library-presets.png`) |
| `library-masking` | Local masks and Before / After in Develop (`library-masking.png`) |
| `showcase-design` | Supplied Design workspace screenshot (`design-poster.png`) |
| `showcase-diagram` | Supplied Diagram workspace screenshot (`diagram.png`) |

Update the image and full-size link inside the corresponding
`figure[data-media-slot]`, or replace them with a video with controls and a poster.
Keep the section ID, figure caption, and accessible media description. Prefer
`loading="lazy"` for screenshots and `preload="none"` for videos; include captions
for narrated films. Existing screenshots in the workspace tour and workflow
sections remain available below the showcases.

The three Library captures supplied on September 30 are preserved unchanged at
2530×1377: `Library2.png` maps to `library-panel.png`, `Library1.png` to
`library-presets.png`, and `Library3.png` to `library-masking.png`. The Library
showcase uses all three, with full-size links and a responsive two-column detail
layout. The tour uses the catalog capture, and `#guide-library-presets` explains
importing a preset pack, applying a listed preset, and checking import details.

The Design showcase and tour use the supplied September 30 neon OMARCHY poster
capture (`screenshot-2026-09-30_13-07-34.png`), preserved unchanged as
`design-poster.png` at 2530×1377. Its captions describe the visible templates,
editable text, paths, and glow effects.

The supplied September 30 `printing.png` is preserved unchanged at 2530×1377 as
`library-printing.png`. The `#printing-workflow` section shows printing from Library
and documents; `#guide-print` covers entry points, placement, layouts, PDF output,
and the current platform validation status from the application printing guide.

## Workflow ownership and branding

The six workflows follow the current application README: Photo retouches and
composites in layers; Library owns RAW development, sidecars, and Develop history.
**Edit in Photo…** opens developed pixels in a new Photo document, which later
Library adjustments do not update. `#raw` now targets the Library workflow;
`#photo-workflow` targets layered Photo editing. Legacy linked-RAW projects are
explained separately in the detailed RAW guide.

The header, footer, PNG favicon, and touch icon use `assets/emulsion-icon.png`,
copied unchanged from the app's `assets/icons/emulsion.png`. The ICO fallback at
`/favicon.ico` is copied from `assets/icons/emulsion.ico`. Keep website branding
aligned with these application assets when the app icon changes.

## Container CI

The main [CI workflow](../.github/workflows/ci.yml) calls the reusable
[Website container](../.github/workflows/site.yml) workflow on every pull request
and push to `main`. It builds the site inside Docker with Node.js 24, `npm ci`,
and `npm run build`, then smoke-tests the running image. This job runs independently
of the Rust checks, including for site-only changes. The container workflow also
supports manual runs.

Pushes to `main` and manual main-branch runs publish to GitHub Packages (GHCR)
after the container checks pass; pull requests only build and test:

- `ghcr.io/jeremielumandong/emulsion-site:latest`
- `ghcr.io/jeremielumandong/emulsion-site:sha-<full-commit-sha>`

The image runs on `linux/amd64`, serves the built static site through unprivileged
NGINX on port `8080`, and exposes `/healthz`. The workflow summary includes the
exact pull/run commands for that build. Publishing uses `GITHUB_TOKEN` with
`packages: write`; no VM credentials or automatic deployment are configured.
Private packages require registry authentication on the VM, or package visibility
can be made public in GitHub. Forks publish under their own repository name.

Product source: https://github.com/jeremielumandong/emulsion
