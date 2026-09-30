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
cards, five practical workflows, and the documentation guide. Keep it aligned with the application
README when features change. Verify shortcuts and menu paths against the UI source,
not only README wording; see [the content audit](CONTENT_AUDIT.md) for checked sources. The guide has dedicated Photo, Paint, Library, Design, and Diagram sections, followed by RAW development, recipes,
batch export, history, file formats, optional AI setup, workspace controls, and links to troubleshooting and further guides.
`src/style.css` provides the responsive layout; `src/main.js` handles the workspace
tour, screenshot lightbox, illustrative colour previews, and installation dialog.

Existing imagery lives in `public/assets/`. The supplied Photo (`editor.png`), Paint (`drawing.png`), and Home (`home.png`)
screenshots come from the supplied September 24 captures, preserved at 2530×1377.
The Paint capture also illustrates the assistant workflow. The supplied RAW capture (`raw-development.png`) illustrates the RAW workflow.
The Library catalog capture (`library-panel.png`) also illustrates the batch-export
workflow at the same native resolution. Workflow screenshots have dedicated
`figure[data-media-slot]` elements in `index.html`:

| Slot | Current capture |
| --- | --- |
| `raw-development` | RAW Properties with histogram, exposure, white balance, and tone controls |
| `recipes-batch` | Library grid, recipe selector, and batch export settings |
| `assistant-editing` | An assistant request, Apply/Skip controls, and resulting layers |

To update a workflow screenshot, replace its asset or update the image and
full-size link in its figure, retaining the caption and descriptive alt text. For example:

```html
<img src="/assets/raw-development.png"
     alt="RAW histogram and exposure controls beside a portrait"
     width="2530" height="1377" loading="lazy">
```

Use real image dimensions and descriptive alt text. Existing workspace tour image
paths and descriptions are in `src/main.js`. The tour has five tabs: Photo and Paint
use application captures; Library uses the supplied `library-panel.png` catalog capture.
Design and Diagram use the supplied `design.png` and `diagram.png` captures. To add a
capture, set that view’s `image` and `alt` fields and update its caption; the tour
will automatically show the image and enable the full-size button. Update the initial screenshot and
lightbox markup in `index.html` too. The hero uses `public/assets/splash.png`, copied unchanged from the app’s
`assets/landing/splash.png`. The illustrative history preview continues to use
`public/assets/hero.jpg`. Google Fonts has system fallbacks.

## Workspace showcase slots

The page introduces Photo, Paint, Library, Design, and Diagram as distinct tools
working together in one application. Each has an anchored section. Paint, Library, Design, and Diagram use supplied application screenshots;
Photo has a labeled showcase-film placeholder. The placeholders advertise forthcoming media, not
forthcoming app functionality; they are not playback controls or fake screenshots.

| Section / media slot | Planned showcase |
| --- | --- |
| `showcase-photo` | Retouching and layered composition |
| `showcase-paint` | Supplied Paint workspace screenshot (`painting.png`) |
| `showcase-library` | Library catalog, folders, metadata, and export (`library-panel.png`) |
| `library-presets` | Imported presets and Before / After in Develop (`library-presets.png`) |
| `library-masking` | Local masks and Before / After in Develop (`library-masking.png`) |
| `showcase-design` | Supplied Design workspace screenshot (`design.png`) |
| `showcase-diagram` | Supplied Diagram workspace screenshot (`diagram.png`) |

Replace the `.showcase-placeholder` inside the corresponding
`figure[data-media-slot]` with a real image or a video with controls and a poster.
Keep the section ID, figure caption, and accessible media description. Prefer
`loading="lazy"` for screenshots and `preload="none"` for videos; include captions
for narrated films. Existing screenshots in the workspace tour and workflow
sections remain available below these placeholders.

The three Library captures supplied on September 30 are preserved unchanged at
2530×1377: `Library2.png` maps to `library-panel.png`, `Library1.png` to
`library-presets.png`, and `Library3.png` to `library-masking.png`. The Library
showcase uses all three, with full-size links and a responsive two-column detail
layout. The tour uses the catalog capture, and `#guide-library-presets` explains
importing a preset pack, applying a listed preset, and checking import details.

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
