# Game Asset Creator — Comet Native Change Plan

An image editor with GIMP-style capabilities, plus the ability to open a video or image
file and extract game assets from it (chalices, skeletons, potions, deathtraps, wall
patterns, …).

## Prerequisites (not Comet changes)

```bash
cd ~/game_asset_creator
git init
comet init          # enable the Native workflow in .comet/config.yaml
```

**Suggested stack** (change C1 is where this gets locked in — adjust there if you prefer
something else): Python 3.11+, PySide6 (Qt) for the UI, numpy + OpenCV + Pillow for the
pixel pipeline, PyAV/ffmpeg for video decode, rembg or MobileSAM for segmentation,
imagehash for dedup, open_clip for zero-shot asset tagging.

Run each change below as its own `/comet-native` invocation, in order, pasting the prompt
verbatim. Each one is scoped to shape → build → verify → archive cleanly before the next
begins. Changes marked ∥ can run as parallel tracks once their dependency is archived.

---

## Phase A — Editor foundation

### C1 — App shell and document model

> Create the foundation of a desktop image editor called Game Asset Creator, using Python
> 3.11+ and PySide6. Deliver: a main window with dockable panels (tools, layers, asset
> library placeholder), a tabbed document area, an in-memory document model (document →
> ordered layers → RGBA numpy pixel buffers, with document metadata like size and DPI),
> opening PNG/JPG/BMP/WebP files into a document, saving/exporting back to those formats,
> and a project skeleton with pyproject.toml, src layout, pytest set up, and a `run` entry
> point. Acceptance: `python -m game_asset_creator` opens the window; opening an image
> shows it in a tab; File → Export round-trips the pixels losslessly for PNG; unit tests
> cover the document model.

### C2 — Canvas rendering, zoom/pan, undo framework

> Build the editing canvas for Game Asset Creator. Deliver: a QGraphicsView-based canvas
> that composites the document's layers, smooth zoom (ctrl+wheel, fit/100% shortcuts) and
> pan (space-drag), a checkerboard backdrop for transparency, pixel-grid display above
> 8x zoom, and a command-pattern undo/redo framework (QUndoStack) that all future editing
> operations must go through. Acceptance: a 4000x4000 image pans and zooms at interactive
> framerates; undo/redo works for a sample "fill layer" command; canvas re-renders only
> dirty regions.

### C3 — Selection system

> Add a selection system to the editor. Deliver: rectangle, ellipse, freehand lasso,
> polygon lasso, and magic-wand (flood-fill by color tolerance) selection tools; a
> selection mask stored as an 8-bit alpha mask on the document; marching-ants rendering;
> boolean combine modes (replace/add/subtract/intersect via modifier keys); select
> all/none/invert; grow/shrink/feather operations. All subsequent destructive operations
> must respect the active selection mask. Acceptance: each tool produces the correct mask
> in unit tests on synthetic images; feather produces a soft edge; marching ants animate
> on canvas.

### C4 — Paint tools

> Add painting tools to the editor, all routed through the undo framework and clipped to
> the active selection. Deliver: brush and eraser with size/hardness/opacity and smooth
> stroke interpolation, color picker/eyedropper, foreground/background color swatches
> with a color dialog, paint bucket (flood fill with tolerance), gradient tool
> (linear/radial), and a clone stamp tool. Acceptance: strokes are anti-aliased with no
> gaps at fast mouse speeds; all tools respect selection masks and are undoable; tool
> options appear in the tool-options bar.

### C5 — Layers: blend modes, masks, transforms

> Complete the layer system. Deliver: layer panel operations (add/delete/duplicate/
> reorder/merge down/flatten), per-layer opacity and visibility, blend modes (normal,
> multiply, screen, overlay, add, darken, lighten), per-layer alpha masks editable with
> the paint tools, and layer transforms (move, free scale/rotate with interactive handles,
> flip). Add a native project file format (.gac — zipped layer PNGs + JSON manifest) with
> save/load. Acceptance: blend modes match reference formulas in tests; transform preview
> is interactive; .gac round-trips a multi-layer document exactly.

### C6 — Filters and adjustments ∥ (after C5; parallel with Phase B)

> Add filters and adjustments. Deliver: brightness/contrast, levels, curves, hue/
> saturation/lightness, color balance, desaturate/invert/posterize; gaussian blur, motion
> blur, sharpen/unsharp mask, add-noise and median (denoise); each with a live preview
> dialog, applied to the active layer through the undo stack and clipped to the selection.
> Register all of them through a small internal filter-plugin API (name, params schema,
> apply fn) so future filters are one file each. Acceptance: previews update under 100ms
> on a 1080p layer; every filter has a golden-image regression test.

---

## Phase B — Media intake and asset extraction

### C7 — Video import and frame browser (after C2)

> Add video support to Game Asset Creator. Deliver: opening common video files (mp4, mkv,
> avi, webm, mov) via PyAV/ffmpeg; a video panel with a scrubbable timeline, play/pause,
> frame-step, and thumbnail strip; "grab frame" to open the current frame as a new image
> document; and batch frame extraction (every Nth frame, or scene-change detection via
> histogram difference) into an in-session frame set used by the extraction pipeline.
> Acceptance: a 1080p video scrubs responsively; grabbed frames are pixel-exact; scene
> detection on a test clip finds the cut points listed in the test fixture.

