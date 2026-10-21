# Outcome

The editor and the extraction pipeline form one loop: a library asset can be edited as a copy and saved back as a new version (history kept, thumbnail refreshed); the C8 extraction tools work on whatever is visible on the canvas, limited to the active selection; library assets can be dragged onto a canvas as new layers; and one click cleans an asset up (stray alpha specks removed, transparent borders cropped). The full loop — open a video, grab a frame, extract a skeleton, edit it, save it back, drag it into a new composition — works end to end.

# Scope

- Library "Edit a Copy" (replaces Open as Document): opens the asset as a document linked to it (single: one layer; animation: one layer per frame). File ▸ Save Back to Library writes the document into the asset as a new version (single: the visible composite; animation: each top-level layer is a frame, in order); thumbnail refreshes; if the asset was removed meanwhile, Save Back adds it as a new asset.
- Version history: every Save Back, Clean Up, and Restore adds a version; older versions stay on disk; library "Versions…" lists them (version, date, size, thumbnail) and restores one (as a new version). Removing an asset deletes all its versions.
- C8 tools on the canvas: Remove Background, Auto-Detect Objects, and Extract Object read the visible composite by default (Source: Canvas / Active layer switch in each), limited to the selection when there is one (outside the selection counts as transparent; detection and segmentation run on the selection's bounds). With Source Canvas, Remove Background adds the keyed result as a new layer; with Active layer it edits the layer as today.
- Drag and drop: dragging a library thumbnail onto a document canvas adds a new layer named after the asset, centred on the drop point, clipped to the canvas, one undo step; animations add their first frame.
- Clean Up: library action on selected assets (new version) and Image ▸ Clean Up on a document (active layer despeckled, then the canvas cropped to the visible content; one undo step). Despeckle: pixels with alpha below 8 become transparent, then 8-connected non-transparent specks smaller than 4 px (or 0.1 % of the content, whichever is larger) are removed; then crop to the remaining content. Animations: the same crop for all frames.
- Fixture video with a CC0 skeleton sprite and an end-to-end UI test of the full loop.

# Non-goals

- Multi-document compositions linked to several assets; merging edits from two copies of the same asset.
- Version diffing, branching, or pruning; version history for categories/tags/names.
- Dropping files from Finder (only library assets); dropping into the layers panel.
- Changing extraction algorithms (C8/C9 engines are reused as they are).

# Acceptance examples

- A1: Library "Edit a Copy" opens the selected asset as a document titled "<asset name> (library)" (animations: one layer per frame); File ▸ Save Back to Library (enabled only for such documents) stores the document into that asset as version n+1 (single: the visible composite; animation: one frame per top-level layer in order), the panel thumbnail and Versions… show the new image immediately, and the document is no longer marked modified; if the asset was removed, Save Back adds a new asset instead.
- A2: Versions are kept on disk and survive restarts; Versions… lists every version (number, date, size, thumbnail) and Restore makes the chosen version current as a new version; removing the asset deletes all its version files; library.json from C10/C11 loads unchanged (assets start at version 1).
- A3: Remove Background, Auto-Detect Objects, and Extract Object use the visible canvas composite by default and only the active selection's area when there is one (pixels outside the selection are ignored/transparent, results never extend beyond it); each tool has a Source: Canvas / Active layer switch; with Canvas, Remove Background adds a new layer "Background removed"; assets record source "canvas" or "layer" and the selection bounds in their extraction parameters.
- A4: Dragging a library thumbnail onto a document canvas adds a new layer named after the asset above the active layer, centred on the drop point (clipped to the canvas), as one undo step; an animation adds its first frame; dropping anywhere else does nothing.
- A5: Clean Up on selected library assets removes stray alpha (alpha < 8 cleared; 8-connected specks below max(4 px, 0.1 % of content) removed) and crops to the remaining content (animations: one crop for all frames), saving a new version; Image ▸ Clean Up does the same to the active layer and crops the canvas to the visible content as one undo step; already clean assets are left unchanged (no new version).
- A6: Full loop (headless UI test on a fixture video of a CC0 skeleton sprite): open the video, grab a frame into a document, extract the skeleton with Extract Object (or Auto-Detect) into the library, Edit a Copy, paint on it, Save Back (version 2, thumbnail updated), create a new document, drag the asset onto it — the new layer holds the edited skeleton pixel-for-pixel at the drop position.

# Constraints and invariants

- Library writes stay atomic; version files are written before the metadata that references them.
- Extraction on the canvas never modifies the document except Remove Background's new layer (Canvas) or layer edit (Active layer), each one undo step.

# Decisions

- C8 tools read the visible canvas by default, limited to the selection, with a Source: Canvas / Active layer switch.
- Save Back keeps version history with Versions… (view and restore).
- Dropping an animation adds its first frame as one layer.
- Clean Up is available in the library (new version) and as Image ▸ Clean Up on documents.
- Conventions adopted without asking: Edit a Copy replaces Open as Document; Remove Background with Source Canvas adds a new layer; despeckle thresholds as in Scope; the fixture video is generated from a Kenney CC0 skeleton sprite; versions stored as assets/<id>.v<n>.png (frames <id>.v<n>_<nn>.png).
- Workspace: branch `comet/editor-pipeline` off master.

# Open questions

- None. User confirmed outcome, scope, decisions, acceptance A1–A6, and non-goals on 2026-10-21.

# Verification expectations

- `cargo test`, clippy `-D warnings`, `fmt --check`, existing release benchmarks.
- Unit tests: versions (store, restore, remove, migration), despeckle and crop, canvas-source extraction with selection clipping, layer drop placement.
- Headless UI tests: Edit a Copy / Save Back / thumbnail refresh, Versions… restore, Source switch in each C8 tool, drag-and-drop from the library panel onto a canvas, Clean Up in library and Image menu with undo, and the full loop of A6.
