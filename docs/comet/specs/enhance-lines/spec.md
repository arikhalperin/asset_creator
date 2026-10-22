# Enhance Lines

## Purpose

Make faded, broken, or grainy dark line work (insignia outlines, flap and panel lines, rivets) on paper-model pages clear again inside a chosen area, without changing the paint.

## Requirements

### Line detection

- Work on CIELAB lightness at the image's scale (`px_per_mm`: 300 dpi pages → 11.81). A 3×3 median removes grain and halftone dots when the image has at least 10 px/mm (below that 3 px is wider than a line, so no median); a 3×3 mean of the result is the "smooth" copy. Background = morphological closing (square grey dilation then erosion, radius ≈ 0.6 mm) of the smooth copy. A pixel is a line pixel when the smooth copy is at least 4 L* below the background there and the pixel itself (median) is at least 4 L* below it; dark features thinner than about 1.2 mm qualify (wider ones survive the closing). Line pixels with fewer than two line neighbours are dropped (grain specks). Strokes lighter than their surroundings (by 4 L* against a grey opening) are light lines: never lines, never smoothed.

### Effects

- Line strength s (0–1, default 0.6): each line pixel's L*, a*, and b* are multiplied by 1 − 0.85·s (a darker shade of the same colour, hue kept, near-black at s = 1). The line mask is binary, so the paint-to-line transition stays at most 2 px.
- Close gaps g (0–2 mm, default 0.5): the line mask is closed (square dilation then erosion) with radius g/2; newly covered pixels take the mean colour of the line pixels within 1 mm, then are darkened as above.
- Thickness t (1.0–2.0×, default 1.0): the line mask is widened by (t − 1) × w / 2 on each side using the Euclidean distance to the mask (the same in every direction, so curves and diagonals widen like straight lines), where w is the area's typical stroke width = line area ÷ line length (the length from the area a Euclidean dilation adds between radius 2 and 3 px, also independent of angle); the outermost added pixels are blended by how much of them the wider line covers (≤ 1 px soft edge, so fractional widths come out right); new pixels are coloured like gap pixels.
- Smooth grain m (0–1, default 0.3): paint pixels (not lines, light lines, or transparent) are blended by m with the mean of the paint pixels in a 0.25 mm box around them; lines and light lines are not smoothed and do not bleed into the paint.
- With s = 0, g = 0, t = 1, m = 0 nothing changes. Alpha is never changed.

### Parts Layout

- Toolbar "Enhance Lines…" opens a window: Line strength, Close gaps, Thickness, Smooth grain, Draw Areas toggle (drag = rectangle, Shift + drag = lasso, < 2 mm ignored), Clear Areas, Apply, Revert to Original, Close. Areas are the window's own (separate from Recolour Area), shown as outlines. Without areas, Apply is disabled ("Draw an area first").
- The page view previews the enhancement inside the areas on the preview images (live, ≤ 200 ms per change in release; large areas are previewed at a reduced scale, a budget shared by the pages with areas). After Apply the preview pauses (the pages already carry the result) until a setting or an area changes or the pages are reverted.
- Apply enhances the areas at full resolution on a worker (progress, Cancel); page images are replaced via the same path as recolour (cut-outs and layout items re-made, placements unchanged); the original pages are kept so Revert to Original restores the PDF render exactly.

### Editor

- Filter ▸ Enhance Lines… is a registered filter with the four parameters (millimetre parameters converted with the document's DPI; Open Page in Editor documents now carry their page's DPI, 300 for PDF pages), previewed through the filter dialog, limited to the selection, with Cancel and OK (one undo step "Enhance Lines").

### Performance

- A whole A3 page at 300 dpi < 3 s; preview update < 200 ms (release).

### Fixture

- Generated in the tests (deterministic, no files): camouflage patches (dark green, light green, tan) with grain (per-pixel noise of σ ≈ 4 L*), a faded ring insignia outline, a 45° diagonal stroke, and flap/panel lines (contrast ≈ 10 L* below their paint, 0.25 mm wide) with gaps of 0.3, 0.6, and 1.2 mm, and a white 0.3 mm line on dark green paint.
