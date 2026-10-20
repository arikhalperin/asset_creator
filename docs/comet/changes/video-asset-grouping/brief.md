# Outcome

A video's frames can be mined for assets in one run: Extract Assets… on a video tab keys and auto-detects objects in every frame of a range in the background, deduplicates repeated appearances by perceptual hash, groups the same object's different poses into an ordered animation, and opens a Review tab where clusters are kept, discarded, or merged before being added to the Asset Library as single assets or animation entries.

# Scope

- Extract Assets… dialog on the video tab (frame range, every N frames, detection settings), background run with progress and Cancel.
- Per-frame extraction with C8's automatic keying and Auto-Detect.
- Perceptual-hash dedup of appearances; tracking and appearance linking into object clusters; ordered animation poses.
- Review tab per run: single-object rows and animation rows (filmstrip), Keep/Discard, Merge, Add Kept to Library.
- Asset Library animation entries (filmstrip, frame count; Save as numbered PNGs; Open as Document with one layer per frame; API access).
- A generated fixture clip (looping 8-pose skeleton walk + static potion) and tests; release benchmark.

# Non-goals

- Objects on detailed or scrolling backgrounds (motion-based detection), MobileSAM per frame.
- Running from the Frame Set.
- Sprite-sheet strip export, tagging, persistence (C10); editing animation timing or reordering poses.
- Splitting a cluster or moving single poses between clusters.

# Acceptance examples

- A1: A video tab's Extract Assets… opens a dialog — From/To frame (default the whole clip), Every N frames (default 1), Minimum Size (16 px²), Merge Distance (2 px); Extract runs in the background with a progress bar and Cancel (Cancel discards everything) and, when done, opens a Review tab titled `<video> review`; the UI stays responsive throughout.
- A2: Each processed frame is keyed with the automatic background color and default tolerance/softness (contiguous), keyed specks of fewer than 5 pixels (compression noise) are removed, and the frame is split into objects with Auto-Detect; every detection is a tight transparent crop recorded with its frame number and position.
- A3: Repeated appearances are deduplicated, robustly under video compression for sprites whose main shape is at least 32 px on its short side (tested with H.264 at crf 18–28, thin limbs, fine textures, and sub-pixel motion): crops are compared by their cores (the main piece plus pieces at least 10 % of its size, so compression specks are ignored) — a perceptual-hash prefilter (≤ 16 reliable bits), core sizes within 3 px or 12 %, shape mismatch (core pixels with no core pixel within 1 px in the other, best alignment with centres matched ±2 px) up to 4 %, and 3×3-averaged colour differences up to 12 % in every quadrant (compared in phase, ±1 px) — and each object's crops are grouped adaptively (tight micro-groups merged by average linkage up to that rule; for sprites under 32 px, further when the object's own distances show a clear gap above it — larger sprites keep the rule, so a smooth series of poses each just above it is never collapsed). A static object seen in 200 frames yields one appearance, and a looping pose is not split by sub-pixel position, texture, or compression. Limits (stated, not guaranteed): sprites under 32 px may still get an extra pose under heavy compression, and poses that differ by less than about 4 % of the sprite's pixels may be merged — both correctable in the Review tab.
- A4: Detections are linked into objects by tracking between consecutive processed frames (overlapping or nearby boxes, similar colors, similar size) and by shared appearances; an object with one distinct appearance becomes a single-asset cluster, one with several becomes an animation cluster whose poses are ordered by first appearance in the video, with repeats of a looping animation collapsed (within A3's scope and limits); distinct objects, including copies recoloured in one area, stay separate.
- A5: The Review tab lists clusters (single: thumbnail, "seen in N frames"; animation: filmstrip of its poses, "P poses, seen in N frames"), each with Keep (default) / Discard; Merge combines two or more selected rows into one animation (poses deduplicated and ordered by first appearance); Add Kept to Library adds kept clusters and closes the tab; discarded clusters are not added.
- A6: Animation entries in the Asset Library show a filmstrip thumbnail, the name `<video stem> animation <n>`, and the frame count; their frames share one size (the largest pose; each pose bottom-centre aligned); Save PNG… writes `<name>_01.png`, `<name>_02.png`, … , Save All writes the sequences too, Open as Document opens one layer per frame ("Frame 1", …); the `AssetLibrary` API exposes the frames.
- A7: On the fixture clip (200 frames: a skeleton looping an 8-pose walk while moving across a flat background, and a static potion), Extract Assets… with default settings and Add Kept to Library yields exactly one single potion asset and one 8-frame skeleton animation whose frames are the walk's poses in order.
- A8: In a release build the fixture clip is processed in under 10 s.

# Constraints and invariants

- Decoding and extraction run on a worker thread; the review and library changes happen only on Add Kept to Library.
- Every asset and animation frame is a tight (or bottom-centre padded) transparent crop; nothing changes open documents.

# Decisions

- Per-frame detection: C8's automatic keying + Auto-Detect (flat/near-flat backgrounds).
- Entry point: Extract Assets… on the video tab (From/To, Every N).
- Review: a center tab per run with Keep/Discard/Merge.
- Animations: one Asset Library entry per animation (numbered PNG sequence on save; one layer per frame when opened).
- Conventions adopted without asking: pHash of the crop over neutral gray (32×32 grayscale DCT, 8×8 low frequencies); dedup: perceptual hash over reliable bits (≤ 16, prefilter), core size within 3 px or 12 %, edge-tolerant core shape mismatch ≤ 4 % at centre-matched ±2 px alignment, 3×3-averaged interior colour ≤ 12 % per quadrant (in phase, ±1 px); adaptive per-object grouping (average linkage; gap above the fixed rule for sprites under 32 px only); robustness guaranteed for sprites ≥ 32 px on the short side; specks under 5 px dropped per frame; poses ordered by first appearance; frames bottom-centre aligned on a shared canvas; names `<video stem> <n>` for single assets and `<video stem> animation <n>` for animations; fixture rendered by a test generator and encoded with the ffmpeg CLI (H.264).
- Workspace: branch `comet/video-asset-grouping` off master.

# Open questions

- None. User confirmed outcome, scope, decisions, acceptance A1–A8, and non-goals on 2026-10-19.

# Verification expectations

- `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, existing release benchmarks, and the new video-extraction benchmark pass.
- Unit tests: perceptual hash, duplicate rule, tracking/linking, pose ordering and loop collapse, merge, frame alignment, library animation save/open.
- Fixture tests: the A7 result exactly (one potion asset, one 8-pose animation in walk order); a crf-18 clip of flat symmetric static shapes and a thin-limbed 4-pose figure gives three single assets and exactly four poses (every frame and every second frame); the same scene with sub-pixel motion at crf 28 still gives exactly four poses (every 1, 2, 3); a finely striped 5-pose figure and a striped crate at crf 28 give exactly five poses and one asset (every 1, 2, 3).
- Headless UI tests: Extract Assets dialog, progress/Cancel, Review tab rows and Keep/Discard/Merge, Add Kept to Library, library animation rows and actions.
