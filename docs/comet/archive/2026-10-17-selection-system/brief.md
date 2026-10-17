# Outcome

Game Asset Creator gains a selection system: five selection tools (rectangle, ellipse, freehand lasso, polygon lasso, magic wand), an 8-bit selection mask stored on the document, animated marching ants on the canvas, replace/add/subtract/intersect combine modes, Select All/None/Invert, Grow/Shrink/Feather, and destructive edits that respect the selection. Selection changes are undoable steps.

# Scope

- Selection mask on `Document` (optional 8-bit `width × height` mask; none = everything).
- Pure, unit-tested mask functions: rectangle, ellipse, polygon (lasso + polygon lasso), magic wand, combine, all/invert, grow, shrink, feather, boundary outline.
- Tools panel: five real selection tools with shortcuts (M, L, W) replacing the "Select" placeholder; tool-options strip with combine-mode buttons, Anti-alias, Tolerance, Contiguous.
- Canvas: tool gestures (drag, clicks), live gesture preview, crosshair cursor, animated marching ants.
- Select menu: All, None, Invert, Grow…, Shrink…, Feather… (radius dialogs).
- Undo: selection commands; masked-apply helper that all pixel-changing commands use; Fill Layer respects the selection.

# Non-goals

- Moving/transforming the selection outline or its contents, Quick Mask, saving/loading selections, Select → Color Range, Border/Smooth operations.
- Shift-to-constrain (square/circle) and draw-from-center modifiers (Shift is used for Add).
- Magic wand "sample all layers".
- Paint tools that respect the selection (C4 builds on the masked-apply helper).

# Acceptance examples

- A1: The Tools panel offers Rectangle Select, Ellipse Select, Lasso, Polygon Lasso, and Magic Wand; the active tool is highlighted (Rectangle Select at startup); M toggles Rectangle/Ellipse, L toggles Lasso/Polygon Lasso, W picks Magic Wand; these single-key shortcuts do nothing while a text field is focused or Cmd/Ctrl is held.
- A2: Rectangle Select: dragging selects exactly the pixels whose centers lie inside the dragged rectangle (clipped to the document, hard edges); a click without dragging in Replace mode clears the selection. Verified by unit tests on synthetic images and a canvas drag test.
- A3: Ellipse Select selects the inscribed ellipse; with Anti-alias on, edge pixels hold coverage-proportional values (4×4 supersampling) and interior pixels 255; with it off, values are only 0/255 by pixel-center test. Verified by unit tests.
- A4: Lasso: dragging a closed freehand path selects its interior by the even-odd rule, with the same Anti-alias behaviour; fewer than 3 distinct points selects nothing. Verified by unit tests (e.g. a triangle and a self-intersecting path).
- A5: Polygon Lasso: clicks add vertices; clicking within 6 points of the first vertex, double-clicking, or pressing Enter closes and applies it (≥3 vertices); Backspace removes the last vertex; Escape cancels; edges and a segment to the pointer are previewed. Verified by unit and canvas input tests.
- A6: Magic Wand selects pixels of the active layer within Tolerance (max RGBA channel difference, default 32) of the clicked color; Contiguous (default on) restricts to the 4-connected region; edges are hard. Verified by unit tests on synthetic images (including a same-colour region separated by a barrier, with Contiguous on and off).
- A7: The selection is stored on the document as an 8-bit mask of the document's size; masks of the wrong size are rejected; an all-zero result clears the selection; with no selection, edits apply to the whole document.
- A8: Combine modes: Replace = b, Add = max(a, b), Subtract = round(a·(255−b)/255), Intersect = min(a, b); Shift / Option(Alt) / Shift+Option held at gesture start select Add / Subtract / Intersect, otherwise the tool-options mode (Replace by default) applies. Verified by unit tests and a canvas test with modifiers.
- A9: Select → All (Cmd/Ctrl+A), None (Cmd/Ctrl+D), Invert (Cmd/Ctrl+Shift+I) behave as specified (Invert with no selection selects everything).
- A10: Select → Grow… / Shrink… (radius 1–100 px) dilate / erode the mask with a Euclidean disk; Shrink does not erode from the canvas edge. Verified by unit tests.
- A11: Select → Feather… (radius 1–100 px, default 5) produces a soft edge: across a hard straight edge the values ramp monotonically from 255 to 0, with intermediate values present, 255 more than 2r inside and 0 more than 2r outside. Verified by unit tests.
- A12: Marching ants: a selection's boundary (mask ≥ 128 vs < 128 or outside the document) is drawn as alternating black/white 4-point screen-space dashes that move over time, and the canvas keeps repainting while a selection exists. Verified by unit tests of the outline and a canvas test showing the drawn dash pattern changes between frames at different times.
- A13: Every selection change is an undoable, redoable step labelled after the operation (e.g. "Undo Rectangle Select", "Undo Feather Selection"); selection-only changes re-upload no canvas tiles.
- A14: Fill Layer respects the selection: fully selected pixels become the fill color, unselected pixels are unchanged, partially selected pixels are interpolated in proportion to the mask; undo restores exact pixels; only the selection's bounding box is marked dirty. Pixel-changing commands use one shared masked-apply helper.
- A15: The tool-options strip shows Replace/Add/Subtract/Intersect buttons for every selection tool, an Anti-alias checkbox (default on) for Ellipse, Lasso, and Polygon Lasso, and Tolerance (0–255, default 32) plus Contiguous (default on) for Magic Wand.
- A16: With a selection tool active, Space+drag and middle-drag still pan, wheel/pinch still zoom, and no selection is created by those gestures; the pointer is a crosshair over the image.

# Constraints and invariants

- Mask functions are pure, window-independent, and deterministic.
- Selection commands never change layer pixels; pixel commands never change the selection.
- Undo/redo of any selection command restores the exact previous mask (or no mask).

# Decisions

- Combine-mode modifiers follow Photoshop: Shift = Add, Option/Alt = Subtract, Shift+Option/Alt = Intersect; plus mode buttons in the tool-options strip.
- Selection changes are undoable steps on the C2 undo stack.
- Anti-alias is on by default for Ellipse, Lasso, and Polygon Lasso, with a checkbox; Rectangle and Magic Wand are always hard-edged.
- Magic Wand samples the active layer with Tolerance (default 32) and Contiguous (default on).
- Implementation choices recorded in the specs: even-odd fill, max-channel color distance, 4-connectivity, disk-shaped grow/shrink, Gaussian feather with σ = r/2, ants threshold 128.
- Workspace: branch `comet/selection-system` off master.

# Open questions

- None. User confirmed outcome, scope, decisions, acceptance A1–A16, and non-goals on 2026-10-17.

# Verification expectations

- `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, and the C2 release pan/zoom benchmark still pass.
- Unit tests for every mask function on synthetic images, combine modes, grow/shrink/feather, outline extraction, masked fill, and selection command undo/redo.
- Headless canvas tests: rectangle drag (with and without modifiers), polygon lasso clicks + Enter/Escape/Backspace, wand click, Select menu shortcuts, marching-ants animation between frames, panning with a selection tool active.
- Manual launch to see marching ants animate and tools work.
