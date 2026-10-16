# Outcome

Game Asset Creator has a runnable Rust desktop shell built on eframe/egui with egui_dock: a main window with dockable Tools, Layers, and Asset Library panels around a tabbed document area, backed by an in-memory document model (document → ordered layers → RGBA8 `image::RgbaImage` buffers, plus size/DPI metadata). Users can create blank documents, open PNG/JPG/BMP/WebP images into tabs, and export documents back to those formats, with PNG export being pixel-lossless. The project is a Cargo package with a library crate (model, I/O, app state) and a binary entry point, tested with `cargo test`.

# Scope

- Project skeleton: `Cargo.toml` (edition 2021, binary `game-asset-creator`; deps eframe/egui, egui_dock, image, rfd, png), `src/lib.rs` + `src/main.rs`, unit tests in modules and integration tests in `tests/`.
- Document model: `Document` (width, height, DPI, name, source path, ordered layer list) and `Layer` (name, visible, opacity, `RgbaImage` buffer); add/insert/remove/move layers; composite visible layers.
- Image I/O: load PNG/JPG/BMP/WebP into a single-layer Document; export a Document to PNG/JPG/BMP/WebP chosen by file extension.
- Main window: menu bar with File (New, Open, Export, Close, Quit) and View (toggle panels) menus, dockable Tools / Layers / Asset Library panels, tabbed document area with closable tabs, status bar.
- Document view: each document tab shows the composited pixels at 100% in a scroll area (the real canvas arrives in C2).

# Non-goals

- Zoom/pan canvas, checkerboard backdrop, undo/redo (C2).
- Selections, paint tools, layer editing UI, blend modes, native `.gac` format, and a File → Save command (C3–C5).
- Any functional asset library (placeholder panel only).
- Animated image formats (only the first frame of an animated file is read).
- Unsaved-changes prompts (nothing is editable yet).
- Python/PySide6 (superseded by the user's switch to Rust).

# Acceptance examples

- A1: `cargo build` builds the project from `Cargo.toml` (library + `game-asset-creator` binary), and `cargo test` runs and passes from the repository root.
- A2: `cargo run` (the `game-asset-creator` binary) opens the main window titled "Game Asset Creator".
- A3: The main window shows three dockable panels — Tools, Layers, Asset Library — that can be dragged/re-docked and closed, plus a View menu that shows/hides each one; documents appear as closable tabs in the document area.
- A4: File → Open… accepts PNG, JPG/JPEG, BMP, and WebP; opening a file adds a new document tab named after the file that shows the image pixels, and focuses it. An unreadable or unsupported file shows an error message and adds no tab.
- A5: File → New… asks for width, height, and DPI and opens a new tab containing a fully transparent single-layer document of that size; invalid (zero/non-numeric) values are rejected.
- A6: File → Export… writes the current document to PNG, JPG, BMP, or WebP by chosen extension, compositing visible layers; exporting an opened PNG to PNG reproduces identical RGBA pixel values (verified by automated test through the same app export path the menu uses).
- A7: JPG and BMP export composite transparent pixels over white; PNG and WebP keep alpha (WebP lossless). DPI is written to PNG (pHYs) and JPG (JFIF density).
- A8: Loading reads every supported format into a document with one RGBA8 layer of the image's size, records the source path and name, and reads DPI from PNG pHYs / JPG JFIF density (72 when absent or for BMP/WebP).
- A9: The Layers panel lists the current document's layers top-first with a visibility indicator and follows the focused document tab (empty state when no document); the Tools panel shows disabled placeholder tool buttons; the Asset Library panel shows placeholder text.
- A10: Unit tests cover the document model: construction and validation of size/DPI, layer buffer size invariants, add/insert/remove/move layer ordering, and compositing of visible layers with opacity.

# Constraints and invariants

- Every layer buffer has exactly the document's width and height; mismatched buffers are rejected with an error.
- Layer order: index 0 is the bottom layer; the Layers panel displays top-first.
- Width, height, and DPI must be positive.
- Model and I/O modules have no egui dependency; app state operations (open/new/export/close, focused document) are testable without a window.

# Decisions

- Language: Rust (user change; supersedes Python 3.11 + PySide6 from the original request).
- GUI: eframe/egui with egui_dock for dockable panels and the tabbed document area; rfd for native file dialogs.
- Pixel buffers: `image::RgbaImage` per layer; the image crate handles PNG/JPG/BMP/WebP decode and encode.
- Entry point: `cargo run` / the `game-asset-creator` binary replaces `python -m game_asset_creator`; `cargo test` replaces pytest.
- Workspace isolation: current directory on `master` (repository has no commits, so branch/worktree isolation is not available).
- Default DPI when the file has none: 72.
- File menu offers Export… (Ctrl+Shift+E) only; no Save command in C1 — "Save" is reserved for the native `.gac` format (C5).
- JPG/BMP export composites over white; PNG/WebP preserve alpha. JPG quality 95; WebP lossless.
- File → New… is in scope: width/height/DPI dialog creating a transparent document.
- Layers panel is live (lists the focused document's layers top-first with visibility); Tools panel shows disabled placeholder buttons; Asset Library is a placeholder.

# Open questions

- None. User confirmed outcome, scope, decisions, acceptance A1–A10, and non-goals on 2026-10-16.

# Verification expectations

- `cargo build` and `cargo test` pass; `cargo clippy` reports no errors.
- Tests cover the document model, image I/O round-trips for all four formats (PNG exact, JPG within tolerance, BMP exact RGB, WebP exact), DPI read/write, and app-state operations (open adds a focused tab, new creates a transparent document, export round-trips PNG exactly, layers listing follows focus, close removes the tab).
- A headless egui frame test renders the full UI (menus, docks, document tabs) via `egui::Context::run` without panicking.
- The binary launches a window (manual or short-lived launch check).