### C8 — Asset extraction engine v1: segment and cut out

> Build the core asset-extraction engine. Input: any open image document or extracted
> video frame. Deliver: (1) automatic background removal for sprites on flat/near-flat
> backgrounds (dominant-color + tolerance keying with alpha matting of edges); (2) an
> ML-powered "extract object" mode — user clicks or drags a box over an object (chalice,
> skeleton, potion, deathtrap...) and a segmentation model (MobileSAM, with OpenCV GrabCut
> as the no-model fallback) returns a mask; (3) contour-based auto-detection that finds
> all distinct objects in a keyed image and proposes one crop per object; (4) each
> accepted extraction becomes a tightly-cropped transparent-PNG asset handed to the asset
> library (stub a simple list panel if C10 isn't built yet). Models must load lazily and
> degrade gracefully to classical CV when unavailable. Acceptance: on fixture sprite
> sheets the auto-detector finds every sprite with correct bounds; click-to-extract on a
> test photo returns a clean mask; all extractions produce tight, transparent crops.

### C9 — Cross-frame dedup and animation grouping (after C7 + C8)

> Extend the extraction engine to work across a whole video. Deliver: run extraction over
> a selected frame range; perceptual-hash (imagehash) dedup so the same chalice seen in
> 200 frames yields one asset; near-duplicate clustering that recognizes the same object
> in different animation poses and groups those variants into an ordered animation
> sequence (frame order from the video); and a review UI showing clusters with
> keep/discard/merge controls. Acceptance: a fixture clip containing a looping 8-frame
> skeleton walk and a static potion yields exactly one potion asset and one 8-frame
> skeleton animation group.

### C10 — Asset library, tagging, and export (after C8)

> Build the asset library. Deliver: a dockable library panel with thumbnails, search, and
> category filters; zero-shot auto-tagging of each extracted asset into user-editable
> categories (chalice, skeleton, potion, deathtrap, wall, prop, other) using open_clip,
> with manual re-tagging; persistent on-disk library (folder of PNGs + sqlite or JSON
> metadata: source file, frame, tags, extraction params); export of single assets,
> selected sets, and packed sprite sheets (grid and max-rects packing) with a JSON atlas;
> animation groups from C9 export as sprite-sheet strips or numbered PNG sequences.
> Acceptance: tagging a fixture set of labeled sprites reaches reasonable accuracy and is
> always manually correctable; sprite-sheet atlas coordinates are verified by test;
> library persists across app restarts.

### C11 — Tileable pattern extraction (after C8)

> Add wall/floor pattern extraction. Deliver: a "find repeating pattern" tool that detects
> the tile period of a repeating region via autocorrelation and proposes the minimal tile;
> a manual tile-crop mode with a live 3x3 tiled preview; a make-seamless operation
> (offset + edge blending/patch-based fix-up) applicable to any tile; and saving tiles
> into the asset library tagged as patterns, exportable as tileable PNGs. Acceptance: on
> fixture images of brick/stone walls the detector recovers the known tile size; seamless
> output shows no visible seam in the tiled preview test (edge-difference metric below
> threshold).

---

## Phase C — Integration and release

### C12 — Round-trip editing and workflow polish (after C6 + C10)

> Connect the editor and the extraction pipeline into one workflow. Deliver: "edit a
> copy" from the asset library opens the asset as a document and "save back to library"
> updates it (new version, thumbnail refresh); "extract from current document" runs the
> C8 engine on whatever is on canvas, honoring the active selection as the extraction
> region; drag-and-drop of library assets onto a canvas as new layers; and a one-click
> "clean up asset" action (auto-crop transparent borders, despeckle stray alpha pixels).
> Acceptance: full loop works — open video, grab frame, extract skeleton, edit it, save
> to library, drag it into a new composition.

### C13 — Packaging, performance, and docs

> Prepare Game Asset Creator for release. Deliver: performance pass (profile and fix the
> slowest canvas/extraction paths; move segmentation to a worker thread so the UI never
> freezes, with progress + cancel for batch extraction); PyInstaller packaging for macOS
> (document model weights download-on-first-use rather than bundled); a user guide
> (README + docs/ with the video→asset walkthrough); and a smoke-test script exercising
> open/edit/extract/export headlessly. Acceptance: packaged app launches on a clean
> machine; batch extraction of a 1-minute clip keeps the UI responsive; smoke test passes
> in CI.

---

## Dependency graph

```
C1 → C2 → C3 → C4 → C5 → C6 ──────────┐
       └→ C7 ───────┐                 ├→ C12 → C13
            C8 ─────┼→ C9             │
              └─────┴→ C10 ──────────-┘
              └→ C11 ─────────────────┘
```

After C5 is archived, one person/session can run Phase B (C7–C11) while another finishes
C6 — Comet Native requires each change to be owned separately, so never run two changes
against the same working tree at once; use separate branches/worktrees per change.
