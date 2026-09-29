# Design variables

Open **Styles → Design variables** to create a named color or number. Select native
objects, choose **Bind…**, and select a compatible property. Editing the variable
updates its linked objects in one Undo step. Colors accept `#RRGGBB` or
`#RRGGBBAA`; numbers must be finite and fit the bound property's range.

Supported properties are fill, stroke, text color, opacity, font size, stroke
width, frame gap and frame padding. Text remains native editable text, including
its character runs. Bound properties follow the variable; unlink a property to
edit it independently. Rename preserves bindings. Remove or unlink retains the
last resolved appearance. Locked consumers reject an update atomically.

Variables belong to a page. Native saves, recovery, duplication and clipboard
preserve definitions and bindings. Pasting a different value under an existing
name creates a suffixed name and preserves both appearances. Component imports
carry the variables their artwork uses. Project library identities support explicit cross-page sharing; remote variable
libraries are not implemented.

MCP exposes `list_design_variables`, `set_design_variable`,
`rename_design_variable`, `remove_design_variable` and `bind_design_variable`.
Use a typed value such as `{"type":"color","value":[80,110,235,255]}` or
`{"type":"number","value":24}`. Binding accepts a property and object IDs;
a null variable name unlinks. UI and MCP use the same validation and history.

## Project variable libraries

**Share across project** gives a local variable a stable identity and adds it to
every current page; unrelated name collisions reject the operation. **Import from
another page** links one variable under a chosen local alias. Ordinary edits stay
local until **Publish value** updates matching identities across pages. New pages
can import existing variables explicitly. **Rename across project** preserves
bindings and rejects conflicting names; **Remove across project** removes bindings
while retaining resolved appearance. **Make local** detaches only this page and
keeps its object bindings. All multi-page changes preflight locks and use grouped
Undo. Native saves, templates and clipboard retain library identity.

MCP exposes `list_project_variables`, `share_project_variable`,
`import_project_variable`, `publish_project_variable`, `rename_project_variable`,
`remove_project_variable` and `detach_project_variable` on the live project host.
