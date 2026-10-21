# Pattern Extraction

## Purpose

Turn repeating wall and floor textures into minimal, seamless, tileable game assets.

## Requirements

### Detection

- Input: the active raster layer cropped to the selection's bounding box (whole layer without a selection), composited over neutral gray, converted to luminance, mean-removed.
- Periods: normalized autocorrelation (FFT-based); the horizontal and vertical periods are the smallest lags whose correlation peak is strong (with integer-multiple checks so a harmonic is never preferred over the fundamental) and that repeat at least twice inside the region. The minimal tile is the smallest axis-aligned rectangle that tiles the region by translation (running bond → two courses).
- Phase: the tile origin is placed where the wrap-around edges differ least, so the proposed tile is as seamless as possible.
- Result: period W × H, confidence 0–1, tile rectangle in layer coordinates. With fewer than two repeats per axis or confidence below the threshold: "No repeating pattern found", tile = whole region.

### Pattern window

- Extract ▸ Find Repeating Pattern… (needs an editable raster layer). Non-modal window: Detect (re-runs on the current selection/layer), period and confidence, X/Y/W/H fields, Manual crop toggle, 3×3 tiled preview, Make Seamless with Blend width 5–50 % (default 25 %) and Revert, Add to Library, Close.
- The tile is outlined on the canvas. In manual crop mode, dragging inside the outline moves it, dragging its edges/corners resizes it, dragging outside draws a new one; values are clamped to the layer and shown in the fields. The preview updates live.

### Make Seamless

- Same size as the input. Offset by half the size, cross-fade the seams over the blend width with the mirrored/opposite content, then a patch-based fix-up of the central cross (copy best-matching patches from the tile interior) so no straight seam remains. Alpha is processed like color.
- Filter ▸ Make Seamless… (Blend width parameter) applies it to the active layer, or to the selection's bounding box, with the standard filter preview, Cancel, and one undo step.

### Library and export

- Default categories gain "pattern" (existing libraries get it at load when missing). Add to Library stores the tile with category "pattern" (manual), origin method "pattern", params `{period: [w, h], tile: [x, y, w, h], seamless, blend}`. Auto-tagging excludes "pattern" from its candidates.
- Export format "Tileable PNG": a folder; per asset `<name>.png` at its size or, with Repeat N×M (1–16), the tile repeated N columns × M rows; animations export each frame this way (`<name>_01.png`, …). Names never overwrite (same rule as other exports).

### Seam metric

- `seam_ratio(tile)` = mean |Δ| RGB across the wrap edges (column W-1 vs 0, row H-1 vs 0) ÷ mean |Δ| between adjacent interior columns and rows. Seamless: ≤ 1.15. Raw crops of the fixture textures (fixed set, sizes not lined up with the texture's repeat) score > 1.15.

### Performance

- Detection on 1024×1024 under 500 ms; Make Seamless on 512×512 under 200 ms (release).

### Fixtures

- `tests/fixtures/patterns/`: CC0 brick, stone, and cobblestone textures (ambientCG, downscaled, LICENSE.txt) tiled a known number of times, synthetic running-bond brick, JPEG q80 and noisy variants; `patterns.json` lists each image's known tile size.
