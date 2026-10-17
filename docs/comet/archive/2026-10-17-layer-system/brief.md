# Outcome

Game Asset Creator gets a complete layer system: Layers-panel operations (new, duplicate, delete, reorder, merge down, flatten, rename), per-layer visibility, opacity, and seven blend modes, layer masks editable with the paint tools, interactive transforms (Move tool, Free Transform with handles, flips), and a native `.gac` project format with Save / Save As / Open and unsaved-changes tracking.

# Scope

- Document model: layer blend mode, mask + enabled flag; W3C blend-mode compositing.
- Layer commands and Layers panel UI; Layer menu with shortcuts.
- Mask commands and mask edit target for all paint tools.
- Transform engine (affine resampling, nearest/bilinear), Move tool, Free Transform session with handles, Flip; selection-aware.
- `.gac` read/write (ZIP + JSON manifest + PNGs), File → Save/Save As, Open for `.gac`, dirty tracking and save prompts.

# Non-goals

- Layer groups, adjustment layers, clipping masks, layer styles/effects, linked/unlinked masks (masks always move with their layer), smart objects.
- Perspective/skew/warp transforms, numeric transform entry fields.
- Layer thumbnails and drag-and-drop reordering in the panel (reordering uses buttons and shortcuts).
- Content beyond the canvas (pixels moved off-canvas are clipped on commit).
- Storing undo history in `.gac`; autosave.

# Acceptance examples

- A1: The Layers panel and Layer menu offer New (Cmd+Shift+N), Duplicate (Cmd+J), Delete (disabled with one layer), Move Up (Cmd+]), Move Down (Cmd+[), Merge Down (Cmd+E, disabled on the bottom layer), and Flatten; each is one undo step with the active layer updated as specified.
- A2: Each Layers row toggles visibility with its eye control and renames on double-click; the active layer's blend mode and opacity (0–100 %) are edited above the rows, opacity previewing live during a drag and recorded as one step when it ends; every property change is undoable.
- A3: The seven blend modes (Normal, Multiply, Screen, Overlay, Add, Darken, Lighten) composite exactly per the W3C Compositing and Blending formulas including alpha: tests compare the compositor with an independent f64 reference over a grid of colors and alphas (within ±1 per channel), and a single Normal layer still composites to its exact pixels.
- A4: Merge Down composites the active layer (with its mode, opacity, and mask) into the layer below, which keeps its name and properties; Flatten produces one "Background" layer equal to the composite with transparency kept. Verified by tests against the composite.
- A5: Masks: Add Layer Mask (reveal all), Add Mask from Selection, Delete Mask, Apply Mask, and Disable/Enable Mask work as specified; an enabled mask multiplies the layer's alpha in the composite and a disabled one is ignored; all undoable.
- A6: Clicking a layer's Mask chip makes the mask the edit target (highlighted; status bar "Editing mask"); Brush, Paint Bucket, and Gradient then write gray (luminance) values into the mask, the Eraser reveals (white), and the Clone Stamp copies mask values, all respecting the selection and undoable, while the layer's pixels stay unchanged.
- A7: Move tool (V): dragging moves the active layer's content (or the selected content, with the selection moving along) live; releasing records one "Move" step; arrow keys nudge 1 px (Shift: 10 px); pixels pushed past the canvas edge are clipped on commit.
- A8: Free Transform (Cmd+T) shows a bounding box with eight handles around the content; dragging inside moves, handles scale (Shift on a corner keeps proportions), dragging just outside a corner rotates (Shift snaps 15°); Interpolation (Bilinear default, Nearest), Commit, and Cancel appear in the tool-options strip; Enter commits one "Free Transform" step, Escape or switching tool/document cancels and restores; the layer mask and selection transform with the content.
- A9: The transform preview is interactive: a release-mode benchmark of Free Transform updates on a 1024×1024 layer averages under 16 ms per update, and each update re-renders only the old ∪ new transformed bounds. Resampling tests: identity and whole-pixel translation are exact, Nearest 90° rotations and 2× scaling are exact, Bilinear produces intermediate values.
- A10: Flip Horizontal / Flip Vertical mirror the content (selection bounds or whole canvas) as one step; flipping twice restores the original exactly.
- A11: Move, Free Transform, and Flip respect the selection: only the selected content (mask-weighted) moves and the vacated area keeps only the unselected remainder; without a selection the whole layer moves.
- A12: `.gac` files are ZIP archives with `manifest.json` (format "game-asset-creator", version 1, size, DPI, active layer, bottom-to-top layers with name, visibility, opacity, blend mode, image path, mask path, mask enabled; selection path) plus `layers/N.png`, `masks/N.png`, and `selection.png`, verified by a test that inspects a saved archive.
- A13: File → Save (Cmd+S) and Save As… (Cmd+Shift+S) write `.gac` atomically and update the document's name and path; Save on a document without a `.gac` file behaves as Save As; File → Open accepts `.gac`; loading reproduces a multi-layer document exactly (pixels, names, visibility, opacity, blend modes, masks and enabled state, DPI, size, selection, active layer); corrupt, foreign, wrong-version, incomplete, or mismatched files show an error and open nothing.
- A14: Unsaved changes: a tab title ends with " •" while its document differs from the last saved/opened state (undoing back to it clears the marker); closing such a document asks Save / Don't Save / Cancel; quitting (menu, Cmd+Q, window close button) asks for each such document in turn and Cancel aborts quitting.
- A15: Compositing everywhere (canvas, export, eyedropper "All layers") honours visibility, opacity, blend modes, and enabled masks; paint tools continue to target the active layer (or its mask).
- A16: Every new operation undoes and redoes exactly (pixels, masks, properties, layer order, active layer, selection), and the canvas refreshes the affected tiles after each.

# Constraints and invariants

- Layers and masks stay canvas-sized.
- Compositing, transform resampling, merge/flatten, and `.gac` encode/decode are pure, window-independent functions with unit tests.
- Pixel-editing operations respect the selection (C3 rule).

# Decisions

- Transform resampling: Bilinear by default, with Nearest as an option in the tool-options strip.
- Off-canvas pixels: clipped when a move/transform is committed (lossless while the transform is still open).
- Unsaved changes: " •" marker and Save / Don't Save / Cancel prompts on close and quit.
- Conventions adopted without asking: W3C blend formulas; Photoshop-style shortcuts (Cmd+Shift+N, Cmd+J, Cmd+E, Cmd+[ / ], Cmd+T, V); masks move with their layer; merged layer keeps the lower layer's properties; Flatten keeps transparency; mask painting uses luminance with the Eraser revealing.
- Delivered as one change: blend modes, masks, transforms, and the file format all extend the same layer model and Layers panel, so splitting would mostly add merge coordination.
- Workspace: branch `comet/layer-system` off master.

# Open questions

- None. User confirmed outcome, scope, decisions, acceptance A1–A16, and non-goals on 2026-10-17.

# Verification expectations

- `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, the C2 pan/zoom benchmark, and a new release transform benchmark pass.
- Unit tests: blend modes vs f64 reference, masks in compositing, layer commands and undo exactness, merge/flatten, resampling exactness, transform sessions with selection, flips, `.gac` round trip and malformed-file rejection, dirty-state tracking.
- Headless UI tests: Layers panel/menu operations and shortcuts, opacity drag as one step, mask chip edit target with brush/eraser, Move tool drag and nudges, Free Transform handle drags with Enter/Escape, Save/Open round trip through the app, dirty marker and close/quit prompts.
- Manual launch to check the panel, handles, and Save/Open.
