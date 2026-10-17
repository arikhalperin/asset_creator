# Editor shell

## Purpose

The Game Asset Creator desktop application window: eframe/egui with egui_dock providing dockable panels and document tabs.

## Requirements

### Project and entry point

- Cargo package `game-asset-creator` with a library crate and a binary `game-asset-creator`; `cargo run` launches the app, `cargo test` runs all tests.
- File paths passed as command-line arguments are opened as documents at startup.
- The window title is "Game Asset Creator".

### Layout

- Menu bar with File, Edit, Select, and View menus; a tool-options strip below the menu bar; status bar at the bottom showing the focused document's name, size, DPI, zoom percentage, and the pointer's pixel coordinate when over the image (or "No document").
- A dock area (egui_dock) containing three panel tabs — Tools, Layers, Asset Library — and the document tabs. Panels and documents can be dragged, re-docked, and closed.

### File menu

- New… (Ctrl+N): dialog with width, height, DPI fields (defaults 512, 512, 72); OK with valid values (1–16384 px sides, positive DPI) opens a new transparent single-layer document tab named "Untitled-N" and focuses it; invalid values show an inline error and do not create a document.
- Open… (Ctrl+O): native file dialog filtered to PNG/JPG/JPEG/BMP/WebP; a loaded file opens in a new focused tab named after the file; failure shows an error message window and adds no tab.
- Export… (Ctrl+Shift+E): enabled only with a focused document; native save dialog for PNG/JPG/BMP/WebP; writes via image I/O export; failure shows an error message.
- Close (Ctrl+W): closes the focused document tab.
- Quit (Ctrl+Q): closes the application.

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

- Tools: Rectangle Select, Ellipse Select, Lasso, Polygon Lasso, and Magic Wand buttons (the active tool is highlighted; Rectangle Select is active at startup), followed by disabled placeholders for upcoming tools (Move, Brush, Eraser, Fill, Picker). Shortcuts: M toggles Rectangle/Ellipse Select, L toggles Lasso/Polygon Lasso, W selects Magic Wand; single-key tool shortcuts are ignored while a text field is focused or Cmd/Ctrl is held.
- Layers: lists the current document's layers top-first, each with a visibility indicator and name; the active layer is highlighted and clicking a row makes it active; shows "No document" when none is focused.
- Asset Library: placeholder text indicating the library arrives later.

### Testability

- App state operations (new, open, export current, close, execute/undo/redo, active layer, current document, layer listing) are callable without a window.
- The UI renders headlessly through `egui::Context::run_ui` without panicking.

### Select menu

- All (Ctrl+A), None (Ctrl+D), Invert (Ctrl+Shift+I). All and Invert are enabled with a current document; None needs an existing selection.
- Grow…, Shrink…, Feather…: each opens a dialog with a radius in pixels (whole number 1–100; defaults 2, 2, 5), enabled when the current document has a selection; OK applies the operation as one undoable step, Cancel does nothing, invalid input shows an inline error.

### Tool-options strip

- Always shows the combine-mode buttons Replace / Add / Subtract / Intersect (one active; Replace by default).
- Ellipse Select, Lasso, and Polygon Lasso also show an Anti-alias checkbox (default on, shared by those tools).
- Magic Wand also shows Tolerance (0–255, default 32) and Contiguous (default on).
