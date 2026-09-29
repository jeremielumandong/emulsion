# Assistant references

Use **Add reference** / **Attach files**, **Attach folder**, or **Paste reference**
beside the assistant input. References work in Library and document workspaces.
You can also focus the Reference panel and press **Ctrl+V**. Pasting into the
assistant's text input continues to insert ordinary prompt text.

Paste text, clipboard images, or copied files/folders. Attachments appear in the
Reference panel, where you can review and remove them individually. References
stay separate from artwork, layers and undo history. They remain fixed while an
assistant request is running and are kept for the current workspace session.

The assistant reads attachments with `get_reference_attachments`. Responses contain pagination metadata followed by a bounded text chunk (12,000 UTF-8 bytes by default, at most 16,000). Continue with `{"offset": next_offset}` until `has_more` is false. Images are included only on the first page. No shell access or provider-created overflow file is needed. CSV, JSON,
Markdown, source code and other UTF-8 text are included as text. Directly attached
images include previews. Folders include a listing and readable text contents;
images inside folders must be attached separately for visual inspection. Binary
formats such as PDF and office documents currently include metadata only; their
contents are explicitly marked as not extracted.

Attachments are snapshots: changes to a source file require attaching it again.
Up to 16 attachments are accepted. Folder snapshots visit at most 100 entries,
prioritize READMEs and manifests, visit folders breadth-first, and do not follow symbolic links. Generated folders (`target`, `build`, `dist`), dependencies (`node_modules`, `vendor`, virtual environments), caches, and common credential files (`.env*`, `.pem`, `.key`) are omitted from folder snapshots. Explicit file attachments are still supported. Text files are bounded to 64 KiB
per file and 256 KiB per attachment; pasted text allows 256 KiB. Truncated or
unreadable entries are marked in the material sent to the assistant.

## Diagrams from code or documentation

In a Diagram workspace, choose **Attach folder** and describe your requirements,
for example: “Diagram the browser request path through the API, authentication,
services and database. Use separate client/server containers and label protocols.”
The assistant reads the references, creates editable shapes and connectors, and
checks the rendered result. Its instructions require supporting source paths and
explicitly identified inferences. This needs a configured assistant provider.

MCP clients connected to the running app can call
`attach_reference_folder({"path":"/absolute/path/to/codebase"})`, then
`get_reference_attachments({})` and the diagram creation tools. Attaching a folder
does not itself generate a diagram: provide the diagram requirements to the agent.
This tool is app-hosted; the standalone document executor cannot attach references.
Large repositories may exceed snapshot limits: attach the relevant subsystem or
specific documentation files instead. This is not exhaustive repository analysis.
