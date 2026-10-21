# Editor ↔ Pipeline Workflow

## Purpose

Connect the asset library, the editor, and the extraction tools into one editing loop.

## Requirements

### Edit a Copy and Save Back

- Library action "Edit a Copy" (replacing Open as Document) opens the asset as a new document titled "<asset name> (library)", linked to the asset id; single assets open as one layer, animations as one layer per frame.
- File ▸ Save Back to Library is enabled only for linked documents. Single: the visible composite becomes the asset image; animation: each top-level layer (bottom to top) becomes a frame. The asset gets a new version; its size may change; the panel thumbnail refreshes at once; the document's modified mark clears. If the asset no longer exists, Save Back adds a new asset from the document and links the document to it.

### Versions

- Each asset has a current version (1 for existing libraries). Save Back, Clean Up, and Restore write the new image files (`assets/<id>.v<n>.png`, frames `assets/<id>.v<n>_<nn>.png`) and then the metadata (`version`, `versions: [{version, created, files, width, height}]`). Version 1 keeps its existing file names.
- Versions… (library, one asset) lists versions newest first with number, date, size, and thumbnail; Restore copies the chosen version as a new current version. Remove deletes all version files.

### Extraction from the canvas

- Remove Background, Auto-Detect Objects, and Extract Object have "Source: Canvas | Active layer" (default Canvas). Canvas = the visible composite. With a selection, the tool works on the selection's bounding box and treats pixels outside the selection mask as transparent; results never extend beyond the selection.
- Remove Background with Canvas adds a new layer "Background removed" (keyed composite) above the active layer; with Active layer it edits the layer as before. Each is one undo step.
- Extraction parameters recorded with assets include `source: "canvas" | "layer"` and `region: [x, y, w, h]` when a selection was used.

### Drag and drop

- Library thumbnails are drag sources; a document canvas accepts the drop. The asset (first frame for animations) becomes a new layer named after the asset, above the active layer, centred on the drop point in image coordinates and clipped to the canvas; one undo step. Drops elsewhere are ignored.

### Clean Up

- Despeckle: alpha < 8 → 0; then 8-connected components of non-transparent pixels smaller than max(4 px, 0.1 % of all non-transparent pixels) become transparent. Crop: to the bounding box of the remaining non-transparent pixels (animations: the union over frames, applied to every frame).
- Library: Clean Up on the selected assets saves a new version for each changed asset; unchanged assets get no version.
- Image ▸ Clean Up: despeckle the active layer, then crop the canvas (all layers, masks, selection) to the visible content; one undo step (undo restores size and pixels).

### Full-loop fixture

- `tests/fixtures/video/skeleton.mp4`: a CC0 Kenney skeleton sprite (scaled up) moving across a plain background (H.264), with a license note.
