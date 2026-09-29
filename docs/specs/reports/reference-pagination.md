# Folder-reference MCP pagination

The reported Claude session successfully called `get_reference_attachments`, but its 262,135-character result exceeded the provider's tool-result limit. Claude saved it as an overflow file. Emulsion intentionally exposes no general shell/file reader to that session, and the previous reference tool had no offset/limit parameters, so the agent could not read the attachment.

`get_reference_attachments` now accepts UTF-8 byte offsets and a bounded page size (12,000 bytes by default, maximum 16,000). Responses contain pagination metadata plus a text block, with `next_offset`, `has_more`, and total/returned byte counts. UTF-8 boundaries are preserved; invalid offsets and limits return errors. Images appear only on the first page. `attach_reference_folder` also returns the first bounded page instead of the entire snapshot.

The turn prompt tells the assistant to follow the page cursor through all captured text without shell access or asking for a smaller attachment solely because there are unread pages. Folder-only turns no longer begin with a misleading “no reference image” notice. Existing snapshot limits and omission notices still apply: paging retrieves the captured snapshot, not source files omitted during attachment.

Regression coverage includes a live MCP relay reading and reconstructing a folder snapshot larger than 250 KB with every serialized response below 20 KB, plus Unicode boundaries, invalid cursors, end-of-text, image reference contents, clipboard references, and the Library host. This verifies the failed transport path; it does not claim a new paid Claude architecture-generation run.
