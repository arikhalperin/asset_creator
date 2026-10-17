# Editor shell

## Purpose

The Game Asset Creator desktop application window: eframe/egui with egui_dock providing dockable panels and document tabs.

## Requirements

### Project and entry point

- Cargo package `game-asset-creator` with a library crate and a binary `game-asset-creator`; `cargo run` launches the app, `cargo test` runs all tests.
- File paths passed as command-line arguments are opened as documents at startup.
- The window title is "Game Asset Creator".

### Layout

- Menu bar with File, Edit, Select, Layer, and View menus; a tool-options strip below the menu bar; status bar at the bottom showing the focused document's name, size, DPI, zoom percentage, and the pointer's pixel coordinate when over the image (or "No document").
- A dock area (egui_dock) containing three panel tabs — Tools, Layers, Asset Library — and the document tabs. Panels and documents can be dragged, re-docked, and closed.

### File menu

- New… (Ctrl+N): dialog with width, height, DPI fields (defaults 512, 512, 72); OK with valid values (1–16384 px sides, positive DPI) opens a new transparent single-layer document tab named "Untitled-N" and focuses it; invalid values show an inline error and do not create a document.
- Open… (Ctrl+O): native file dialog filtered to PNG/JPG/JPEG/BMP/WebP; a loaded file opens in a new focused tab named after the file; failure shows an error message window and adds no tab.
- Export… (Ctrl+Shift+E): enabled only with a focused document; native save dialog for PNG/JPG/BMP/WebP; writes via image I/O export; failure shows an error message.
- Save (Ctrl+S): writes the current document to its `.gac` file; a document without one (new, or opened from PNG/JPG/BMP/WebP) asks for a location as Save As does.
- Save As… (Ctrl+Shift+S): native save dialog for `.gac`; afterwards the document's name and path are the saved file's.
- Open… also accepts `.gac` project files.
- Close (Ctrl+W): closes the focused document tab, asking Save / Don't Save / Cancel first when it has unsaved changes.
- Quit (Ctrl+Q, also the window close button): asks Save / Don't Save / Cancel for each document with unsaved changes in turn; Cancel aborts quitting.

### Unsaved changes

- A document tab title ends with " •" while the document has unsaved changes (see the undo spec's saved state).

### Edit menu

- Undo (Ctrl+Z) and Redo (Ctrl+Shift+Z, also Ctrl+Y), labelled with the command name (e.g. "Undo Fill Layer"); disabled when unavailable.
- Fill Layer…: enabled with a current document; opens a dialog with an RGBA color picker defaulting to the last used color (opaque black initially); OK fills the current document's active layer via the Fill Layer command; Cancel does nothing.
- Ctrl means Cmd on macOS for all shortcuts.

### View menu

- Zoom In (Ctrl+=), Zoom Out (Ctrl+-), Fit to Window (Ctrl+0), Actual Size (Ctrl+1), and a Show Pixel Grid toggle (default on), acting on the current document's canvas.
- A checkable entry per panel; checking reopens the panel, unchecking closes it.

### Document tabs

- Each document tab shows the document on the editing canvas (see the canvas spec).
- Closing a document tab removes the document and its undo history.
- The most recently focused document tab is the "current document" for Export, Close, Edit and View actions, the Layers panel, and the status bar.

### Panels

- Tools: Rectangle Select, Ellipse Select, Lasso, Polygon Lasso, Magic Wand, Brush, Eraser, Eyedropper, Paint Bucket, Gradient, and Clone Stamp buttons (the active tool is highlighted; Rectangle Select is active at startup), the Move tool (V), and the foreground/background color swatches. Shortcuts: M toggles Rectangle/Ellipse Select, L toggles Lasso/Polygon Lasso, W Magic Wand, B Brush, E Eraser, I Eyedropper, G toggles Gradient/Paint Bucket, S Clone Stamp, X swaps foreground and background, D resets them to black and white, [ and ] decrease and increase the current brush size; single-key shortcuts are ignored while a text field is focused or Cmd/Ctrl is held.
- Layers: lists the current document's layers top-first, each with a visibility indicator and name; the active layer is highlighted and clicking a row makes it active; shows "No document" when none is focused.
- Asset Library: placeholder text indicating the library arrives later.

### Testability

- App state operations (new, open, export current, close, execute/undo/redo, active layer, current document, layer listing) are callable without a window.
- The UI renders headlessly through `egui::Context::run_ui` without panicking.

### Select menu

- All (Ctrl+A), None (Ctrl+D), Invert (Ctrl+Shift+I). All and Invert are enabled with a current document; None needs an existing selection.
- Grow…, Shrink…, Feather…: each opens a dialog with a radius in pixels (whole number 1–100; defaults 2, 2, 5), enabled when the current document has a selection; OK applies the operation as one undoable step, Cancel does nothing, invalid input shows an inline error.

### Tool-options strip

- For selection tools, shows the combine-mode buttons Replace / Add / Subtract / Intersect (one active; Replace by default).
- Ellipse Select, Lasso, and Polygon Lasso also show an Anti-alias checkbox (default on, shared by those tools).
- Magic Wand also shows Tolerance (0–255, default 32) and Contiguous (default on).

- Brush and Eraser show Size, Hardness, Opacity, and Pixel; Clone Stamp shows Size, Hardness, Opacity, and Aligned; Paint Bucket shows Tolerance, Contiguous, and Opacity; Gradient shows Type (Linear/Radial), Colors (Foreground to Background / Foreground to Transparent), and Opacity; Eyedropper shows Sample (All layers / Active layer). Each tool keeps its own option values (see the paint spec for ranges and defaults).

### Color swatches

- The Tools panel shows the foreground and background colors (default opaque black and opaque white). Clicking a swatch opens an RGBA color picker that edits that color; a swap button and X exchange them; a reset button and D restore black/white.

### Layer menu

- New Layer (Ctrl+Shift+N), Duplicate Layer (Ctrl+J), Delete Layer, Move Layer Up (Ctrl+]), Move Layer Down (Ctrl+[), Merge Down (Ctrl+E), Flatten Image; Add Layer Mask, Add Mask from Selection, Delete Mask, Apply Mask, Disable/Enable Mask; Edit Layer Pixels / Edit Layer Mask. Items are disabled when they cannot apply (e.g. Delete with one layer, Merge Down on the bottom layer, mask items without a mask, Add Mask from Selection without a selection).

### Edit menu additions

- Free Transform (Ctrl+T), Flip Horizontal, Flip Vertical, acting on the active layer (or the selected content).
