# Outcome

Game Asset Creator gains an asset-extraction engine: Remove Background keys flat or near-flat sprite backgrounds with soft, color-decontaminated edges; Auto-Detect Objects proposes one tight crop per object in a keyed image; the Extract Object tool segments an object from a click or box with MobileSAM (downloaded on first use, loaded lazily) or, without it, a built-in GrabCut; every accepted extraction becomes a tightly cropped transparent asset in a session Asset Library panel that can save PNGs.

# Scope

- Extract menu: Remove Background…, Auto-Detect Objects…, Add Selection to Library.
- Remove Background dialog with live preview (auto dominant color or picked color, Tolerance, Edge Softness, Contiguous), applied to the active layer as one undo step.
- Auto-Detect Objects dialog: proposals as numbered boxes on the canvas and a checklist (Minimum Size, Merge Distance), Add to Library.
- Extract Object tool: click, drag a box, Shift-click include / Option-click exclude points; mask overlay; Add to Library, To Selection, Esc.
- Segmentation engines: MobileSAM through ONNX Runtime (loaded dynamically, models downloaded after consent, SHA-256 checked) and a built-in GrabCut fallback; background workers.
- Asset Library panel (stub until C10): list, Save PNG…, Save All…, Open as Document, Remove; `AssetLibrary` API.
- Fixtures (generated sprite sheets and synthetic photos with ground truth), tests, and release benchmarks.

# Non-goals

- Asset tagging, persistence across sessions, sprite-sheet/animation export (C10), dedup/animation grouping (C9), tileable patterns (C11).
- Batch extraction straight from the Frame Set (frames are opened as documents first).
- OpenCV; GPU/CoreML acceleration; other models (rembg/U²-Net, SAM variants); text-prompted segmentation.
- Editing an asset inside the library (it can be opened as a document).

# Acceptance examples

- A1: Extract → Remove Background… opens a preview dialog: Background color (auto: the most common border color; or picked by clicking the canvas), Tolerance (default 10 %), Edge Softness (default 10 %), Contiguous (default on), Preview; OK records one undo step "Remove Background" on the active layer (through the selection), Cancel restores exactly. On the flat and near-flat fixture sheets every background pixel becomes fully transparent, sprite interiors keep their exact pixels, and anti-aliased edge pixels become partially transparent with the background color removed from their color.
- A2: Extract → Auto-Detect Objects… finds the objects in the active layer (keying the background automatically, without changing the layer, when it has no transparency) and shows them as numbered boxes on the canvas and a checklist with Minimum Size (default 16 px²) and Merge Distance (default 2 px); on every fixture sprite sheet (flat, near-flat, transparent) it proposes exactly one box per sprite with the ground-truth bounds (exact; ±1 px on the near-flat sheet), keeping detached parts within the merge distance together; Add to Library adds the checked proposals.
- A3: The Extract Object tool (Tools panel, shortcut O) segments the object under a click, inside a dragged box, or refined by Shift-click (include) / Option-click (exclude) points on the active layer; the mask shows as a tinted overlay; Add to Library (Enter), To Selection, and Clear (Esc) are in the tool options; segmentation runs in the background (the UI stays responsive) and the tool options show which engine produced the mask.
- A4: Click-to-extract on each fixture photo returns a clean mask — IoU ≥ 0.90 with the ground truth, one connected region, no holes or islands larger than 0.5 % of the object — with MobileSAM and with the GrabCut fallback.
- A5: Models load lazily and degrade gracefully: nothing ML-related loads before the first Extract Object; without ONNX Runtime or the models the tool uses GrabCut and says why; the first use asks before downloading the MobileSAM models (~45 MB) into the app's data folder in the background with progress; files are SHA-256 checked; declining, a failed download, or a corrupt file leaves GrabCut in use (no partial file is used) and later extractions switch to MobileSAM once the models are ready.
- A6: Every asset — from Auto-Detect, Extract Object, or Add Selection to Library — is a tight transparent crop: its size is the bounding box of its non-transparent pixels, pixels outside the extraction are fully transparent (partial mask coverage scales alpha), and its saved PNG is RGBA and reloads identical.
- A7: The Asset Library panel lists assets (thumbnail, name `<document> <n>`, size) in the order added; Save PNG… saves the selected asset, Save All… writes every asset into a chosen folder with unique file names, Open as Document opens it as a new image document, Remove deletes selected assets; the library lasts for the session and is available through the `AssetLibrary` API (name, image, source document, source rectangle).
- A8: Release benchmarks: Remove Background preview on 1920×1080 < 150 ms; Auto-Detect on a 2048×2048 sheet < 250 ms; GrabCut mask on 1920×1080 < 2 s; MobileSAM first mask on an image < 2 s and later masks on the same image < 200 ms.

# Constraints and invariants

- The UI thread never waits on segmentation, model loading, or downloads.
- Extraction never changes the document except Remove Background (one undo step) and To Selection (an undoable selection change).
- Without network access, ONNX Runtime, or models, every feature except MobileSAM works unchanged.

# Decisions

- MobileSAM via ONNX Runtime loaded dynamically (Homebrew `onnxruntime` on this machine), models (MIT, Hugging Face `Acly/MobileSAM`: `mobile_sam_image_encoder.onnx`, `sam_mask_decoder_single.onnx`) downloaded on first use after consent; GrabCut implemented in Rust as the fallback (no OpenCV).
- Remove Background edits the active layer through a preview dialog, one undo step.
- Video frames are processed by opening them as documents (Frame Set → Open).
- Asset Library stub: session list with Save PNG…, Save All…, Open as Document, Remove.
- Conventions adopted without asking: Extract menu after Filter; Extract Object tool shortcut O; extraction reads the active layer's pixels; Contiguous keying from the image border by default; asset names `<document stem> <n>`; synthetic fixtures with ground truth (no third-party photos).
- Workspace: branch `comet/asset-extraction` off master.

# Open questions

- None. User confirmed outcome, scope, decisions, acceptance A1–A8, and non-goals on 2026-10-18.

# Verification expectations

- `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, existing release benchmarks, and the new extraction benchmarks pass.
- Unit tests: dominant color, keying and decontamination, connected components and merging, tight crop, GrabCut on simple images, SAM pre/post-processing, download verification.
- Fixture tests: sprite-sheet bounds vs ground truth; photo masks vs ground truth with both engines (MobileSAM tests run when the models are installed).
- Headless UI tests: menus, dialogs, overlays, tool interactions, library panel actions, consent/fallback flows.
