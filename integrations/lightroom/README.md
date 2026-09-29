# Emulsion handoff for Lightroom Classic

This companion runs in Adobe Lightroom Classic's Lua SDK host. Emulsion itself
does not execute Adobe `.lrplugin` files or distribute Adobe/VSCO profiles.

1. In Lightroom's Plug-in Manager, add the `Emulsion.lrplugin` directory.
2. Select photos and choose Library → Plug-in Extras → Export selected photos to Emulsion.
3. Choose a parent folder. A new handoff folder receives copies of originals,
   Develop settings, ratings/flags, collection names and 16-bit sRGB TIFF renders.
4. In Emulsion Library, choose Import Lightroom catalog and select
   `handoff.emulr.json`, or use MCP `library_catalog` with action `import_lightroom`.

Keep the handoff folder: imported originals stay there. Emulsion creates its own
sidecars and preserves existing edits. TIFFs appear in “Lightroom rendered
references”. They retain Lightroom's rendered appearance, including any installed
and licensed VSCO preset/profile used in Lightroom. RAW controls are translated
where supported; the importer reports adjustments it cannot reproduce. TIFFs
cannot recover the original sensor data or become editable Adobe Develop history.

A failed/canceled export leaves an incomplete folder without a finished manifest;
originals in the Lightroom catalog are unchanged. A new export creates a new folder.
No Adobe or VSCO assets are included. This is not a VSCO account integration.

Validation: Lua syntax and a mocked SDK contract test are available locally:
`lua integrations/lightroom/test_contract.lua`. Running the plugin inside a real
Lightroom Classic installation remains required before treating Adobe-host
compatibility as verified. Direct `.lrcat` migration separately supports recognized
readable history records; the companion exports current settings only.
