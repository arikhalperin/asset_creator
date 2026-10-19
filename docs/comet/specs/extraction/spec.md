# Asset Extraction

## Purpose

Turn parts of an image — sprites on a flat background, or objects in a photo — into tightly cropped transparent assets collected in the Asset Library.

## Requirements

### Remove Background

- Extract → Remove Background… opens a dialog with live canvas preview (like filters): Background color (Auto = the most frequent color among the active layer's border pixels, quantized to 5 bits per channel and refined to the mean of matching border pixels; or Pick, then click the canvas), Tolerance 0–100 % (default 10), Edge Softness 0–100 % (default 10), Contiguous (default on), Preview (default on), Reset, OK, Cancel.
- Distance is the largest per-channel difference from the background color, normalized to 0–1. Pixels with distance ≤ tolerance become background (alpha 0); pixels with distance ≥ tolerance + softness are unchanged; in between, alpha = (d − tolerance) / softness and the color is decontaminated: `c = (p − (1 − α)·bg) / α`, clamped. Existing alpha is multiplied. With Contiguous, only background-colored regions connected (4-neighbour) to the image border are keyed; enclosed regions of the same color are kept.
- OK applies to the active layer's pixels through the selection as one undo step "Remove Background"; Cancel restores exactly. Disabled without a document or during a transform.

### Auto-Detect Objects

- Extract → Auto-Detect Objects… analyses the active layer: if it has transparent pixels, objects are its regions with alpha > 0; otherwise it keys the background automatically (Auto color, default tolerance/softness, Contiguous) without changing the layer.
- Objects are 8-connected regions; regions whose bounding boxes are within Merge Distance (0–64 px, default 2) of each other are merged repeatedly; regions with fewer than Minimum Size (1–10000 px², default 16) non-transparent pixels are dropped. Each proposal's box is the tight bounds of its pixels. Proposals are ordered top-to-bottom, then left-to-right.
- The dialog lists the proposals (number, size) with checkboxes (all checked), Select All / None, Minimum Size, Merge Distance, Add to Library, Cancel; the canvas shows numbered boxes (checked ones highlighted). Add to Library adds the checked proposals as assets and closes the dialog.

### Extract Object tool

- A tool in the Tools panel (shortcut O). On the active layer: a click places an include point and replaces the current prompt; a drag places a box; Shift-click adds an include point; Option/Alt-click adds an exclude point. Each prompt change recomputes the mask in the background (the newest prompt wins); the mask is drawn as a tinted overlay with an outline, include/exclude points as markers.
- Tool options: engine status (`MobileSAM`, `GrabCut — <reason>`, `Downloading MobileSAM… n %`, `Analyzing image…`), Add to Library (Enter), To Selection (sets the document selection to the mask as an undoable selection change), Clear (Esc), Download Model… when MobileSAM is not installed.
- MobileSAM: the active layer's RGB is resized so its longer side is 1024 (bilinear) and encoded once per layer content (cached until the layer changes); prompts use the single-mask decoder with points scaled to the resized image (box corners labels 2/3; a padding point with label −1 when there is no box); the mask is the decoder output > 0 at full resolution, keeping the region connected to an include point.
- GrabCut fallback: 5-component RGB Gaussian mixtures for object and background, 8-neighbour contrast-sensitive smoothness, min-cut by max-flow, 5 iterations, on the image downscaled to at most 512 px (mask upsampled and refined at full resolution along its border). Include/exclude points pin disks of radius 1 % of the image diagonal as object/background. Box: outside is background, inside unknown. Click/points without a box: candidate boxes are tried smallest first and the first whose object does not touch the box edges (at most 2 % of its edge pixels) is used — first the box of a region grown from the include points across small colour steps (padded 25 %; the region is the initial object model and only pixels outside the box start the background model; a region that leaks over half the image or to its border is retried with smaller steps), then squares of 30 %, 50 %, 75 %, and 100 % of the image's short side centred on the points. The result keeps the region connected to an include point and fills enclosed holes smaller than 0.5 % of it.

### Models and degradation

- Nothing ML-related loads until the first Extract Object prompt. ONNX Runtime is loaded dynamically from `ORT_DYLIB_PATH`, `/opt/homebrew/lib`, `/usr/local/lib`, or the system library path; models are read from `<app data>/models/`.
- If the runtime is missing, GrabCut is used with the reason shown. If the runtime exists but the models do not, the first prompt asks: "Download MobileSAM (about 45 MB)?" — Download / Use GrabCut. Downloading runs in the background with progress; the files are written to temporary names, SHA-256 verified, then renamed. While downloading, after declining (for the session), or after a failure (shown in the status), GrabCut is used. Once ready, the next prompt uses MobileSAM.

### Assets and the Asset Library

- An asset is made from the active layer's pixels and a mask (coverage 0–255): alpha is multiplied by coverage, then the image is cropped to the bounding box of pixels with alpha > 0 (empty results are not added). Sources: Auto-Detect proposals (mask = the proposal's region), Extract Object (the mask), Extract → Add Selection to Library (the selection).
- Asset names are `<document stem> <n>`, n counting per document from 1.
- The Asset Library panel lists assets in order (thumbnail, name, width × height), selectable (click, Cmd/Shift extend). Buttons: Save PNG… (one selected asset; RGBA PNG), Save All… (choose a folder; files `<name>.png`, with ` (2)`, ` (3)`… for duplicates), Open as Document (new image document named after the asset, one "Background" layer), Remove. The library is kept for the session only.
- The `AssetLibrary` API exposes each asset's id, name, RGBA image, source document name, and source rectangle.

### Performance (release build)

- Remove Background preview on 1920×1080 < 150 ms; Auto-Detect on 2048×2048 < 250 ms; GrabCut mask on 1920×1080 < 2 s; MobileSAM first mask < 2 s, later masks on the same image < 200 ms.
