# Emulsion handoff plug-in for the photo catalog app

This companion runs in the host catalog application's Lua SDK. Emulsion itself
does not execute `.lrplugin` files or distribute proprietary vendor profiles.

1. In the host application's Plug-in Manager, add the `Emulsion.lrplugin` directory.
2. Select photos and choose Library → Plug-in Extras → Export selected photos to Emulsion.
3. Choose a parent folder. A new handoff folder receives copies of originals,
   Develop settings, ratings/flags, collection names and 16-bit sRGB TIFF renders.
4. In Emulsion Library, choose the photo catalog (`.lrcat`) import and select
   `handoff.emulr.json`, or use MCP `library_catalog` with action `import_lightroom`.

Keep the handoff folder: imported originals stay there. Emulsion creates its own
sidecars and preserves existing edits. TIFFs appear in a “rendered references”
collection. They retain the host application's rendered appearance, including any
installed and licensed third-party preset/profile. RAW controls are translated
where supported; the importer reports adjustments it cannot reproduce. TIFFs
cannot recover the original sensor data or become editable Develop history.

A failed/canceled export leaves an incomplete folder without a finished manifest;
originals in the host catalog are unchanged. A new export creates a new folder.
No proprietary vendor assets are included. This is not an account integration
with any preset vendor.

Validation: Lua syntax and a mocked SDK contract test are available locally:
`lua integrations/lightroom/test_contract.lua`. Running the plugin inside a real
installation of the host catalog application remains required before treating
host compatibility as verified. Direct `.lrcat` migration separately supports
recognized readable history records; the companion exports current settings only.
