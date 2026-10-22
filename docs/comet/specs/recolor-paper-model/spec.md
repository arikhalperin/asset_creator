# Recolour Paper Model

## Purpose

Change a paper model's paint colours while keeping its markings and detail.

## Requirements

### Palette

- Pixels are converted to CIELAB (D65). Paper (the page's paper colour, ΔE < 6) and near-black ink (L < 18) are excluded. k-means (k = 8, seeded deterministically, on a sample of at most 200 000 pixels) clusters the rest; clusters within ΔE 12 merge; clusters under 0.5 % of the painted pixels are dropped. Each swatch: base Lab colour, share of the painted area.
- Membership: paper, ink, and transparent pixels never belong. A pixel is an own member of the swatch nearest its colour when within the tolerance (ΔE, default 18, 4–40). Every paint pixel then belongs to the swatch whose own members fill most of the 0.5 mm box around it, when they fill at least 35 % of it (so rivets and panel lines join the paint around them, while outlines, blends of ink and paper, and a darker shade of one paint inside another do not join a neighbouring paint).

### Mapping and shading transfer

- A mapping gives some swatches a new colour. For a member pixel p of a mapped swatch with base b and new colour n: Lab(p') = Lab(n) + (Lab(p) − Lab(b)) × detail, with detail 0–1.5 (default 1.0); converted back to sRGB and clamped; alpha unchanged. Non-member pixels, unmapped swatches, protected pixels, and (editor) pixels outside the selection are unchanged.

### Protect (Parts Layout)

- "Protect (MobileSAM)" mode: a click on a page sends a point prompt to MobileSAM (existing segmenter); the returned mask is added to that page's protected mask, shown as a hatched overlay; "Clear Protected" removes a page's protection. Disabled with "Needs the MobileSAM model" when not installed.

### Parts Layout

- Recolor… opens a window: swatches (base colour, area %, new colour picker or "keep"), Tolerance, Detail strength, Apply, Revert, Close. Clicking a page while the window is open selects the swatch under the pointer. The page view previews the mapping on its preview images (live). Apply recolours every page at full resolution on a worker thread (progress bar, Cancel); on completion the page images are replaced, each proposal's cut-out and the layout items' images are re-made from the new pages (outlines and placements unchanged), and manual-frame sheets use the new pages. Revert restores the original pages and part images.

### Editor

- Image ▸ Recolor Paint… (filter-style dialog on the active layer; with a selection, palette and changes are limited to it): swatches, new colours, Tolerance, Detail strength, live preview on the canvas, Cancel (restores) and OK (one undo step "Recolor Paint").

### Recolour Area (Parts Layout)

- "Recolour Area" toggle in the Recolor… window (exclusive with Protect): on a page, a drag draws a rectangle, Shift + drag a freehand lasso (polygon of pointer positions, closed on release); tiny drags (< 2 mm) are ignored. Areas are per page, kept while the tab is open, drawn as outlines, removed with "Clear Areas" (all pages).
- With any area, the allowed mask per page is the union of its areas (pages without areas: nothing allowed); preview and Apply change only allowed, unprotected member pixels. The palette is recomputed from the pixels inside areas (all pages); targets are kept for new swatches within ΔE 12 of a previously mapped swatch. Clearing areas recomputes the palette from the whole pages the same way.

### Colour entry

- Each swatch row: swatch, area %, →, colour button, R/G/B drag values (0–255, typed or dragged), a #RRGGBB text box (applied when it parses; otherwise ignored), and Keep when mapped. A value equal to the paint's own colour means unchanged.

### Open Page in Editor

- Right-click a page in the Parts Layout tab ▸ Open Page in Editor: a new one-layer document "<file> – page N" with the page's current image (300 dpi), linked to (tab, page).
- Image ▸ Update Parts Layout Page (linked documents only): the flattened document replaces that page's image in the tab through the same path as a recolour Apply (proposal cut-outs and layout item images re-made, placements unchanged, manual frames use the new page); the original render is kept for Revert to Original. Disabled with a reason when the document is not linked, the tab is closed, or the canvas size differs from the page.

### Performance

- Palette per A3 page < 2 s; full-resolution apply per A3 page < 3 s; preview update < 200 ms (release).

### Fixture

- `tests/fixtures/recolor/`: generated A3 sheets (PyMuPDF) with panels in known paint colours (olive, sand, grey, light blue), rivet rows (small dark dots), panel lines, part outlines, and a white star insignia; `recolor.json` lists colours, rivet and line positions, and the insignia box.
