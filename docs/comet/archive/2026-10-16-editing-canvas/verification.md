---
generated_from_state_version: 12
---

# Verification

## Current result

- Result: **Archived**
- Verification status: **Checks completed; result confirmed**
- Goal cycle: 1
- Iteration: 2
- Verifier attempt: 1
- Completed: 2026-10-16T19:38:00.265Z
- Summary: All 14 acceptance items pass. The A7 repair works on the real wgpu path: a CPU-built mip pyramid with per-level linear tiles, with GPU minification always under 2×, while level 0 stays nearest when magnified. egui-wgpu 0.36.2 honors the min/mag filters. The other items still hold on code review and on the tests (canvas suite re-run locally: 17 passed).

## Acceptance

| ID | Result | Source | Criterion | Reason |
| --- | --- | --- | --- | --- |
| A1 | passed | brief.md | A1: Opening a 4000×4000 image shows it on the canvas; continuous pan (space-drag, scroll) and zoom (Ctrl/Cmd+wheel, pinch) frames do no recompositing and no texture uploads, and a release-build headless benchmark of pan/zoom frames on a 4000×4000 document averages under 8 ms of CPU time per frame. The running app pans and zooms the image without visible stutter. | TileCache uploads every tile of every mip level once on first refresh; later refresh() calls do nothing unless pending/dirty, so pan/zoom (incl. switching mip level) never recomposites or uploads. Integration tests check pinch, scroll and space-drag with unchanged upload counters; the release benchmark (Runtime check) asserts <8 ms/frame with tessellation and zero uploads. Live stutter-free feel was not driven by hand (see risks). |
| A2 | passed | brief.md | A2: Ctrl/Cmd+wheel and pinch zoom continuously (not in fixed steps) keeping the image point under the cursor fixed on screen; zoom is clamped to 1%–6400%. | Canvas uses i.zoom_delta() (Cmd/Ctrl+wheel and pinch) multiplicatively via zoom_around anchored at the pointer; clamp_zoom 0.01..64. Tests cover anchor invariance, a non-step small wheel delta, and the clamp at both ends. |
| A3 | passed | brief.md | A3: Cmd/Ctrl+0 (View → Fit to Window) fits the whole image in the view, centered; Cmd/Ctrl+1 (View → Actual Size) shows 100% (one image pixel per physical screen pixel); Cmd/Ctrl+= and Cmd/Ctrl+- zoom in/out by a factor of √2 around the view center. A newly opened document starts at Fit if larger than the view, otherwise at 100%, centered. | ViewState::fit/actual_size/centered/initial match the spec, with zoom in physical pixels (ppp-aware). Cmd+=/−/0/1 shortcuts and View menu items queue ViewActions; ZoomIn/Out use ×√2 at the view center. egui keyboard UI-zoom is disabled. Unit tests and the view_shortcuts and small/large-initial tests pass. |
| A4 | passed | brief.md | A4: Holding Space and left-dragging pans the view (grab cursor shown); middle-drag and plain wheel/two-finger scroll also pan; Space does not type into or trigger other widgets while panning. | Space+primary drag and middle drag pan; Grab cursor while Space is held over the canvas and Grabbing while dragging; plain smooth_scroll_delta pans. Space over the canvas takes focus so other widgets don't get it, and is ignored while a text edit is focused. Tests: space_drag_pans, plain_drag_without_space_does_not_pan, middle_drag_pans, scroll_pans_without_reupload. |
| A5 | passed | brief.md | A5: Transparent and semi-transparent image areas are drawn over a screen-space checkerboard (8-point light/dark gray squares) limited to the image bounds; the area outside the image is the plain canvas background. | A 2×2 Repeat/Nearest checker texture is drawn over the visible image rect only, with UVs = screen position / 16 points, so squares are 8 points in screen space and don't scale with zoom. Light 0xD6 / dark 0xA8 gray. Outside the image is extreme_bg_color. Tiles are drawn on top. |
| A6 | passed | brief.md | A6: Above 800% zoom (strictly greater than 8×) a 1-physical-pixel grid is drawn on pixel boundaries over the visible image; at or below 8× no grid is drawn; View → Show Pixel Grid (default on) toggles it. | The grid draws when show_grid && zoom > 8.0 (strict), with 1/ppp-point (1 physical px) lines on each pixel boundary within the visible pixel rect. Show Pixel Grid checkbox in the View menu, default true. The test checks no grid at exactly 8×, a grid above it, and that the toggle turns it off. |
| A7 | passed | brief.md | A7: Image sampling is nearest-neighbor when magnified (crisp pixels) and smooth (linear with mipmaps) when minified. | Level 0 tiles use mag Nearest and min Linear. At zoom >= 1 the canvas draws level 0, so magnified pixels are crisp. When minified, mip_level_for_zoom picks the CPU-built 2× box-filtered (premultiplied) pyramid level at or above display resolution, drawn with LINEAR, so GPU minification within a level is always <2×. I checked egui-wgpu 0.36.2 create_sampler: it honors min/mag filters (mip_level_count 1), so linear filtering plus explicit mip levels holds on the actual wgpu path. This is equivalent to LINEAR_MIPMAP_NEAREST. Tests cover level selection, sizes, premultiplied downsample, partial refresh == rebuild, and a Fit view drawing ≤20 meshes from level 2. |
| A8 | passed | brief.md | A8: The composite is cached in tiles; an edit that changes a rectangle recomposites and re-uploads only tiles intersecting that rectangle (verified by tile-upload counters in tests), and undo/redo of it does the same. | mark_dirty marks only intersecting tiles at each level; refresh composites only the pending rects via composite_region and re-uploads only dirty tiles. Tests: a single-pixel edit/undo uploads exactly 1 level-0 tile each, a tile-corner pixel uploads 1, Fill/undo on 300×300 uploads 4 each, and the pyramid partial refresh uploads exactly 1 tile per level and equals a full rebuild. |
| A9 | passed | brief.md | A9: Every document has an undo stack; documents can only be modified through `AppState::execute(Command)` (no public mutable document access); executing pushes the command and clears redo history; undo/redo restore exact pixel data; a command exposes a label used in the Edit menu ("Undo Fill Layer"). | Each OpenDocument owns an UndoStack. AppState exposes only &OpenDocument/&Document; document_mut is private, and execute/undo/redo are the only mutation paths. execute drains redo. Tests show FillLayer apply/undo/redo is bit-exact. The label 'Fill Layer' drives the 'Undo Fill Layer'/'Redo …' menu text. |
| A10 | passed | brief.md | A10: Edit menu has Undo (Cmd/Ctrl+Z), Redo (Cmd/Ctrl+Shift+Z and Ctrl+Y), and Fill Layer…; Undo/Redo are disabled when nothing to undo/redo, Fill Layer… when there is no current document. | Edit menu has Undo (Cmd+Z) and Redo (Cmd+Shift+Z) enabled only via can_undo/can_redo, plus Fill Layer… enabled only with a current document. Cmd+Y (Ctrl+Y off macOS) is also bound to Redo. Shortcut ordering consumes Shift+Z before Z. Tests exercise Cmd+Z, Cmd+Shift+Z and Cmd+Y. |
| A11 | passed | brief.md | A11: Edit → Fill Layer… opens a dialog with an RGBA color picker defaulting to the last used color (opaque black initially); OK fills the entire active layer with that color as one undoable step and the canvas updates; Cancel changes nothing. | The Fill Layer window uses color_edit_button_srgba_unmultiplied (RGBA), seeded with last_fill_color (default [0,0,0,255]). OK runs one FillLayer on the active layer via execute and remembers the color; Cancel just closes. The canvas refreshes from the dirty rects on the next frame. The test covers default color, fill, upload counts, remembered color and Cancel leaving history unchanged. |
| A12 | passed | brief.md | A12: The Layers panel highlights the active layer; clicking a row makes it active; a new or opened document's active layer is its top layer. | The Layers panel uses selectable_label(row.active), and a click sets the active layer for the current document. OpenDocument::new sets active_layer = top, and it is kept in range after changes. Unit tests cover the default, setting it and row flags; the actual row click has no automated test (see risks). |
| A13 | passed | brief.md | A13: Each document's undo history keeps unlimited steps but drops the oldest steps once their recorded memory exceeds 1 GB; the most recent step is always kept. | UndoStack counts costs recorded at push for both undo and redo, and pops the oldest undo entries while memory > 1 GiB and more than one step remains. FillLayer's cost is the saved layer size. Unit tests cover dropping the oldest, keeping a single over-budget step, and redo accounting. |
| A14 | passed | brief.md | A14: The status bar shows the current zoom percentage, and the cursor's image pixel coordinate while the pointer is over the image. | status_text appends the zoom % and 'x, y' from canvas.hover_pixel, which is set only when the pointer is inside the image bounds. The test checks 100% and that a 1-point move shifts by 1 pixel at ppp 1. |

