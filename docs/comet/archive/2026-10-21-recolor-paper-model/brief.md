# Outcome

A paper model's paint scheme can be changed while everything that makes it look real stays: the app finds the model's paint colours as a palette, the user gives any swatch a new colour, and every area painted that colour changes on all pages — rivets, panel lines, part joints, and weathering keep their light/dark detail (adjustable), and insignia, decals, or other areas the user protects (MobileSAM click) stay untouched. It works on the Parts Layout pages (so the A4 export is recoloured) and as Image ▸ Recolor Paint… on any document; a page loaded from a PDF (or an image folder) can be opened in the editor, recoloured or edited there, and sent back to its Parts Layout tab.

# Scope

- Palette: the paint colours of the pages (or the layer) found by colour clustering in CIELAB (up to 8 swatches; paper and near-black ink excluded; similar clusters merged), each with its share of the painted area.
- Mapping: each swatch can get a new colour (colour picker); a click on the page picks the swatch under it; "Swatch tolerance" decides which pixels belong to a swatch (judged on a lightly smoothed image so rivets and lines inside a painted area count as part of it).
- Shading transfer: inside a mapped swatch's area, each pixel keeps its difference from the swatch's base colour (lightness and colour) on top of the new colour, scaled by "Detail strength" (0–150 %, default 100 %).
- Protect: in the Parts Layout tab, "Protect (MobileSAM)" — a click outlines an area (e.g. insignia, canopy) that is never recoloured; protected areas are shown and can be removed; without MobileSAM the button says why. In the editor, recolouring applies through the selection (as every filter), so areas are protected by selecting around them (Extract Object → To Selection uses MobileSAM).
- Parts Layout: Recolor… window with the palette of all pages, live preview on the page view, Apply (full resolution, on a worker thread, with progress), and Revert to the original colours; parts and the A4 layout are kept (their images are re-cut from the recoloured pages, no re-pack), and Export A4 Pages… uses the new colours.
- Editor: Image ▸ Recolor Paint… dialog on the active layer (or the selection) with the same palette, mapping, tolerance, and detail controls, live preview, and one undo step.
- Recolour Area (Parts Layout): a mode in the Recolor… window where dragging on a page draws a rectangle and Shift-dragging a freehand lasso; once any area exists, only pixels inside areas are recoloured (pages without areas stay unchanged), the palette is recomputed from the areas only (new colours already chosen are kept for matching swatches), areas show as outlines on the pages, and Clear Areas removes them. Protected pixels stay protected inside areas.
- Colour entry: each swatch row has R, G, B number fields (0–255) and a #RRGGBB hex box beside the colour button, in both the Recolor… window and Image ▸ Recolor Paint…; typing a value maps the paint at once (the paint's own colour means unchanged).
- Open Page in Editor: right-clicking a page in the Parts Layout tab opens it (its current 300-dpi image) as an editor document linked to that page; in the editor, Image ▸ Update Parts Layout Page sends the flattened document back and replaces the page (parts re-cut, no re-pack, Export A4 Pages… uses it); Revert to Original still restores the PDF render. The command is disabled, with a reason, when the document is not linked, its Parts Layout tab was closed, or the canvas size no longer matches the page.
- Fixture: generated paper-model sheets with known paint colours, rivets, panel lines, and a white insignia.

# Non-goals

- Trained recolour (image-to-image) models; recolouring by text prompt.
- Changing insignia/decal designs, adding new markings, or painting new camouflage patterns.
- Recolouring vector content of the PDF (pages are recoloured as 300-dpi images).

# Acceptance examples

