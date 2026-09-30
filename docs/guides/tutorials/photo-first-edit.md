# Tutorial: your first photo edit

This tutorial retouches and grades a portrait in Photo. It takes about 15
minutes.

## What you'll make

A portrait with a blemish healed, the subject brightened with a masked Curves
layer, light sharpening, and a text caption. You save it as an editable `.ora`
file and export a PNG or JPEG copy.

## Prerequisites

- Emulsion is installed and running.
- You have a portrait photo in JPEG, PNG or TIFF format.
- If your photo is a camera RAW file, develop it in Library first. Follow
  [your first Library shoot](library-first-shoot.md) up to Develop, then choose
  **Edit in Photo…** and continue from step 2.

## Steps

1. **Open the photo.** On Home, choose **Open…** and select your photo. In Photo,
   **File → Open images…** (Ctrl+O) does the same.
   You should see the photo on the canvas and one layer in the Layers panel.

2. **Select the photo layer.** Click the photo's row in the Layers panel.
   You should see the row highlighted.

3. **Heal a blemish.** Press J to choose Heal. Press [ or ] until the brush is
   slightly larger than the blemish. Paint over the blemish in one short stroke.
   You should see the blemish replaced by surrounding skin texture.

4. **Select the subject.** Press Shift+W to choose **Quick select (AI)**. Brush
   over the subject; the selection grows through similar colour. If the AI model
   is installed, turn on **AI** in the options bar and click the subject
   instead. **Select subject** in the contextual bar is another option.
   You should see a marching-ants outline around the subject.

5. **Soften the edge.** Choose **soften 10** in the options bar.
   You should see the outline smooth slightly.

6. **Brighten the subject with Curves.** Press Shift+Q to choose Grade, then
   choose **Curves** in the options bar. The new Curves layer uses the selection
   as its mask. In **Properties**, drag the middle of the curve up a little.
   You should see only the subject brighten, and a Curves layer with a mask
   above the photo layer.

7. **Deselect.** Press Ctrl+D.
   You should see the outline disappear while the Curves layer keeps its mask.

8. **Sharpen the photo.** Click the photo layer in the Layers panel. Choose
   **Filter → Sharpen → Unsharp mask**. In **Properties**, lower the amount if
   the edges look harsh.
   You should see finer detail, and the photo layer marked as a smart layer
   with an editable Unsharp mask filter.

9. **Add a caption.** Press T to choose the Type tool. Click near the bottom of
   the photo and type a caption. Choose **Done** in the contextual bar.
   You should see a new text layer directly above the photo layer.

10. **Check the edit.** Click the visibility toggle of the Curves layer off and
    on again.
    You should see the subject switch between the original and graded looks.

## Save and export

1. Choose **File → Save** (Ctrl+S). Name the file and keep the `.ora` extension.
   The file keeps every layer, mask, filter and the editable text.
2. Choose **File → Export…** (Ctrl+Alt+Shift+W). Choose **PNG** for a lossless
   copy or **JPEG** for a small one, then choose **Export…**.
   You should see a flat image file in the folder you chose.

## Next steps

- Read the [Photo editing guide](../photo-editing.md) for every tool, adjustment,
  filter, blend mode and layer style.
- Compare Photo with the other workspaces in [workspaces](../workspaces.md).
- Develop RAW files in Library with [your first Library shoot](library-first-shoot.md).
