# Portable Design templates and Diagram stencils

## Author, export, install

1. Create a **Design** project for a design template, or a **Diagram** project for
   a stencil pack. Keep text and shapes editable. A stencil pack uses one page per
   reusable item; name each page for its library button.
2. In Design's **Templates** drawer or the Diagram drawer, choose **Export Design
   template…** or **Export stencil pack…**. Supply a name and optional author,
   license, and tags. Stencil export excludes root page-background fills.
3. Share the `.emutemplate` or `.emustencil` file. Use **Install template / stencil
   file…** to add it to the local library. Opening the package through Open also
   installs it and opens an editable copy. Originals are never edited on import.
4. A Design template opens a page-by-page preview. Choose **Add as new page**,
   **Replace current page**, or **Add all pages** for a multi-page template.
   Each action is undoable; Cancel leaves the project unchanged. Each named
   stencil button places editable objects and connections as one undoable action.
   Search matches pack names, page names, and tags. Existing library properties
   allow metadata changes and removal of the library entry.

Installed packs work offline. Removal removes the library entry; it does not
modify the shared original file or artwork already placed in a project.
Export strips local version history, but includes current page artwork and data.
Review that content before sharing. Fonts are referenced by family name, so install
matching fonts on the recipient machine for identical typography.

## GitHub sharing

Unzip an exported package into a public GitHub repository. The selected directory
must contain `emulsion-template.json` and its referenced native project:

```text
emulsion-template.json
project.emu
preview.png
```

In either drawer, choose **Install from GitHub URL…** and paste:

- `https://github.com/owner/repository` for the default branch and root directory.
- `https://github.com/owner/repository/tree/main/packs/flowchart` for a directory.
- `https://github.com/owner/repository/tree/<commit-sha>/packs/flowchart` for a pinned version.

Branch names containing `/` are ambiguous in a web URL; use a commit SHA instead.
Private repositories, authenticated downloads, release links, and automatic updates
are not supported by this installer. An exported ZIP stored as a single file in a
repository must first be downloaded and installed locally. Installation downloads
repository data over HTTPS, validates the package, and copies it into the local
library; it never executes repository scripts. No Git installation is needed.

## Version 1 file specification

Both extensions are ordinary ZIP archives with the same manifest. The extension
is a convenience; the manifest's `kind` and native project kind must agree.
This is Emulsion's openly documented package format, not a claim of compatibility
with Visio stencil packages or another vendor's template format.

```json
{
  "format_version": 1,
  "kind": "stencil",
  "name": "Example workflow shapes",
  "author": "Example author",
  "license": "CC0-1.0",
  "description": "Editable workflow examples",
  "tags": ["workflow", "general"],
  "project": "project.emu",
  "preview": "preview.png"
}
```

`kind` is `design` or `stencil`. `project` is a relative path to a complete native
`.emu` project. Its pages contain editable objects, embedded image pixels, diagram
metadata, and Design metadata. `preview` is optional and points to an image.
Author, license, description, and tags default to empty. Export includes a PNG
preview of the first page. It resets history to the current artwork on every page.

Manifest names are 1–200 characters; tags are at most 50 strings of 1–200
characters. No absolute paths, parent traversal, duplicate ZIP entries, or future
manifest versions are accepted. Limits: 256 MiB package/download and total decoded
outer entries, 10,000 entries, 64 KiB manifest, 8 MiB preview, and the native
project's own nested archive/page limits. Invalid packages never enter the catalog.
The library identifies installed versions by a SHA-256 digest of project bytes
and manifest. Reinstalling the same version updates the same entry; a changed
version can coexist with it.

## Diagram exchange

Use **Import diagram file…** in Diagram, or Open for recognized extensions:

| Input | Behavior |
| --- | --- |
| `.drawio`, draw.io XML through Import | Editable pages, shapes, connectors; editable `.drawio` export available |
| `.vsdx`, `.vsdm`, `.vstx` | Visio OPC/XML drawing pages; macros are not run |
| `.vssx` | Visio masters become editable pages, which can be exported as an Emulsion stencil pack |
| `.vdx`, `.vsx` | Visio XML drawings or stencil masters |
| `.lucid` | Lucid Standard Import v1 ZIP containing `document.json` and optional embedded images |
| `.lucidjson`, Lucid Standard Import JSON through Import | Same document schema without embedded image files |

For a Lucidchart document, export **VSDX or VDX** from Lucidchart, then import that
file. Lucid Standard Import packages are a separate documented exchange format;
this reader does not support undocumented cloud backups or infrastructure JSON.
Legacy binary `.vsd` requires conversion to `.vsdx` or `.vdx` first.

Import keeps source files unchanged and creates an editable local project. It
reports conversions such as unsupported custom shapes, rich label formatting,
complex Visio geometry, rotated-label alignment, foreign/OLE objects, specialized
arrowheads, and external images. Review those notes before relying on visual
fidelity. These formats are not yet lossless substitutes for their vendor editors.
Lucid data-backed shapes and unsupported endpoint constraints reject the import
with an explanation rather than silently disappearing.

Specifications: [Microsoft Visio format reference](https://learn.microsoft.com/en-us/office/client-developer/visio/visio-file-format-reference),
[Lucid Standard Import](https://lucid.readme.io/docs/overview-si),
[Lucid document export](https://help.lucid.co/hc/en-us/articles/16324571257492-Export-or-print-a-Lucid-document).