- A1: On the fixture sheets the palette lists every paint colour (each known paint colour matched by one swatch within ΔE 8), with paper, black ink, and the white insignia not offered as paint swatches unless they cover painted area; detection takes under 2 s per A3 page (release).
- A2: Mapping one swatch (e.g. olive → blue) changes exactly that paint: in its areas the base colour becomes the new colour (mean ΔE to the target under 5 away from markings), every other swatch, the paper, and the insignia are unchanged (ΔE < 1), on all pages.
- A3: Markings survive: the lightness contrast of rivets and panel lines against their surrounding paint is kept within 10 % at Detail 100 %, removed to under 10 % of the original at 0 %, and increased at 150 %; part joints/outlines stay dark.
- A4: In the Parts Layout tab, Protect (MobileSAM) outlines a clicked area (the insignia) and recolouring leaves every protected pixel exactly unchanged; protected areas show on the page and can be removed; without MobileSAM the control is disabled with a reason.
- A5: Parts Layout Recolor…: the preview follows every change within 200 ms (release); Apply runs on a worker with progress while the UI stays responsive; afterwards parts keep their outlines and placements (no re-pack), Export A4 Pages… shows the new colours, and Revert restores the original pages exactly.
- A6: Image ▸ Recolor Paint… on a document: palette of the active layer (within the selection when there is one), the same controls, live preview, Cancel restores, OK is one undo step, and pixels outside the selection are unchanged.
- A7: Applying a recolour to an A3 page at full resolution takes under 3 s (release).
- A9: In the Parts Layout Recolor… window, Recolour Area mode: a rectangle (drag) and a lasso (Shift-drag) on page 1 limit recolouring — after mapping olive, olive inside the areas changes and olive outside them and on page 2 is unchanged (preview and Apply); the swatches offered are those inside the areas; areas show on the page; Clear Areas returns to whole-page recolouring.
- A10: Typing R, G, B values or a #RRGGBB hex code in a swatch row (both windows) sets that paint's new colour exactly and previews it; invalid hex is ignored without changing the colour; typing the paint's own colour leaves it unchanged.
- A8: Right-click a PDF page in the Parts Layout tab ▸ Open Page in Editor opens a document with exactly that page's pixels; after Image ▸ Recolor Paint… (or any edit) there, Image ▸ Update Parts Layout Page replaces that page in the tab — the edited pixels appear on the page and in Export A4 Pages…, parts keep their outlines and placements (no re-pack), other pages are unchanged, and Revert to Original restores the PDF render exactly; the command is disabled with a reason for unlinked documents, a closed tab, or a changed canvas size.

# Constraints and invariants

- Recolouring never changes protected pixels, pixels outside the selection (editor), paper, or swatches without a new colour.
- The original pages are kept so Revert is exact.

# Decisions

- Available in the Parts Layout tab and as Image ▸ Recolor Paint… in the editor.
- Parts Layout area limit: drawn areas (rectangle + Shift-lasso); pages without areas unchanged; palette from the areas (added 2026-10-21).
- Colour entry: R G B fields plus hex box per swatch (added 2026-10-21).
- PDF pages can be opened in the editor (page right-click) and the result sent back to the linked page (added 2026-10-21).
- Conventions adopted without asking for A8: one-layer document named "<file> – page N"; Update Parts Layout Page in the Image menu sends the flattened image; a canvas-size change blocks the update.
- Palette mapping (consistent across all pages); click picks a swatch.
- Shading kept automatically with a Detail strength slider (0–150 %).
- Local AI: MobileSAM to protect areas (Parts Layout) and, in the editor, via Extract Object → To Selection; recolouring itself is colour clustering + shading transfer (no trained recolour model).
- Conventions adopted without asking: CIELAB k-means palette (up to 8, merged within ΔE 12); smoothing radius 0.5 mm for swatch membership; default tolerance ΔE 18; generated fixture with known colours.
- Workspace: branch `comet/recolor-paper-model` off master.

# Open questions

- None. User confirmed outcome, scope, decisions, acceptance A1–A7, and non-goals on 2026-10-21; A8 (open a PDF page in the editor and send it back) added on request and confirmed the same day; A9 (drawn recolour areas) and A10 (RGB/hex entry) added on request and confirmed the same day.

# Verification expectations

- `cargo test`, clippy `-D warnings`, `fmt --check`, existing release benchmarks (and the release PDF UI run), and new recolour benchmarks.
- Unit tests: Lab conversion, palette clustering, membership, shading transfer (detail 0/100/150 %), protection.
- Fixture tests: palette vs known colours, mapping accuracy, marking contrast, untouched areas.
- Headless UI tests: Parts Layout Recolor… (map, preview, Apply in background, Revert, export), MobileSAM protect (with GAC_MODEL_DIR), editor Image ▸ Recolor Paint… with selection, preview, Cancel, undo; Open Page in Editor → edit → Update Parts Layout Page → export → Revert, and the disabled cases; Recolour Area rectangle + lasso limit, palette from areas, Clear Areas; RGB/hex entry through the real widgets.
