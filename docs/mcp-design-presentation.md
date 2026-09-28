# Design presentation tools for MCP

The same native page metadata powers Emulsion's Design UI and MCP tools. These tools operate on the active page; use the project/page tools to change the authored page first. Saved notes, transitions, animations, and YouTube associations travel with the editable project. No browser engine or video downloader is bundled by these tools.

| Tools | Behavior |
| --- | --- |
| `get_design_presentation` | Read notes, page transition, duration, frame rate, and per-object animations. |
| `set_design_presentation` | Patch notes, transition, duration, or frame rate; omitted fields stay unchanged. |
| `set_design_motion`, `remove_design_motion` | Author or remove native node animations. Locked nodes and locked ancestors cannot be changed. |
| `list_design_videos` | Read YouTube links, native group IDs, and visible poster bounds. |
| `add_design_video`, `update_design_video`, `detach_design_video` | Create/update validated YouTube links or detach a link while retaining its editable poster. |
| `get_presentation_state` | Read actual live audience state, current page, fullscreen, automatic advance, animation, video, and presenter status. |
| `start_presentation` | Start at the current page, with optional fullscreen, separate presenter window, and automatic advance. |
| `navigate_presentation` | Move `next`, `previous`, `first`, or `last`. Boundary navigation stays on the boundary page. |
| `set_presentation_fullscreen` | Enable or disable the clean fullscreen audience view. |
| `end_presentation` | Stop playback and restore the original editing page, selection, and view. |

Native metadata changes are atomic and undoable. Invalid fields, unknown arguments, invalid URLs, and inconsistent timing return errors without partial changes. Shortening a page does not silently truncate existing animations: adjust those animations first. Durations use milliseconds. Page duration is 100–60,000 ms, frame rate 1–60, and slide transition duration 100–3,000 ms. Notes allow up to 20,000 characters. An object animation's transition must fit within half its start/end interval.

For example, set page metadata with `{"speaker_notes":"Explain the two options.","page_transition":"fade","transition_ms":400,"duration_ms":5000}`. Add a YouTube poster with `{"url":"https://youtu.be/M7lc1UVf-VE?t=15","origin":[80,60],"size":[640,360]}`. Adding the link does not load a remote player.

Live tools require the running Emulsion UI host and its visible editor window. Offline document MCP servers return explicit host-required errors instead of claiming to play a presentation. Fullscreen defaults to true, automatic advance to false, and presenter view to false. Request presenter view with `{"presenter":true}`; it requires a fullscreen audience. Repeating start updates live options without replacing the original return page/view. The separate presenter window exposes notes; the audience shows slide content. A response may report `presenter_opening` while the native window is being created.

Host commands join the assistant's ordered operation queue. Before deferred execution they recheck the source document, presentation session, visible editor, workspace, and assistant generation. Closing a window or cancelling a request produces an explicit result and releases the queue. Presentation playback does not mutate authored slides. Keyboard Escape and presenter controls remain available to end a session.

YouTube playback remains subject to network access, the video's embed permission, and the platform's system web runtime. It requires an axis-aligned poster occupying at least 200 × 200 screen pixels. These tools author links and control the presentation session; they do not expose remote video seeking/playback automation. Static image/PDF exports contain the poster, not an interactive video. See [video behavior](design-video.md) and [presentation controls](design-presentation.md).