## Checks

| Check | Command | Working directory | Status | Exit | Duration |
| --- | --- | --- | --- | ---: | ---: |
| cargo test | test | . | passed | 0 | 7008 ms |
| release pan/zoom benchmark | test --release --test canvas -- --ignored --nocapture | . | passed | 0 | 2582 ms |
| cargo clippy | clippy --all-targets -- -D warnings | . | passed | 0 | 444 ms |
| cargo fmt check | fmt --check | . | passed | 0 | 121 ms |

### Builder-reported evidence

These are Builder reports, not Runtime check receipts or independent verification results.

- cargo test: passed — 88 tests: 46 unit, 13 app, 17 canvas, 12 io; 1 ignored benchmark
- cargo test --release --test canvas -- --ignored: passed — 0.069 ms per pan/zoom frame, zero uploads
- cargo clippy --all-targets -- -D warnings: passed — —
- cargo fmt --check: passed — —
- manual launch (aliasing check): passed — Release app with a 4000x4000 image of alternating 1px black/white columns at Fit (18%): renders as uniform mid-gray with no moire, confirming mipmapped minification in the real wgpu renderer.
- Known limitation: Mip selection is per-level (no blending between levels); GPU minification within a level is linear and always < 2x.
- Known limitation: The pyramid keeps CPU copies of every level (~1.33x the composite size; ~85 MB for 4000x4000).
- Known limitation: Live mouse/trackpad pan/zoom smoothness not driven by hand; evidence is headless input tests and the release CPU benchmark.
- Known limitation: Layers panel row click not covered by an automated click test.
- Known limitation: Ctrl means Cmd on macOS (egui COMMAND modifier).

