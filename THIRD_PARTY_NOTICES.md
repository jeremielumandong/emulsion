# Third-party notices

Emulsion's original code is licensed under MIT. The third-party code it uses
retains its own licenses and ownership; the root MIT license does not relicense
the vendored GPUI code. Changes to this GPUI fork remain under its upstream
Apache-2.0 terms unless a file specifies another applicable license.

The GPUI family is included under `vendor/gpui/`. Its exact package versions,
release checksums, upstream repositories, and declared licenses are recorded in
[`UPSTREAM.json`](vendor/gpui/UPSTREAM.json). Most of these packages are
Apache-2.0, with copyright notices identifying Zed Industries or Longbridge.
`gpui-pre-reqwest` retains its MIT OR Apache-2.0 license choice.

Additional notices apply to Lucide and Feather icons (ISC and MIT),
Microsoft Terminal-derived rendering code (MIT), and Arboard-derived clipboard
code (Apache-2.0 OR MIT). Their license texts and original source attributions
are retained; see [`vendor/gpui/LICENSING.md`](vendor/gpui/LICENSING.md) for exact
locations and the provenance of supplemental license files.

This notice inventories the vendored GPUI family. Other dependencies retain
their own licenses; this is not an exhaustive inventory of the entire Cargo
dependency graph. Release packages must also retain notices required by their
other bundled dependencies and assets.
