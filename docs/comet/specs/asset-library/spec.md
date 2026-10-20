# Asset Library

## Purpose

Keep every extracted asset and animation in a persistent, searchable, categorized library and export them for game engines.

## Requirements

### Storage

- Location: `<app data>/Game Asset Creator/library/` (`GAC_LIBRARY_DIR` overrides). `assets/<id>.png` per single asset; `assets/<id>_<nn>.png` per animation frame; `library.json`: `{ "version": 1, "categories": [...], "assets": [ { "id", "name", "kind": "single"|"animation", "files": [...], "width", "height", "category", "category_source": "auto"|"manual"|"none", "confidence", "tags": [...], "source": { "file", "frame", "rect": [x, y, w, h] }, "extraction": { "method", "params" }, "created" } ] }`.
- Every change (add, rename, category/tag edit, remove, category list edit) rewrites `library.json` via a temporary file and rename; image files are written before the metadata that references them. Removal deletes the asset's files after confirmation.
- At startup the library is loaded; entries whose files are missing or unreadable are skipped and listed in a notice; a damaged `library.json` is kept aside as `library.json.bad` and an empty library starts. Thumbnails are created lazily.
- Assets added by Auto-Detect, Extract Object, Add Selection to Library, and Review's Add Kept record their source file, frame (videos), source rectangle, and extraction method with its parameters.

### Panel

- Dockable panel "Asset Library": toolbar (search box, thumbnail size slider, Export…, Auto-Tag, Categories…), category chips (All plus each category with its count; multi-select), and a thumbnail grid (name, category, a filmstrip mark and frame count for animations) of the assets matching the search and chips, in order added.
- Search: case-insensitive substring on name, tags, category, source file name.
- Selection: click, Cmd-click toggle, Shift-click range. Actions on the selection: Rename (one), Category ▸ (sets manually), Tags… (add/remove free text), Remove… (asks first), Open as Document (one; animations open one layer per frame), Reveal in Finder, Export…, Auto-Tag.
- Categories…: add, rename, remove (assets in a removed category become "other"; "other" stays).

### Auto-tagging

- Model: OpenCLIP ViT-B/32 LAION-2B int8 ONNX (`model_int8.onnx`, `tokenizer.json`), SHA-256 pinned, stored in `<app data>/models/clip/`, downloaded after consent (same flow as MobileSAM), loaded lazily on a worker thread through the dynamically loaded ONNX Runtime.
- Image: composited over neutral gray, padded to a square, resized to 224 (bicubic), CLIP-normalized. Text: per category a set of prompt phrases ("pixel art of …"); custom categories use "pixel art of a <name>". A category's score is its best phrase's similarity; the best category and its softmax confidence are stored.
- New assets are tagged automatically when the model is available (`category_source: auto`); Auto-Tag re-tags the selection (manual ones included, since the user asked). Setting a category by hand marks it `manual`; automatic tagging never changes manual categories. Without the model, new assets get "other" (`none`) and the panel shows why.

### Export

- Export… on the selection (or all shown assets): Format: PNG files (a folder; `<name>.png`, unique names; animations as sequences) or Sprite sheet: Layout Grid (columns Auto = ⌈√n⌉ or a number; cell = largest width × height) or Max-Rects (best-short-side-fit, sizes sorted by area descending, growing the sheet as needed), Padding 0–16 px (default 2), Power of two (default off), Max size 4096 (error if it does not fit), file name. Writes `<name>.png` and `<name>.json` (TexturePacker JSON Hash: `frames` keyed by asset name with `frame {x, y, w, h}`, `rotated: false`, `trimmed: false`, `spriteSourceSize`, `sourceSize`; `animations` mapping animation names to their frame names in order; `meta {app, version, image, format: "RGBA8888", size {w, h}, scale: "1"}`).
- Animations alone can export as a horizontal strip (frames left to right, PNG plus JSON Hash with an `animations` entry) or as numbered PNGs (`<name>_01.png`, …). In sprite sheets an animation contributes frames `<name>_01`, … and an `animations` entry.

### Performance

- 1,000-asset library loads in under 2 s; tagging one asset under 1 s in a release build once the model is loaded.

### Fixture

- `tests/fixtures/tagging/`: CC0 sprites from Kenney packs (Tiny Dungeon, Roguelike RPG, 1-bit), at least 3 per default category, `labels.json` with category and split (`tune` or `test`), and `LICENSE.txt`. Accuracy is measured on the `test` split.
