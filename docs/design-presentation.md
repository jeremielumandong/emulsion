# Design presentations

Emulsion can present a local multi-page Design project with a separate presenter
window. The audience window uses the existing canvas renderer and supported
embedded video player. Speaker notes and transition settings are saved per page
inside the editable project.

## Prepare a page

Open **Animate** in Design. **Speaker notes…** opens a private text area for the
current page; Save notes commits one undoable change, while Cancel preserves the
saved notes. Notes support up to 20,000 characters per page.

Choose a **Page transition**: None, Fade, Slide or Zoom. Set its duration between
100 and 3000 milliseconds. Transitions introduce the incoming slide; existing
object entrance and exit animations continue to use their separate timing
controls. A page stays onscreen for at least its transition duration. Page
transitions use temporary presentation images, then return to the native canvas;
they never rewrite editable shapes, text or effects.

## Present to an audience

**Present** opens the audience canvas in the current window. Its windowed controls
provide Previous, Next, Fullscreen, Presenter view, automatic advance and Exit.
In fullscreen the audience sees only the slide, black surrounding space and
an embedded video when playing. Application tabs, toolbars, status messages,
editing controls, notes and presenter controls are hidden. No controls appear
when moving the pointer over the audience slide.

Audience keyboard shortcuts:

| Key | Action |
| --- | --- |
| Right, Down, Page Down, Space | Next page. |
| Left, Up, Page Up, Shift+Space | Previous page. |
| Home / End | First / last page. |
| F | Toggle audience fullscreen. |
| P | Open or focus the presenter window. |
| Escape | End the presentation and restore the original editing page. |

Editing shortcuts are suppressed during presentation. Navigation stays within
the project and does not add undo steps or modify authored objects. Ending the
presentation restores the page and selection from which it began.

## Presenter window

**Open presenter view** or **Presenter view** opens a separate window while the
original window becomes the clean fullscreen audience. Move the presenter window
to the presenter's display using the operating system's normal window controls.
Automatic external-display placement is not provided.

The presenter window contains:

- Current and next slide previews, page position and title.
- The current page's saved speaker notes.
- An elapsed presentation timer with Pause/Resume and Reset.
- Previous/Next, automatic advance, Show audience and End presentation.
- Stop video while a video is active, and playback errors that are hidden from
  the fullscreen audience.

Closing the presenter window or choosing End presentation ends the session and
returns the audience window to editing. Preview images show the authored slides;
they do not mirror live video frames or animate alongside the audience.

## Video and automatic advance

[YouTube video objects](design-video.md) play in their frames on the audience
slide. Normal video keys, including Space, belong to the focused player.
Automatic page advance waits while a video player is open. Stop the video or use
presenter navigation to move on. Changing pages stops playback.

Native web views on Windows and macOS can receive keys directly, including
Escape. The separate presenter's End presentation and Stop video controls remain
available when the embedded player has focus. The video source, internet access
and installed system runtime still determine whether playback is available.

## Persistence and limits

Speaker notes, page transition type and transition duration round-trip in native
projects. Older documents use empty notes and no page transition. Notes are not
included in slide artwork or ordinary image/PDF exports. Presenting does not
change the project's authored document data or undo history.

This workflow does not yet provide remote controls, recording, automatic
multi-monitor assignment, live previews of video in the presenter window or
interactive HTML export. Existing animated export handles object animation;
presentation-only page transitions are not included in GIF export.
