# File actions by workspace

The File menu follows the active document tab and workspace. Open (Ctrl+O)
opens a separate document; the Import submenu adds content or resources through
the existing workspace importers.

| Workspace | Open | Import |
| --- | --- | --- |
| Photo | Images, including RAW and layered image formats | Images as layers; color lookup tables |
| Paint | Artwork, including OpenRaster, PSD, XCF and images | Images as layers; brushes |
| Design | Emulsion designs, PowerPoint presentations, Lottie animations | Images/SVG; video/audio; local templates |
| Diagram | Emulsion diagrams, Visio, draw.io, Lucid exports | Pages from Visio, draw.io or Lucid into the current diagram; stencil libraries |
| Library | Photo folder | Develop preset packs; a separate action opens images in Photo |
| Home | Any supported document | Workspace-specific actions appear after opening a document |

New uses the active workspace's document type. Existing Save, Save As, Print and
Export actions remain available in the editor. Templates, brushes, stencils and
presets use their existing installation flows; they do not create document tabs.

An image opened through Photo or Paint keeps that workspace, captured when the
picker opens. Native Design and Diagram projects retain their stored type. Opens
from Home or recent files retain the existing classification behavior.

Diagram imports append editable pages and use project Undo. Import fidelity and
warnings come from the existing format readers. Lucid imports use exported files,
not a live Lucid account connection. File picker titles describe the relevant
formats; the current platform picker API does not provide extension filters.

Regression coverage: `crates/emulsion-ui/src/file_menu_tests.rs` exercises native
menu routing, contextual image opening, diagram page import and Undo, and Library
folder/preset pickers. Existing shared export and workspace navigation tests cover
the retained File actions.
