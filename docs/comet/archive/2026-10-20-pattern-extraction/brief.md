# Outcome

Wall and floor textures can be turned into game-ready tiles: a "Find Repeating Pattern" window detects the tile period of a repeating region by autocorrelation and proposes the minimal tile, the tile can be adjusted by hand with a live 3×3 tiled preview, any tile can be made seamless, and tiles are saved to the asset library in a "pattern" category and exported as tileable PNGs (optionally repeated N×M).

# Scope

- Period detection (autocorrelation) on the selection's bounding box, or the whole active layer when nothing is selected; proposes the minimal axis-aligned tile (period width × height) at a low-contrast phase.
- Extract ▸ Find Repeating Pattern… window: detected period and confidence, editable tile X/Y/W/H, manual crop mode (drag/move/resize the tile on the canvas), live 3×3 preview, Make Seamless (blend width), Add to Library.
- Make Seamless operation (offset + edge blending + patch fix-up), in the window and as Filter ▸ Make Seamless… on the active layer or selection (preview, undo).
- "pattern" library category; tiles saved from the tool are tagged "pattern" (manual); auto-tagging never proposes it.
- Export format "Tileable PNG": one PNG per tile at its size, optional repeat N×M.
- Fixtures: brick/stone wall images with known tile sizes; seam metric tests.

# Non-goals

- Rotated, perspective-distorted, or non-rectangular (hex, diamond) lattices; tiles from several layers.
- Texture synthesis larger than the source, normal/height maps, Wang tiles or autotile sets.
- Auto-tagging assets as "pattern"; pattern detection in videos.

# Acceptance examples

- A1: Extract ▸ Find Repeating Pattern… (enabled with an editable raster layer) opens a window that analyzes the selection's bounding box (or the whole active layer) and shows the detected period (W × H px) and a confidence; the proposed tile is outlined on the canvas; when the region holds fewer than two repeats per axis or no clear period, it says "No repeating pattern found" and proposes the whole region.
- A2: On the fixture images (brick, stone, and cobblestone walls built from CC0 textures tiled a known number of times, plus synthetic running-bond brick, also saved as JPEG quality 80 and with added noise σ=8), the detector returns exactly the known tile size, independent of where the analyzed region starts (the minimal tile: e.g. two courses for running bond).
- A3: Manual crop mode: the tile can be moved and resized by dragging on the canvas or typed as X/Y/W/H (clamped to the layer); the 3×3 tiled preview updates live (within one frame for tiles up to 512×512).
- A4: Make Seamless in the window (blend width 5–50 %, default 25 %) replaces the preview's tile with a seamless version of the same size (Revert restores it); Filter ▸ Make Seamless… applies the same operation to the active layer, or to the selection's bounding box, with live preview and one undo step.
- A5: Seam metric: for a tile, the mean absolute RGB difference across the wrap-around edges (last column vs first, last row vs first) divided by the mean difference between adjacent interior columns/rows; seamless means ≤ 1.15. Tests: on a fixed set of 16 crops of the four fixture textures (4 each, sizes not lined up with the texture's repeat) every raw crop scores > 1.15 (a measurable seam), and after Make Seamless every result is ≤ 1.15 at blend widths 5 %, 25 %, and 50 %; detected tiles of the periodic fixtures are ≤ 1.15 without any processing; a survey of 200 random crops per texture is reported (raw crops whose size happens to line up with a regular brick grid can already be nearly seamless).
- A6: Add to Library saves the current tile (seamless version if applied) as an asset in category "pattern" (manual), with origin method "pattern" and parameters (period, tile rectangle, seamless, blend width); "pattern" is a default category, added to existing libraries when missing; auto-tagging never proposes "pattern".
- A7: Export… has a "Tileable PNG" format: one `<name>.png` per selected asset at its exact size, or with Repeat N×M (1–16 each) the tile repeated N×M; a test checks the repeated image equals the tile at every offset and that export names never overwrite.
- A8: Detection on a 1024×1024 region takes under 500 ms and Make Seamless on a 512×512 tile under 200 ms in a release build.

# Constraints and invariants

- Analysis and previews never modify the document; only Filter ▸ Make Seamless changes pixels, as one undoable step.
- Transparent pixels are composited over neutral gray for detection; tiles keep their alpha.

# Decisions

- The tool is a window opened from Extract ▸ Find Repeating Pattern… (like Auto-Detect), with manual crop on the canvas.
- Make Seamless is available in the pattern window and as Filter ▸ Make Seamless… on the active layer or selection.
- Saved tiles get category "pattern" (manual); auto-tagging never proposes "pattern".
- Export adds a "Tileable PNG" format: tile at its size, optional repeat N×M.
- Conventions adopted without asking: fixtures from CC0 ambientCG textures (downscaled, with license note) tiled a known number of times plus synthetic brick; minimal tile = smallest axis-aligned translation period; seam metric as in A5.
- A5 reworded on 2026-10-20 after measurement (random grey-brick crops aligned with the brick grid are already nearly seamless): raw crops must show a measurable seam (> 1.15) on a fixed crop set instead of ≥ 1.5 on any crop.
- Workspace: branch `comet/pattern-extraction` off master.

# Open questions

- None. User confirmed outcome, scope, decisions, acceptance A1–A8, and non-goals on 2026-10-20.

# Verification expectations

- `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, existing release benchmarks, and a new pattern benchmark pass.
- Unit tests: period detection (fixtures, phase independence, JPEG/noise, no-pattern case), seam metric, Make Seamless, tileable export.
- Headless UI tests: window open/detect, outline, manual crop drag and typed values, 3×3 preview, Make Seamless/Revert, Add to Library, Filter ▸ Make Seamless with undo, export dialog.
