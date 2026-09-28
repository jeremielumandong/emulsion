# Temporary stencils and picked connector attachments

## Stencil lifetime

Opening/importing a diagram no longer writes packs to the permanent library.
This applies to File Open, Import diagram and live/headless MCP imports.
`import_diagram.save_stencils` defaults to false; explicitly setting true,
`save_document_stencils`, Import stencils, and Keep as stencil pack still persist.

Page artwork is grouped in **Imported data · temporary**, with collapse,
Clear and Show page shapes controls. Clear affects only this session's toolbox
for that page. It leaves the document, undo history and saved packs untouched.
Pending preview jobs cannot repopulate a cleared section. Opening another project
uses its own temporary toolbox.

Saved packs are collapsed groups, with individual Remove and properties/folder
controls. Search includes stencil names and expands matching packs. **Clear
collected imports** removes library references tagged Imported shapes by previous
versions; source files and canvas objects remain. Older explicit saves used the
same tag, so this is a deliberate cleanup action, never an automatic migration.
New explicit saves use the distinct Saved shapes tag. MCP can remove individual
saved packs through `remove_creative_asset`.

## Connector placement

In Connect mode, picking an object records the exact relative point (clamped to
its bounds), instead of falling back to a side midpoint. This also applies to
reconnection and the destination of a port drag. Visible midpoint handles remain
shortcuts. Existing connectors keep their current ports until explicitly changed.
The picked position tracks movement and resizing. Connector-to-connector attachment
continues to use the nearest path position.

## Validation and delivery

22 diagram UI/project tests, five diagram project MCP tests and the persistent
stencil deduplication/round-trip test passed. Regressions exercise custom
attachment movement/resize/undo and clearing/restoring temporary stencils
without modifying the document. The explicit pack export/install/place/undo UI
test also passed. Release build, AppImage packaging and version smoke check passed.

Installed `/home/arkane/Applications/Emulsion.AppImage`; restart to use the update.
The previous AppImage was backed up as `/home/arkane/Applications/Emulsion.AppImage.bak-stencil-attachment-20260928T164701858952Z`.
Delivery checksum and source snapshots are in `target/stencil-attachment-review/`.
