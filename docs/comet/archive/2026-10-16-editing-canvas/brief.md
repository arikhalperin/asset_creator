# Outcome

Game Asset Creator documents are shown on a real editing canvas instead of a fixed 100% image view: the canvas composites the document's layers into cached tiles, zooms smoothly around the cursor (Ctrl/Cmd+wheel, pinch, fit and 100% shortcuts), pans with space-drag, draws a checkerboard behind transparency and a pixel grid above 800% zoom, and re-uploads only the tiles touched by an edit. Every document gets a command-pattern undo/redo stack that is the only public way to edit a document, demonstrated by an undoable Edit → Fill Layer… command targeting a selectable active layer.

# Scope

- Canvas widget (egui custom painting; the Rust equivalent of the plan's QGraphicsView) per document tab: view transform (zoom + pan), tiled composite cache with dirty-region invalidation, checkerboard backdrop, pixel grid, sampling rules.
- Navigation: Ctrl/Cmd+wheel and trackpad pinch zoom anchored at the cursor; plain wheel / two-finger scroll pans; space+left-drag and middle-drag pan; View menu Zoom In / Zoom Out / Fit to Window / Actual Size and Show Pixel Grid; shortcuts Cmd/Ctrl+= , Cmd/Ctrl+-, Cmd/Ctrl+0, Cmd/Ctrl+1.
- Undo framework (Rust equivalent of the plan's QUndoStack): `Command` trait, per-document `UndoStack` with a 1 GB memory budget, `AppState::execute` as the sole public mutation path, Edit menu Undo/Redo with shortcuts.
- Sample command: Edit → Fill Layer… with an RGBA color picker dialog (remembers last color) filling the active layer as one undoable step.
- Active layer: per-document, defaults to the top layer, selectable by clicking a row in the Layers panel (highlighted).
- Status bar: adds zoom percentage and the cursor's pixel coordinate when over the image.

# Non-goals

- Painting, selection, and other editing tools (C3–C4); layer add/delete/reorder UI, blend modes, transforms (C5).
- Animated zoom transitions; rotation of the view.
- Dirty/unsaved-document indicators and save prompts (no Save command exists yet).
- Persisting undo history or view state across app restarts.
- Qt/PySide6 (superseded by the Rust/egui stack chosen in C1).

# Acceptance examples

- A1: Opening a 4000×4000 image shows it on the canvas; continuous pan (space-drag, scroll) and zoom (Ctrl/Cmd+wheel, pinch) frames do no recompositing and no texture uploads, and a release-build headless benchmark of pan/zoom frames on a 4000×4000 document averages under 8 ms of CPU time per frame. The running app pans and zooms the image without visible stutter.
- A2: Ctrl/Cmd+wheel and pinch zoom continuously (not in fixed steps) keeping the image point under the cursor fixed on screen; zoom is clamped to 1%–6400%.
- A3: Cmd/Ctrl+0 (View → Fit to Window) fits the whole image in the view, centered; Cmd/Ctrl+1 (View → Actual Size) shows 100% (one image pixel per physical screen pixel); Cmd/Ctrl+= and Cmd/Ctrl+- zoom in/out by a factor of √2 around the view center. A newly opened document starts at Fit if larger than the view, otherwise at 100%, centered.
- A4: Holding Space and left-dragging pans the view (grab cursor shown); middle-drag and plain wheel/two-finger scroll also pan; Space does not type into or trigger other widgets while panning.
- A5: Transparent and semi-transparent image areas are drawn over a screen-space checkerboard (8-point light/dark gray squares) limited to the image bounds; the area outside the image is the plain canvas background.
- A6: Above 800% zoom (strictly greater than 8×) a 1-physical-pixel grid is drawn on pixel boundaries over the visible image; at or below 8× no grid is drawn; View → Show Pixel Grid (default on) toggles it.
- A7: Image sampling is nearest-neighbor when magnified (crisp pixels) and smooth (linear with mipmaps) when minified.
- A8: The composite is cached in tiles; an edit that changes a rectangle recomposites and re-uploads only tiles intersecting that rectangle (verified by tile-upload counters in tests), and undo/redo of it does the same.
- A9: Every document has an undo stack; documents can only be modified through `AppState::execute(Command)` (no public mutable document access); executing pushes the command and clears redo history; undo/redo restore exact pixel data; a command exposes a label used in the Edit menu ("Undo Fill Layer").
- A10: Edit menu has Undo (Cmd/Ctrl+Z), Redo (Cmd/Ctrl+Shift+Z and Ctrl+Y), and Fill Layer…; Undo/Redo are disabled when nothing to undo/redo, Fill Layer… when there is no current document.
- A11: Edit → Fill Layer… opens a dialog with an RGBA color picker defaulting to the last used color (opaque black initially); OK fills the entire active layer with that color as one undoable step and the canvas updates; Cancel changes nothing.
- A12: The Layers panel highlights the active layer; clicking a row makes it active; a new or opened document's active layer is its top layer.
- A13: Each document's undo history keeps unlimited steps but drops the oldest steps once their recorded memory exceeds 1 GB; the most recent step is always kept.
- A14: The status bar shows the current zoom percentage, and the cursor's image pixel coordinate while the pointer is over the image.

# Constraints and invariants

- Undo/redo of any command returns the document to bit-identical layer pixels and metadata.
- `Document` mutation from outside the crate goes only through `AppState::execute`; read access stays public.
- Pan/zoom never recomposites or re-uploads textures; only dirty tiles are refreshed after edits.
- Canvas math (view transform, fit, zoom clamping, anchor-preserving zoom, tile/dirty intersection) is window-independent and unit-tested.

# Decisions

- Stack: the plan's QGraphicsView and QUndoStack are implemented as an egui canvas widget and a Rust command-pattern undo stack (Rust/eframe/egui chosen in C1).
- Workspace: branch `comet/editing-canvas` off master; C1 code and project setup were committed to master first.
- Fill color: dialog with RGBA color picker remembering the last color (opaque black initially).
- Active layer: selectable in the Layers panel, defaulting to the top layer.
- Undo history: unlimited steps with a 1 GB per-document memory budget, oldest dropped first.
- Zoom range 1%–6400%; zoom steps ×√2; pixel grid above 8×; nearest sampling when magnified, linear+mipmaps when minified; checkerboard in screen space.

# Open questions

- None. User confirmed outcome, scope, decisions, acceptance A1–A14, and non-goals on 2026-10-16.

# Verification expectations

- `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` pass.
- Unit tests for view math, tile dirty tracking, undo stack semantics and memory budget, fill command exactness.
- Headless egui tests: canvas frames with simulated scroll/zoom/space-drag input change the view without tile uploads; Edit menu actions undo/redo the fill; Layers panel click selects the active layer.
- A release-mode benchmark test (ignored by default, run explicitly) measuring headless pan/zoom frame time on a 4000×4000 document.
- Manual launch with a 4000×4000 image to confirm smooth pan/zoom, checkerboard, and pixel grid.
