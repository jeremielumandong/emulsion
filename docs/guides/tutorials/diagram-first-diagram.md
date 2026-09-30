# Tutorial: your first diagram

This tutorial builds a small approval flow inside a swimlane, styles it, and
exports it for draw.io and as an image. It takes about fifteen minutes.

## What you'll make

A four-step flow with connected shapes and a decision, arranged automatically,
grouped in a swimlane and styled with a theme. You save it as an editable `.emu`
project and export a `.drawio` file and a PNG.

## Prerequisites

- Emulsion is installed and running.
- Optional: draw.io or diagrams.net, to open the exported `.drawio` file.

## Steps

1. **Start a blank diagram.** On Home, choose **Diagram**. In the
   **New document** dialog, choose **Blank document**, then **Create**.
   You should see an empty page, the drawer with the **Shapes** tab open, and
   the tool strip below the drawer tabs.

2. **Add shapes.** In the **Shapes** tab, click **Start / End**, **Process**,
   **Decision** and **Process** again. Drag each shape to a free spot on the
   page.
   You should see four shapes with white fills and thin dark outlines.

3. **Name the shapes.** Select a shape, open the **Text** tab in the properties
   panel, and choose **Edit label…**. Label the four shapes `Request`,
   `Review`, `Approved?` and `Publish`.
   You should see each label centred in its shape.

4. **Connect the shapes.** Choose **Connect shapes** in the tool strip. Click
   **Request**, then **Review**. Connect **Review** to **Approved?**, and
   **Approved?** to **Publish** in the same way. You can also drag from a
   shape's port to another shape.
   You should see arrows with right-angled bends. Select one and the connector
   toolbar shows **Elbow**, the default routing.

5. **Arrange the flow.** Open **Arrange diagram** in the tool strip and choose
   **Left to right**.
   You should see the four shapes in one row, in connection order, with the
   arrows rerouted.

6. **Add a swimlane.** Open the **Containers** tab and add a **Swimlane**. Drag
   and resize it with its corner handles so it surrounds the row of shapes.
   Select each shape, and in the properties panel open
   **Move into container** and choose the swimlane.
   You should see the shapes move with the swimlane when you drag it.

7. **Apply a theme.** Open the **Themes** tab, choose **Entire page**, and
   click **Soft teal**.
   You should see every shape and connector restyled in one step. Press
   Ctrl+Z (Cmd+Z on macOS) to compare, then redo.

## Save and export

1. Choose **File → Save**, name the diagram, and save it.
   You should see the name in the document tab. The `.emu` project keeps
   history and every native effect.
2. Choose **Export editable .drawio…** in the tool strip and pick a
   destination.
   You should see a status message that the editable diagram was exported.
3. Choose **File → Export…**, select **PNG**, and keep the **2×** output
   size. Choose **Export…** and pick a destination.
   You should see a sharp PNG at twice the page's pixel dimensions.

## Optional: generate the flow from text

1. Open **Generate from data** in the tool strip and choose **Text flow**.
2. Replace the sample with these lines, then choose **Apply**:

   ```text
   Request
   Review
   Approved?
   Publish
   ```

   You should see a new page with four connected shapes, one per line.

## Next steps

- Learn text, CSV, Mermaid and SQL generation, Lucid import, themes and layout
  locks in [Diagram functionality and compatibility](../diagram-functionality.md).
- Compare Diagram with the other workspaces in
  [Emulsion workspaces compared](../workspaces.md).
