# PDF Parts Layout

## Purpose

Re-lay out paper-model parts from large (A3) PDF sheets onto printable A4 pages at true scale.

## Requirements

### Opening

- File ▸ Open accepts `.pdf`. PDFium (pdfium-binaries build for the platform, SHA-256 pinned) is downloaded to `<app data>/pdfium/` after consent (same consent/progress/failure flow as the models); `GAC_PDFIUM_DIR` overrides. Pages render at 300 dpi on a worker thread; each page records its size in mm.

### Image folders

- File ▸ Open Folder of Images… (folder picker) loads the folder's `.png`, `.jpg`/`.jpeg`, `.bmp`, `.webp` files (case-insensitive; others ignored), sorted by natural file-name order, as the pages of one Parts Layout tab named after the folder, on the worker thread with progress.
- Page size: the DPI stored in the file (PNG pHYs, JPEG JFIF density) when present; otherwise the image is taken as A3 (297 × 420 mm portrait, or landscape when wider than tall; the scale fits the image inside that sheet). Pages are resampled to 300 dpi for detection, layout, and export.
- Each image page's header shows "W × H mm (from file DPI)" or "(assumed A3)" and a DPI field with Apply: the page is reloaded from its file at that DPI on a worker thread and its parts are detected again (the page's previous parts are replaced).
- An empty folder (no images) shows "No images in <folder>" and opens no tab.

### Parts Layout tab

- Left: source pages (scrollable) with part proposals outlined and numbered; right: A4 sheets with placed parts; bottom: tray of unplaced parts, oversized parts marked with a warning icon and "does not fit on A4: W × H mm".
- Actions: Merge (two or more selected proposals), Delete, Refine (MobileSAM click, when installed), Re-pack, Add Sheet, Remove empty Sheet, Rotate 90° (selected placed part), Split… (oversized or any part), Export A4 Pages….

### Detection

- Paper colour estimated per page; ink = pixels differing from it; ink is grouped with a closing distance of 1.5 mm so a part's outline, fold lines, glue tabs, and its printed number stay one part; a part's mask includes everything enclosed by its outline, and smaller proposals lying inside a part's mask are merged into it; regions smaller than 4 mm² are dropped; each part keeps its pixels cut along its outer contour (holes kept), on a transparent background.
- Clicks select the smallest proposal under the pointer. MobileSAM refine: a click inside a part (its mask, enclosed area included) replaces its mask with the MobileSAM mask; a click outside all parts adds a part.

### Packing

- Sheet: A4 portrait 210 × 297 mm, margin 5 mm, spacing 2 mm. Parts sorted by height (then area), placed by shelf/max-rects packing with 0°/90° orientations; a new sheet when needed. Manual moves snap to free space and are refused if they overlap or leave the printable area.

### Split

- The user draws a straight line across a part; the part splits into two pieces along it; the piece the user chooses in the Split window (default: the smaller) gets a glue strip of the given width (0–20 mm, default 8 mm) copied from the other piece's content along the cut, outlined; alignment marks (short ticks every 20 mm, at least two) are drawn across the cut on both pieces. Pieces that still do not fit stay flagged.

### Manual decompose

- Each page has a "Manual (A4 frames)" toggle. When on, the page's parts are removed from the layout (and come back when it is turned off); the page shows its A4 frames: full A4 rectangles (210 × 297 mm) at true scale, portrait or turned 90°; the first frame is turned to match the page (landscape on a portrait page, portrait on a landscape page) so two frames tile an A3 page. Add Frame (placed at the page's top-left, or next to the last frame, next row when past the page's right edge), Remove Frame (selected), Rotate Frame 90°, and free dragging move frames; frames may overlap and may extend past the page edge (outside the page prints white). The page view zooms (−, +, Fit, and Cmd/Ctrl+scroll; 100 %–800 %) and scrolls both ways.
- Each frame becomes one portrait A4 sheet filled edge to edge with the framed page area at true scale (a landscape frame's content turned 90° clockwise).

### Export

- One PDF: packed sheets first, then frame sheets (page order, then frame order); each sheet one A4 page (595.28 × 841.89 pt) containing a 300-dpi image of the sheet; optional PNG per sheet. Parts keep their size (±0.5 mm).

### Performance

- Render A3 page < 2 s; detection per page < 2 s; pack 100 parts < 1 s (release).
