# Editor shell

## Purpose

The Game Asset Creator desktop application window: eframe/egui with egui_dock providing dockable panels and document tabs.

## Requirements

### Project and entry point

- Cargo package `game-asset-creator` with a library crate and a binary `game-asset-creator`; `cargo run` launches the app, `cargo test` runs all tests.
- The window title is "Game Asset Creator".

### Layout

- Menu bar with File and View menus; status bar at the bottom showing the focused document's size and DPI (or "No document").
- A dock area (egui_dock) containing three panel tabs — Tools, Layers, Asset Library — and the document tabs. Panels and documents can be dragged, re-docked, and closed.
- View menu has a checkable entry per panel; checking reopens the panel, unchecking closes it.

### File menu

- New… (Ctrl+N): dialog with width, height, DPI fields (defaults 512, 512, 72); OK with valid positive values opens a new transparent single-layer document tab named "Untitled-N" and focuses it; invalid values show an inline error and do not create a document.
- Open… (Ctrl+O): native file dialog filtered to PNG/JPG/JPEG/BMP/WebP; a loaded file opens in a new focused tab named after the file; failure shows an error message window and adds no tab.
- Export… (Ctrl+Shift+E): enabled only with a focused document; native save dialog for PNG/JPG/BMP/WebP; writes via image I/O export; failure shows an error message.
- Close (Ctrl+W): closes the focused document tab.
- Quit (Ctrl+Q): closes the application.

### Document tabs

- Each document tab shows the document composite at 100% scale inside a scroll area; the texture refreshes when the document changes.
- Closing a document tab removes the document.
- The most recently focused document tab is the "current document" for Export, Close, the Layers panel, and the status bar.

### Panels

- Tools: disabled placeholder buttons for upcoming tools (Move, Select, Brush, Eraser, Fill, Picker).
- Layers: lists the current document's layers top-first, each with a visibility indicator and name; shows "No document" when none is focused.
- Asset Library: placeholder text indicating the library arrives later.

### Testability

- App state operations (new, open, export current, close, current document, layer listing) are callable without a window.
- The UI renders headlessly through `egui::Context::run` without panicking.
