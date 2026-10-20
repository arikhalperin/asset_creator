# Outcome

The Asset Library becomes a persistent, searchable collection: every extracted asset and animation is saved to disk with its metadata, survives restarts, is auto-tagged into editable categories by OpenCLIP (downloaded on first use) and always manually re-taggable, and can be exported as PNGs, packed sprite sheets with a TexturePacker JSON atlas, animation strips, or numbered PNG sequences.

# Scope

- On-disk library in the app's data folder: a PNG per asset (per frame for animations) and `library.json` metadata (source file, frame, source rectangle, extraction method and parameters, category, tags, created time); written on every change, loaded at startup.
- Library panel: thumbnail grid, search, category filter chips, selection, rename, category and tag editing, remove, open as document, reveal in Finder, export.
- Editable categories (defaults chalice, skeleton, potion, deathtrap, wall, prop, other).
- Zero-shot auto-tagging with OpenCLIP ViT-B/32 (LAION, int8) through the dynamically loaded ONNX Runtime; download on consent; background; manual corrections are kept.
- Export: single assets, selected sets, sprite sheets (grid and max-rects packing) with a TexturePacker JSON Hash atlas; animations as strips (with atlas) or numbered PNG sequences.
- Labeled tagging fixture (CC0 Kenney sprites), atlas tests, persistence tests.

# Non-goals

- Several libraries, cloud sync, sharing; library undo/redo.
- Pattern/tile assets (C11), dedup across the library, image editing inside the library.
- Sprite rotation, trimming, or extrusion in packing; atlas formats other than TexturePacker JSON Hash; multi-page atlases.
- Other tagging models, fine-tuning, or text search by image similarity.

# Acceptance examples

- A1: The library lives in the app's data folder (`library/` with `assets/*.png` and `library.json`); every add, edit, and removal is written immediately (atomically); at startup it is loaded, so assets, animations, names, categories, tags, and metadata (source file, frame, source rectangle, extraction method and parameters, created time) survive an app restart pixel-identically; missing or damaged files are skipped with a notice, never crashing or losing the rest.
- A2: The dockable Asset Library panel shows a thumbnail grid (adjustable size) with name and category, a search box (case-insensitive match on name, tags, category, source file), category filter chips with counts (multi-select plus All), click/Cmd/Shift selection, and actions: Rename, Category (menu), Tags (edit), Remove (asks first), Open as Document, Reveal in Finder, Export…, Auto-Tag.
- A3: Categories are editable in the panel: add, rename, and remove (assets of a removed category become "other"; "other" cannot be removed); category changes are persisted.
- A4: Auto-tagging uses OpenCLIP ViT-B/32 (LAION-2B, MIT, int8, about 155 MB) via ONNX Runtime, loaded lazily; the first auto-tag asks before downloading (SHA-256 checked, background with progress); when available, each new asset is tagged in the background with its best category and confidence; Auto-Tag re-tags the selection; a category set by hand is marked manual and never overwritten by automatic tagging; without the model, new assets get "other" and the panel says why.
- A5: On the labeled fixture set (CC0 Kenney sprites, at least 3 per default category, `labels.json`), auto-tagging is at least 70 % correct (top-1) on the held-out half (prompt phrases tuned only on the other half), and every tag can be corrected manually (UI test).
- A6: Export… writes single assets or the selection as PNGs (unique names), or a sprite sheet: Layout Grid (columns automatic or fixed, cells of the largest size) or Max-Rects (best-short-side-fit), Padding (default 2 px), Power-of-two size (optional), Max size 4096 (error if it does not fit), as `<name>.png` plus a TexturePacker JSON Hash `<name>.json`; a test verifies every atlas frame rectangle is in bounds, non-overlapping (with padding), and pixel-identical to its asset.
- A7: Animations export as a horizontal strip (PNG plus JSON Hash with its frames and an `animations` entry listing them in order) or as numbered PNGs (`name_01.png`, …); in a sprite sheet an animation contributes its frames and an `animations` entry.
- A8: A library of 1,000 assets loads in under 2 s and the panel stays responsive (thumbnails created lazily); tagging one asset takes under 1 s in a release build once the model is loaded.

# Constraints and invariants

- Library writes never block the UI on tagging or downloads; file writes are atomic (temporary file + rename).
- Without network, ONNX Runtime, or the model, everything except auto-tagging works.

# Decisions

- Auto-tagging model: OpenCLIP ViT-B/32 LAION-2B int8 ONNX (Hugging Face `onnx-community/CLIP-ViT-B-32-laion2B-s34B-b79K-ONNX`), downloaded on first use after consent.
- One library in the app's data folder (`GAC_LIBRARY_DIR` overrides, for tests).
- One category per asset plus free-text tags.
- Atlas format: TexturePacker JSON Hash.
- Conventions adopted without asking: JSON metadata (not SQLite); C8–C9 session assets become persistent; auto-tag prompt ensembles per default category (custom categories use "pixel art of a <name>"); Remove deletes the files after confirmation; labeled fixture from CC0 Kenney packs with a license note.
- Workspace: branch `comet/asset-library` off master.

# Open questions

- None. User confirmed outcome, scope, decisions, acceptance A1–A8, and non-goals on 2026-10-20.

# Verification expectations

- `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, existing release benchmarks, and new library/tagging benchmarks pass.
- Unit tests: metadata round trip, atomic writes, damaged-file handling, category edits, search/filter, grid and max-rects packing, atlas JSON, strips and sequences.
- Fixture tests: tagging accuracy on the held-out half (when ONNX Runtime and the model are available), atlas coordinates vs assets, persistence across a simulated restart.
- Headless UI tests: panel grid, search, filters, selection, rename/category/tags/remove, categories editing, consent/fallback, export dialog.
