# Video Extraction

## Purpose

Extract assets from a whole video: find objects in every frame of a range, collapse repeats, group an object's poses into an animation, and review the result before adding it to the Asset Library.

## Requirements

### Running

- The video tab has Extract Assets… next to Extract Frames…. Its dialog: From frame and To frame (default the whole clip), Every N frames (1–1000, default 1), Minimum Size (default 16 px²), Merge Distance (default 2 px), Extract, Cancel.
- Extract decodes the exact frames From, From+N, … ≤ To on a worker thread and processes each: automatic keying (dominant border color, tolerance 10 %, softness 10 %, contiguous), removal of keyed specks (8-connected pieces of fewer than 5 pixels, typically compression noise, which Merge Distance would otherwise attach to nearby objects), and Auto-Detect with the dialog's Minimum Size and Merge Distance. Each detection is a tight transparent crop with its frame number and image rectangle.
- A progress bar shows frames processed; Cancel stops the run and discards it. When done, a center tab `<video name> review` opens with the clusters.

### Appearances and dedup

- Each crop's perceptual hash: the crop composited over neutral gray (128), converted to grayscale, resized to 32×32 (bilinear), 2-D DCT, the 8×8 lowest frequencies; each bit = coefficient > median of those 64. A bit is reliable when its coefficient differs from the median by more than a quarter of the coefficients' mean absolute deviation (flat or symmetric shapes put many coefficients at the median, where compression noise would flip them).
- Each crop's core: the opaque pixels (alpha ≥ 128) of its largest 8-connected piece and of pieces at least 10 % of that size (compression specks attached by Merge Distance are left out).
- Two crops are the same appearance when all of these hold:
  - their hashes differ in ≤ 16 of the bits reliable in both (a fast prefilter: compression moves flat or symmetric shapes' hashes by up to ~12 bits; shape and colour decide);
  - their cores' widths and heights each differ by at most the larger of 3 px and 12 %;
  - shape: aligning the cores' bounding-box centres and searching ±2 px around that, at the best alignment at most 4 % of the core pixels of either crop have no core pixel within 1 px (8-neighbourhood) in the other — edges that wobble by a pixel do not count, a limb that moved does;
  - colour: each crop's colours are averaged over 3×3 neighbourhoods of core pixels (fine textures compare by their local average); at the shape's alignment, refined by ±1 px so textures are compared in phase, the mean absolute difference of these averages over pixels interior to both cores (core pixels whose four neighbours are core; edges are where chroma subsampling bleeds) is ≤ 12 % of 255 in each quadrant of the core box that holds at least 10 % (and 8) of those pixels, and overall (0 when they share no interior) — so a copy recoloured in one area is not averaged away.
- Distance: the larger of shape mismatch / 4 % and colour difference / 12 % (infinite when the size or hash test fails); the rule above is distance ≤ 1.
- These hold for repeated appearances in H.264 video up to crf 28 for sprites whose core is at least 32 px on its short side, including flat-coloured symmetric shapes, thin limbs, fine high-contrast textures, and sub-pixel motion. Smaller sprites are handled on a best-effort basis (heavy compression can still add a pose), and poses differing by less than about 4 % of the sprite's pixels may be merged; the Review tab's Merge (and Discard) correct either.

### Objects and animations

- Tracking: going through processed frames in order, a detection continues an object seen in the previous processed frame when their boxes overlap or their centers are within the larger box dimension, their 16-bin RGB histograms (opaque pixels) differ by ≤ 0.35 (normalized L1 as in scene detection), and each size ratio is within 0.5–2; each object takes the closest match, at most one detection per frame.
- Linking: objects that share an appearance (same-appearance rule) are merged.
- An object's poses are found adaptively from all its crops (in frame order): crops join tight micro-groups (distance ≤ 0.4 to a group's first crop, trying the last group matched first); micro-groups merge by average linkage (the mean distance between their first crops; any infinite pair keeps them apart) while the merge distance is ≤ 1; for small sprites (median core under 32 px on its short side) merging continues up to the first clear gap above 1 (a merge distance at least 1.5× the largest before it), which lets them absorb their own compression noise — larger sprites stop at 1, so a smooth series of poses each just above the rule is never collapsed; groups of fewer than 3 crops and under 5 % of the object are folded into their nearest group when within distance 1. Each group's first crop is a pose; poses are ordered by the frame they first appear in. One pose → a single-asset cluster; two or more → an animation cluster.
- Linking compares the objects' poses with the rule (distance ≤ 1) and regroups the combined crops.

### Review tab

- One row per cluster, in order of first appearance: single — thumbnail, `seen in N frames`, first frame; animation — filmstrip of the poses, `P poses, seen in N frames`, first frame. Each row has Keep (default on) and Discard; rows are selectable (click, Cmd/Shift extend).
- Merge (enabled with two or more selected rows) replaces them by one animation cluster whose poses are their poses deduplicated with the same-appearance rule and ordered by first appearance.
- Add Kept to Library adds every kept cluster and closes the tab; closing the tab otherwise discards the run.

### Asset Library animations

- An animation entry holds its frames: every pose placed on a shared canvas of the largest pose width × largest pose height, horizontally centered and bottom-aligned. Name `<video stem> animation <n>` (n per video), source the video name.
- The library list shows a filmstrip thumbnail, the name, `P frames, W × H`. Save PNG… on an animation writes `<name>_01.png`, `<name>_02.png`, … (two digits, more if needed) next to the chosen name; Save All writes single assets as before and animations as their sequences; Open as Document opens a document of the frame size with one layer per frame named `Frame 1` … (frame 1 at the bottom); Remove removes it.
- Single-asset clusters are added as ordinary assets named `<video stem> <n>`.
- The `AssetLibrary` API exposes whether an entry is an animation and its frames in order.

### Performance

- The fixture clip (200 frames, 640×360) is processed in under 10 s in a release build.
