# Emulsion website

A responsive product website and on-page documentation for the Emulsion desktop
image editor, built with Vite, vanilla JavaScript, and CSS. Feature descriptions
and limits follow the [application README](../README.md).

## Development

Use Node.js 24 (also used by the container build):

```sh
cd site
npm ci
npm run dev
```

`npm run build` produces `dist/`; `npm run preview` previews that build.

## Content and images

`index.html` contains the marketing copy, capability cards, three detailed
workflows, and the documentation guide. Keep it aligned with the application
README when features change. Verify shortcuts and menu paths against the UI source,
not only README wording; see [the content audit](CONTENT_AUDIT.md) for checked sources. The guide covers editing, RAW development, recipes,
batch export, history, file formats, optional AI setup, workspace controls, and links to troubleshooting and further guides.
`src/style.css` provides the responsive layout; `src/main.js` handles the workspace
tour, screenshot lightbox, illustrative colour previews, and installation dialog.

Existing imagery lives in `public/assets/`. The current Photo (`editor.png`), Draw (`drawing.png`), and Home (`home.png`)
screenshots come from the supplied September 24 captures, preserved at 2530×1377.
The Draw capture also illustrates the assistant workflow. The supplied RAW
(`raw-development.png`) and batch (`batch-export.png`) captures fill the remaining
workflow sections at the same native resolution. Workflow screenshots have dedicated
`figure[data-media-slot]` elements in `index.html`:

| Slot | Current capture |
| --- | --- |
| `raw-development` | RAW Properties with histogram, exposure, white balance, and tone controls |
| `recipes-batch` | Recipe library and batch export settings |
| `assistant-editing` | An assistant request, Apply/Skip controls, and resulting layers |

To update a workflow screenshot, replace its asset or update the image and
full-size link in its figure, retaining the caption and descriptive alt text. For example:

```html
<img src="/assets/raw-development.png"
     alt="RAW histogram and exposure controls beside a portrait"
     width="2530" height="1377" loading="lazy">
```

Use real image dimensions and descriptive alt text. Existing workspace tour image
paths and descriptions are in `src/main.js`; update the initial screenshot and
lightbox markup in `index.html` too. The hero and illustrative history preview
currently share `public/assets/hero.jpg`. Google Fonts has system fallbacks.

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
