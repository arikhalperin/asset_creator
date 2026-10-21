# Outcome

A paper-model PDF printed on A3 (or any page size) can be opened, its model parts are found automatically (local analysis plus MobileSAM for refining), and the parts are re-laid out at true printed size onto A4 pages — packed automatically, then adjustable by hand — and exported as a printable A4 PDF. A part too big for A4 (e.g. a wing) is flagged, and the user splits it with a straight cut that adds a glue strip and alignment marks.

# Scope

- Open PDF: File ▸ Open accepts .pdf; pages are rendered at 300 dpi by PDFium (downloaded on first use after consent, SHA-256 checked); page size read from the PDF (A3 expected, any size accepted).
- Parts Layout tab: source pages with detected parts outlined; A4 sheets with placed parts; a tray for unplaced and oversized parts.
- Part detection: ink on the paper background grouped into parts (outline, fold lines, tabs, and the part's printed number/label kept together — including labels printed inside an unfilled (white) part, whose whole inside belongs to the part); MobileSAM click to add or refine a part when the model is installed; user can merge, split into separate parts, or delete proposals (e.g. title text, instructions).
- Layout: parts keep their printed size; auto-pack onto as few A4 pages as possible (portrait, 5 mm margins, 2 mm spacing, 90° rotations allowed); then drag parts within/between pages, rotate 90°, add/remove pages, re-pack.
- Oversized parts: listed with a warning ("does not fit on A4: W × H mm"); Split: draw a straight line across the part; the two pieces get an overlap/glue strip (default 8 mm) on one side and matching alignment marks on both; split pieces go back to layout; repeat if still too big.
- Image folders: File ▸ Open Folder of Images… loads every PNG/JPEG/BMP/WebP image of a folder (natural file-name order) as the pages of one Parts Layout tab; a page's real size comes from the DPI stored in the file, or — when there is none — the image is taken as a full A3 sheet (portrait or landscape by its shape); the tab shows each image page's size and where it came from, and the DPI can be overridden per page (the page is re-scaled and its parts found again). Everything else (detection, layout, split, manual frames, export) works as for PDF pages.
- Manual decompose: a page can be marked "Manual (A4 frames)"; its parts are then left out of auto-packing, and the user places A4 frames (a full A4 sheet, 210 × 297 mm, so two frames tile an A3 page exactly) on the page instead — add, remove, drag freely, turn 90°; frames may overlap (the same page area can appear on several sheets); the page view can be zoomed in for precise placement. Each frame becomes one full A4 sheet of exactly that page area at true scale (printers without borderless printing may lose about 5 mm at the edges); a turned (landscape) frame is printed turned 90° onto a portrait sheet.
- Export A4 Pages…: one multi-page A4 PDF (300 dpi raster) at true scale, plus optional PNG per page: packed sheets first, then the frame sheets (page order, frame order).
- Fixtures: generated synthetic A3 model sheets (parts with outlines, fold lines, tabs, numbers; a wing longer than A4; a title block) with known part rectangles.

# Non-goals

- Saving/reopening a layout session; editing the PDF's vector content; non-straight (curved) cuts.
- Arbitrary-angle rotation; automatic splitting without the user's line; printer-specific scaling or tiling.
- Bundling PDFium or the AI models in the app.

# Acceptance examples

- A1: File ▸ Open of a PDF asks before downloading PDFium the first time (SHA-256 checked, progress, failure keeps the app usable), then opens a Parts Layout tab with every page rendered at 300 dpi and its size in mm shown (A3 = 297 × 420 mm).
- A2: On the fixture sheets, part detection finds every model part (each known part rectangle matched by one proposal within 2 mm, part numbers and tabs inside their part — also for unfilled (white) parts with a number printed inside, whose inside is part of the part) and nothing else except proposals the user can delete (title block); a click selects the innermost proposal under it; the user can merge two proposals, delete one, and (with MobileSAM installed) click to add or refine a part (a click inside a part, filled or not, replaces it).
- A3: Auto-pack places all parts that fit onto A4 sheets at exactly their printed size (mm), within 5 mm margins, at least 2 mm apart, never overlapping, rotated only by 90°, using no more sheets than a simple shelf packing would; parts can be dragged within and between sheets, rotated 90°, and the sheets re-packed; illegal placements (overlap or outside the margin) are refused or flagged.
- A4: A part larger than A4's printable area in both orientations is not placed; the tab shows a warning naming it and its size; the user draws a straight cut line across it; the result is two pieces with an 8 mm glue strip (configurable 0–20 mm) on one and alignment marks on both; pieces that still do not fit are flagged again; after splitting, every piece can be placed.
- A5: Export A4 Pages… writes one A4 PDF (210 × 297 mm pages, 300 dpi images) with one page per sheet; printed at 100 % every part has its original size (verified by measuring the exported pages: ±0.5 mm); optional PNGs per page.
- A7: A page can be marked "Manual (A4 frames)" in the tab: its parts leave the layout; A4 frames (full A4, 210 × 297 mm, shown at true scale on the page; the first frame is turned to match the page so that two frames tile an A3 page exactly) can be added, removed, dragged freely, and turned 90°, and may overlap; the page view zooms in (buttons and Cmd+scroll) for placing them; Export A4 Pages… adds one portrait A4 sheet per frame after the packed sheets, filled edge to edge with exactly the framed page area at true scale (±0.5 mm), turned 90° for landscape frames; unmarking the page brings its parts back into the layout.
- A8: File ▸ Open Folder of Images… opens a folder's images (PNG, JPEG, BMP, WebP; other files ignored; natural name order, e.g. page2 before page10) as one Parts Layout tab without blocking the UI; a 300-dpi PNG/JPEG scan of an A3 sheet gets its size from the file (297 × 420 mm ± 1 mm) and yields the same parts as the PDF page; an image without DPI is taken as A3 by its shape; each image page shows "W × H mm (from file DPI | assumed A3)" and a DPI override that re-scales the page and re-detects its parts in the background; an empty folder shows a message.
- A6: Rendering an A3 page takes under 2 s, detection on a page under 2 s, and auto-pack of 100 parts under 1 s in a release build; the UI stays responsive (work on worker threads with progress).

# Constraints and invariants

- True scale is preserved through render, detection, layout, split, and export (mm ↔ px at 300 dpi).
- Without network, PDFium, or MobileSAM, the rest of the app works; without MobileSAM the PDF feature works with automatic detection only.

# Decisions

- Output: printable A4 PDF (300 dpi) with optional PNGs.
- Layout: auto-pack with 90° rotations, then manual adjustment.
- Split: straight cut + glue strip (default 8 mm) + alignment marks; the Split window lets the user choose which piece gets the strip (default: the smaller).
- Image folders (2026-10-21, user): page size from the file's DPI, else assumed A3; per-page DPI override.
- Manual decompose (2026-10-21, user): per-page "Manual (A4 frames)" mode; frames are full A4 (210 × 297 mm; changed from the printable area so two frames tile an A3 page), placed freely; the page view zooms; landscape frames are turned onto portrait sheets; frames may overlap, no extra marks or labels.
- PDF engine: PDFium (pdfium-render), downloaded on first use after consent.
- Conventions adopted without asking: detection by ink grouping on the paper colour with MobileSAM refine; A4 portrait sheets, 5 mm margins, 2 mm spacing; synthetic fixtures (the user's own papercraft PDFs are not copied into the repo).
- Workspace: branch `comet/pdf-parts-layout` off master.

# Open questions

- None. User confirmed outcome, scope, decisions, acceptance A1–A6, and non-goals on 2026-10-21, and the updates (A7 manual decompose with full-A4 frames and page zoom, tightened A2, glue side choice; A8 image folders; export off the UI thread) on 2026-10-21.

# Verification expectations

- `cargo test`, clippy `-D warnings`, `fmt --check`, existing release benchmarks, and new PDF benchmarks.
- Unit tests: mm/px scale, detection on fixtures, packing invariants, split geometry (glue strip, marks), PDF export page size and part scale.
- Headless UI tests: open PDF (with a local PDFium), consent flow, tab views, merge/delete/refine parts, drag/rotate/re-pack, oversized warning and split, export.
