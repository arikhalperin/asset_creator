# Game Asset Creator — User Guide

This guide starts with the most common job — turning a video into game assets — and then covers the editor, extraction tools, the asset library, patterns, and paper-model sheets. Menu commands are written as `Menu ▸ Command`.

## 1. From a video to game assets

### Open the clip

`File ▸ Open…` (⌘O) and choose a video (MP4, MOV, MKV, WebM, or AVI). It opens in its own tab with a scrubber and three buttons: **Grab Frame**, **Extract Frames…**, and **Extract Assets…**.

### Extract Assets

1. Click **Extract Assets…**. Choose the frame range, how often to sample (every Nth frame), and the detection settings (**Minimum Size** and **Merge Distance**).
2. Click **Extract**. The run happens in the background with a progress bar; the editor stays usable, and **Cancel** stops it at once.
3. When it finishes, a **Review** tab opens. Each row is a cluster: a single sprite, or an animation (the same object across frames, played as a strip).

### Review

- Uncheck **Keep** on anything you don't want.
- Select two or more rows and click **Merge** to treat them as one animation.
- Click **Add Kept to Library**. Single sprites become assets; animations become animated assets with all their frames.

New assets are tagged automatically (see [Auto-Tag](#auto-tag)) and appear in the **Asset Library** panel.

### Export a sprite sheet

1. In the Asset Library, select the assets (⌘-click to add, or select a category).
2. Click **Export…** and pick a format:
   - **PNG files** — one PNG per asset; animations as numbered frames.
   - **Sprite sheet** — everything packed into one PNG with a JSON atlas (frame names, positions, sizes, and animation durations). Choose **Grid** or **Max-Rects** packing, padding, and **Power-of-two size**.
   - **Animation strips** — each animation as a horizontal strip with its atlas.
   - **Tileable PNG** — each asset repeated in a grid (for textures).
3. Choose where to save. The atlas JSON is written next to the sheet.

### Other ways to get frames

- **Grab Frame** opens the current frame as a new image document.
- **Extract Frames…** adds every Nth frame, or one frame per scene change, to the **Frame Set** panel, where **Open** turns frames into documents and **Remove** / **Clear** drop them.

## 2. Extracting from images

Open an image with `File ▸ Open…`. Every extraction can read the visible canvas (all layers) or just the active layer — the **Source** switch in each dialog — and respects the selection when there is one.

### Remove Background

`Extract ▸ Remove Background…` keys out a flat background. The color is chosen automatically; **Pick** takes it from a canvas click instead. **Tolerance**, **Edge Softness**, and **Contiguous** refine it, and the preview updates as you change them (computed in the background, so the canvas never freezes). **OK** applies it as one undo step; from the canvas it adds a new "Background removed" layer.

### Auto-Detect Objects

`Extract ▸ Auto-Detect Objects…` finds every separate object (keying the background first when the image has no transparency) and numbers them on the canvas. Adjust **Minimum Size** and **Merge Distance**, uncheck any you don't want, then **Add to Library**.

### Extract Object tool

Choose **Extract Object** in the Tools panel, then click an object or drag a box around it. With MobileSAM installed the outline follows the object closely; otherwise GrabCut is used. The first time, the tool offers to download MobileSAM (44.7 MB); **Cancel Download** stops it. Then **Add to Library** (Enter) or **To Selection**; **Clear** (Escape) starts over.

### Add a selection

Select anything with the selection tools and use `Extract ▸ Add Selection to Library`.

## 3. The asset library

The **Asset Library** panel shows thumbnails of everything you've extracted.

- **Search** filters by name and tags; the category buttons (chalice, skeleton, potion, deathtrap, wall, prop, pattern, other) filter by category.
- **Rename**, **Category**, and **Tags…** edit an asset. **Categories…** edits the list of categories.
- **Edit a Copy** opens the asset in the editor. After editing, `File ▸ Save Back to Library` stores a new version; **Versions…** lists and restores earlier ones.
- **Clean Up** removes stray specks and crops to the visible pixels. **Reveal in Finder** shows the file; **Remove…** deletes the asset.
- Drag a thumbnail onto a canvas to place it as a new layer.

### Auto-Tag

New assets get a category from OpenCLIP, a model that compares the picture with each category name. The first time, the panel offers **Download tagging model…** (156 MB; **Cancel Download** stops it). Without the model, new assets are tagged "other". **Auto-Tag** re-tags the selected assets; you can always change a category by hand.

## 4. The editor

### Documents and files

- `File ▸ New…` (⌘N) creates a blank document; `File ▸ Save` (⌘S) and `File ▸ Save As…` (⇧⌘S) keep layers in a `.gac` project.
- `File ▸ Export…` (⇧⌘E) writes a flattened PNG, JPEG, BMP, or WebP.
- `Edit ▸ Undo` (⌘Z) and `Edit ▸ Redo` (⇧⌘Z) step through the history.

### Tools

Move, Rectangle Select, Ellipse Select, Lasso, Polygon Lasso, Magic Wand, Brush, Eraser, Eyedropper, Paint Bucket, Gradient, Clone Stamp (Option-click sets the source), and Extract Object. The options for the current tool appear above the canvas.

### Selections

`Select ▸ All` (⌘A), `Select ▸ None` (⌘D), `Select ▸ Invert` (⇧⌘I), and `Select ▸ Grow…`, `Select ▸ Shrink…`, `Select ▸ Feather…`. The selection mode buttons (Replace, Add, Subtract, Intersect) combine new selections with the current one. Edits and filters apply only inside the selection.

### Layers

The Layers panel shows each layer's thumbnail, visibility, blend mode, and opacity. `Layer ▸ New Layer` (⇧⌘N), `Layer ▸ Duplicate Layer` (⌘J), `Layer ▸ Merge Down` (⌘E), and `Layer ▸ Flatten Image`. `Layer ▸ Add Layer Mask` hides parts of a layer without erasing them; `Layer ▸ Edit Layer Mask` paints the mask (black hides, white shows).

### Transform and fill

`Edit ▸ Free Transform` (⌘T) moves, scales, and rotates the layer or selection; `Edit ▸ Flip Horizontal` and `Edit ▸ Flip Vertical` mirror it. `Edit ▸ Fill Layer…` fills with a color.

### Filters

The Filter menu has adjustments (Brightness/Contrast, Levels, Curves, Hue/Saturation/Lightness, Color Balance, Desaturate, Invert, Posterize), blurs, sharpening, noise, and `Filter ▸ Make Seamless…`. Filters with settings open a dialog with a live preview; **OK** applies them as one undo step and **Cancel** restores the image.

`Filter ▸ Enhance Lines…` makes thin dark lines — panel lines, rivets, insignia outlines — darker, joined, and bolder, and can smooth print grain in the paint around them: **Line strength**, **Close gaps** (mm), **Thickness** (×), and **Smooth grain**. Millimetre settings use the document's DPI.

### Recolor Paint

`Image ▸ Recolor Paint…` finds the paint colors of the active layer (inside the selection when there is one). Give any swatch a new color with its color button, its R G B values, or a #RRGGBB code; rivets, panel lines, and shading keep their light and dark detail, adjustable with **Detail strength**. One undo step.

### Clean Up

`Image ▸ Clean Up` removes stray, nearly transparent specks from the active layer and crops the canvas to the visible content.

### View

`View ▸ Zoom In` (⌘=), `View ▸ Zoom Out` (⌘−), `View ▸ Fit to Window` (⌘0), `View ▸ Actual Size` (⌘1), and `View ▸ Show Pixel Grid`. Pinch or scroll to zoom and pan.

## 5. Repeating patterns

`Extract ▸ Find Repeating Pattern…` finds the smallest repeating tile in the image (or selection) and shows it tiled. **Detect** searches again; drag on the canvas for a **Manual crop**; **Make Seamless** blends the tile's edges so it repeats without seams. Add the tile to the library to export it, for example as a **Tileable PNG**.

## 6. Paper-model sheets

### Lay out parts on A4

Open a PDF with `File ▸ Open…`, or a folder of scanned A3 pages with `File ▸ Open Folder of Images…`. The first PDF asks to download PDFium (3.5 MB). A **Parts Layout** tab shows the pages on the left and the A4 sheets on the right:

- Parts are detected automatically. **Merge**, **Delete**, **Split…**, **Rotate 90°**, and **To Tray** adjust them; **Refine (MobileSAM)** re-outlines a part with a click.
- **Re-pack** packs all parts onto A4 sheets again; drag parts between sheets yourself. **Add Sheet**, **Remove Empty Sheets**, and **Place Tray Items** manage the sheets.
- Turn on **Manual (A4 frames)** for a page to cut it into A4 frames you place and rotate yourself instead.
- **Export A4 Pages…** writes the sheets as a PDF (**Also PNGs** adds a PNG per sheet).

### Recolor the paint

**Recolor…** finds the model's paint colors across all pages. Give a swatch a new color (color button, R G B, or #RRGGBB), adjust **Tolerance** and **Detail strength**, and watch the preview. **Recolour Area** limits the change to rectangles you drag (Shift-drag for a lasso); **Protect (MobileSAM)** keeps clicked areas such as insignia untouched. **Apply** recolours every page in the background; parts keep their places. **Revert to Original** restores the PDF's colors.

### Enhance lines

**Enhance Lines…** works the same way on areas you draw on the pages: darker, joined, and bolder lines with smoother paint, then **Apply** and, if needed, **Revert to Original**.

### Edit a page in the editor

Right-click a page ▸ **Open Page in Editor** opens it as an image document. After editing, `Image ▸ Update Parts Layout Page` sends it back; the parts are re-cut from the new page without re-packing.

## 7. Models and where files are kept

| What | Size | Where |
|---|---|---|
| MobileSAM (Extract Object, Protect, Refine) | 44.7 MB | `~/Library/Application Support/Game Asset Creator/models` |
| OpenCLIP tagging model | 156.4 MB | `~/Library/Application Support/Game Asset Creator/models/clip` |
| PDFium (PDF pages) | 3.5 MB | `~/Library/Application Support/Game Asset Creator/pdfium` |
| Asset library | — | `~/Library/Application Support/Game Asset Creator/library` |

Each model downloads only after you agree, shows progress, can be cancelled, and is verified against a fixed checksum. To remove a model, quit the app and delete its folder; the app offers the download again when it is next needed.

## 8. Keyboard shortcuts

| Action | Shortcut |
|---|---|
| New / Open / Save / Save As | ⌘N / ⌘O / ⌘S / ⇧⌘S |
| Export | ⇧⌘E |
| Close / Quit | ⌘W / ⌘Q |
| Undo / Redo | ⌘Z / ⇧⌘Z (or ⌘Y) |
| Select All / Deselect / Invert Selection | ⌘A / ⌘D / ⇧⌘I |
| Free Transform | ⌘T |
| New Layer / Duplicate Layer / Merge Down | ⇧⌘N / ⌘J / ⌘E |
| Move Layer Up / Down | ⌘] / ⌘[ |
| Zoom In / Zoom Out / Fit / Actual Size | ⌘= / ⌘− / ⌘0 / ⌘1 |
