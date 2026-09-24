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
README when features change. The guide covers editing, RAW development, recipes,
batch export, history, file formats, optional AI setup, and workspace controls.
`src/style.css` provides the responsive layout; `src/main.js` handles the workspace
tour, screenshot lightbox, illustrative colour previews, and installation dialog.

Existing imagery lives in `public/assets/`. New workflow screenshots have dedicated
`figure[data-media-slot]` elements in `index.html`:

| Slot | Suggested capture |
| --- | --- |
| `raw-development` | RAW Properties with controls and the before/after divider |
| `recipes-batch` | Recipe library and batch export settings |
| `assistant-editing` | An assistant request, Apply/Skip controls, and resulting layers |

Replace the corresponding `.media-placeholder` div with an image, retaining the
figure and updating its caption. For example:

```html
<img src="/assets/raw-development.webp"
     alt="RAW exposure controls beside a photo with a before/after divider"
     width="1600" height="1200" loading="lazy">
```

Use real image dimensions and descriptive alt text. Existing workspace tour image
paths and descriptions are in `src/main.js`; update the initial screenshot and
lightbox markup in `index.html` too. The hero and illustrative history preview
currently share `public/assets/hero.jpg`. Google Fonts has system fallbacks.

## Container CI

[Website container](../.github/workflows/site.yml) builds and smoke-tests the
Docker image for website pull requests, pushes to `main`, and manual workflow
runs. Main-branch runs publish to GHCR after those checks pass:

- `ghcr.io/jeremielumandong/emulsion-site:latest`
- `ghcr.io/jeremielumandong/emulsion-site:sha-<full-commit-sha>`

The image runs on `linux/amd64`, serves the built static site through unprivileged
NGINX on port `8080`, and exposes `/healthz`. The workflow summary includes the
exact pull/run commands for that build. Publishing uses `GITHUB_TOKEN` with
`packages: write`; no VM credentials or automatic deployment are configured.
Private packages require registry authentication on the VM, or package visibility
can be made public in GitHub. Forks publish under their own repository name.

Product source: https://github.com/jeremielumandong/emulsion
