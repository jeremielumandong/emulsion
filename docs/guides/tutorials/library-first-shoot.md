# Tutorial: your first Library shoot

This tutorial takes a folder of photos from import to export in Library. It
takes about 20 minutes.

## What you'll make

A culled set of photos with ratings and flags, one photo developed with a
graduated sky mask, the same development synced to similar frames, and exported
JPEG files in a folder you choose.

## Prerequisites

- Emulsion is installed and running.
- You have a folder with a few photos from one shoot. Camera RAW files work
  best; JPEG, TIFF, PNG and WebP also work.
- Ideally the folder is writable, so Library can save its sidecar files beside
  the originals. For a read-only folder, Library keeps them in its application
  data directory instead.

## Steps

1. **Open Library.** On Home, choose **Library**. From an editor, choose
   **File → Photo Library…**.
   You should see the Library screen with **Library** and **Develop** at the top.

2. **Import the folder.** Choose **Import folder…** in the toolbar and select
   your folder. **File → Import photo folder…** does the same.
   You should see thumbnails fill the grid. The files stay where they are.

3. **Look at one photo.** Click a thumbnail, then press E. Press the left and
   right arrow keys to move between photos. Press G to return to the grid.
   You should see the photo large in the loupe, then the grid again.

4. **Rate and flag.** Click in the grid so it has keyboard focus. For each photo,
   press 1–5 to rate it, P to pick it, or X to reject it. U removes a flag, and
   0 clears the rating. Press A to turn on auto advance so each key moves to the
   next photo.
   You should see stars and a flag on each thumbnail you marked.

5. **Open Develop.** Select your best photo and press D.
   You should see the photo in Develop with the adjustment sections on the
   right, starting with **Basic**.

6. **Set the basic tone.** In **Basic**, drag **Exposure** until the midtones
   look right. Lower **Highlights** and raise **Shadows** to recover detail.
   Drag **Temperature** and **Tint** until neutral tones look neutral.
   You should see the photo update after each change.

7. **Darken the sky with a linear mask.** Press M, or open **Masking** and choose
   **Linear**. Drag from the top of the sky down to the horizon. Choose the
   **−** button beside **Exposure** two or three times. Press O to show the mask
   overlay, and press O again to hide it.
   You should see a mask named "Mask 1" in the Masking section and a darker sky
   that fades toward the horizon.

8. **Save the edits.** Library saves edits automatically after a short pause.
   Choose **Save edits** to save at once.
   You should see **Save edits** turn inactive once the save finishes.

9. **Compare before and after.** Choose **Before / After** above the photo. Drag
   the divider across the photo, then choose **Before / After** again.
   You should see the original and edited versions on either side of the
   divider.

10. **Sync to similar frames.** Ctrl-click (Cmd-click on macOS) two or three
    similar photos in the filmstrip. Under **Sync:**, choose **All**, then
    choose **Sync to selected photos**. **All** copies every setting, including
    the mask.
    You should see the other photos take on the same look.

## Save and export

1. Press G to return to the grid, and select the photos to export. Click one
   photo, then Shift-click the last one to select a range.
2. In **Export settings**, choose **JPEG** under **Format**.
3. Choose **Choose folder…** under **Destination** and select an output folder.
4. Choose **Export** followed by the number of selected photos, for example
   **Export 5**.
   You should see a message that reports how many photos were exported.

Library never changes your originals. Each photo's edits are saved beside it as
`<original>.emulsion-raw.json`. Keep that file with the photo to keep its
edits. Mask and spot details are stored in the application data directory and
referenced from that file. To reuse the output settings later, choose **Save export preset…**.

To retouch one of the developed photos with layers, select it in Develop and
choose **Edit in Photo…**. Continue with
[your first photo edit](photo-first-edit.md).

## Next steps

- Read [Library and Develop](../library-develop.md) for every Develop section,
  masking and spot removal, HDR and panorama, and catalog tools.
- Compare Library with the other workspaces in [workspaces](../workspaces.md).
- See the [RAW recipe controls](../raw-development.md) for how RAW settings are
  stored and their limits.