## Blockers

_None._

## Risks and skipped work

- Mip selection snaps per level with no blending between levels (nearest-mip), so a slight sharpness jump is possible when zoom crosses a power of two.
- Each tile is its own ClampToEdge texture, so linear filtering at tile and level edges does not blend across tiles; faint seams are possible at some minified zooms.
- The minified_view_draws_from_mip_pyramid test only bounds the mesh count from above and would also pass if nothing were drawn.
- Live trackpad/mouse smoothness, the minified on-screen look (Builder screenshot only) and the Layers row click were not exercised by hand; these rely on headless tests and code reading.
- The CPU pyramid keeps about 1.33× the composite in memory (~85 MB for 4000×4000), and opening a document uploads every level up front.
- A focused button drawn before the canvas in the same frame could still react to Space; low likelihood.

## Previous iterations

| Goal cycle | Iteration | Attempt | Outcome | Unresolved | Summary | Completed |
| ---: | ---: | ---: | --- | --- | --- | --- |
| 1 | 1 | 1 | fail | A7 | 13 of 14 acceptance items pass on code and automated evidence. A7 fails: minified sampling is linear without mipmaps because eframe uses the wgpu renderer, which ignores mipmap_mode (mip_level_count 1, no mipmap filter). Switch to the glow renderer or supply mipmaps explicitly, then re-verify. | 2026-10-16T19:31:34.503Z |
| 1 | 2 | 1 | pass | — | All 14 acceptance items pass. The A7 repair works on the real wgpu path: a CPU-built mip pyramid with per-level linear tiles, with GPU minification always under 2×, while level 0 stays nearest when magnified. egui-wgpu 0.36.2 honors the min/mag filters. The other items still hold on code review and on the tests (canvas suite re-run locally: 17 passed). | 2026-10-16T19:38:00.265Z |



## Conclusion

All 14 acceptance items pass. The A7 repair works on the real wgpu path: a CPU-built mip pyramid with per-level linear tiles, with GPU minification always under 2×, while level 0 stays nearest when magnified. egui-wgpu 0.36.2 honors the min/mag filters. The other items still hold on code review and on the tests (canvas suite re-run locally: 17 passed).
