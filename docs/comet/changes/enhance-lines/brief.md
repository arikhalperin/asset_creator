# Outcome

Faded or grainy line work on a paper model becomes clear again: the user marks an area (an insignia, a wing section) and Enhance Lines makes its thin dark lines — insignia outlines, flap and panel lines, rivets — darker, crisper, unbroken, and optionally bolder, and can smooth the print grain of the paint around them, without changing the paint colours. It works on Parts Layout pages (inside drawn areas, so the A4 export is enhanced) and as Filter ▸ Enhance Lines… on the editor selection.

# Scope

## Source coverage

Sources supplied by the user on 2026-10-21 (all read completely):

| Unit | Content | Status | Classification | Maps to |
|---|---|---|---|---|
| S1 | "Add enhancer that will work on an area and enhance mechanical lines or insignia on the aircraft" | complete | executable | E1, E2, E6 |
| S2 | Image 1: US star-and-bar insignia drawn as thin dark-green outlines on green camouflage, low contrast, visible print grain — "If selected with the enhancer the US airforce markings will become more enhanced" | complete | executable (example input) | E1, E2, E3 (fixture insignia), E5 |
| S3 | "if I give it a wing section for example it will make the lines of the flaps more enhanced. etc" | complete | executable | E1, E3, E4 (fixture flap/panel lines) |
| S4 | Image 2: paper-model sheet — camouflage (two greens, tan), insignia outline, panel/flap lines, rivet circles, white parts with thin black lines, part numbers | complete | executable (example input) + background | E1, E2, E5; white parts/numbers: background (dark lines on white are lines too) |

## Features

- Line detection (image processing, no model download): dark strokes thinner than about 1 mm that are darker than the paint around them (black-hat of lightness against a local background), at 300 dpi page scale; light strokes are not lines.
- Effects, each with its own control:
  - Line strength (0–100 %, default 60 %): line pixels become a darker shade of their own colour (hue kept), reaching near-black at 100 %, with crisp edges.
  - Close gaps (0–2 mm, default 0.5 mm): breaks along a line up to that length are filled with the line's colour.
  - Thickness (1.0–2.0×, default 1.0×): lines are widened about their centre.
  - Smooth grain (0–100 %, default 30 %): print grain in the paint (non-line pixels) is smoothed; lines stay sharp.
- Parts Layout: Enhance Lines… window (toolbar) with its own areas (drag = rectangle, Shift + drag = lasso, Clear Areas), the four controls, live preview of the areas, Apply on a worker (progress, Cancel), Revert to Original; only pixels inside areas change; parts and placements are kept (no re-pack) and Export A4 Pages… uses the result.
- Editor: Filter ▸ Enhance Lines… on the active layer, limited to the selection (as every filter), with the same four controls, live preview, Cancel, and one undo step.
- Fixture: a generated test sheet with camouflage paints, halftone-like grain, a faded insignia outline, flap/panel lines with known gaps, and white light lines (which must stay unchanged).

# Non-goals

- AI/trained enhancement or super-resolution models; enhancing photos.
- Enhancing light lines (light strokes on darker paint) or recolouring paint (C14 Recolor does that).
- Redrawing insignia shapes, filling insignia with colours, or vectorising lines.

# Acceptance examples

- E1: On the fixture, with default settings, every faded line (insignia outline, flap and panel lines) has at least twice its original lightness contrast against the adjacent paint (or reaches L* ≤ 20), and each enhanced line pixel keeps its hue (within 15° for lines with chroma > 10) — a darker shade of its own colour; at Line strength 0 % lines are unchanged.
- E2: Paint is kept: with Smooth grain 0 %, every pixel farther than 1 mm from any line is unchanged (ΔE < 1); with Smooth grain 100 %, the grain (standard deviation of L* in flat paint) drops by at least half while the paint's mean colour moves by less than ΔE 2; light (white) lines on dark paint are unchanged (ΔE < 2).
- E3: Close gaps: gaps in a line shorter than the Close gaps setting are filled (every point along the line is at least 70 % as dark as the enhanced line), and gaps longer than 1.5× the setting stay open; at 0 mm no gap is filled.
- E4: Thickness 2.0× makes the fixture's lines 1.6–2.4× as wide as at 1.0× (measured across the line), and at 1.0× line width changes by at most 1 px.
- E5: Crisp edges: across an enhanced line, the transition from paint to line takes at most 2 px at 300 dpi.
- E6: Area limit: in the Parts Layout tab, only pixels inside the Enhance Lines areas change (outside: byte-identical; pages without areas unchanged); the areas are separate from Recolour Area's; rectangle and Shift-lasso both work; Clear Areas removes them.
- E7: Parts Layout Enhance Lines…: the preview follows every control change within 200 ms (release); Apply runs on a worker with progress and Cancel; afterwards parts keep their outlines and placements (no re-pack), Export A4 Pages… shows the enhanced lines, and Revert to Original restores the PDF render exactly.
- E8: Filter ▸ Enhance Lines… in the editor: limited to the selection (pixels outside unchanged), live preview, Cancel restores the layer exactly, OK is one undo step "Enhance Lines".
- E9: Enhancing a whole A3 page at 300 dpi takes under 3 s (release).

# Constraints and invariants

- Pixels outside the areas (Parts Layout) or the selection (editor) never change; alpha never changes.
- Paint colours are never shifted except by the grain smoothing (mean ΔE < 2).
- The original pages are kept so Revert is exact.

# Decisions

- Separate change after C14 (recolour), before release preparation (C16).
- Effects: darker crisper lines, close gaps, thicker lines, smooth print grain — each its own control.
- Line colour: a darker shade of each line's own colour, near-black at full strength.
- Available in the Parts Layout tab (inside drawn areas) and in the editor (Filter ▸ Enhance Lines… on the selection).
- Method: image processing (no model download).
- Enhance Lines areas are separate from Recolour Area's.
- Dark lines only; light lines are left alone.
- Conventions adopted without asking: defaults Line strength 60 %, Close gaps 0.5 mm, Thickness 1.0×, Smooth grain 30 %; lines are features thinner than about 1 mm; the editor filter scales millimetre settings by the document DPI, and Open Page in Editor documents now carry their page's DPI (300 for PDF pages); generated raster fixture.
- Workspace: branch `comet/enhance-lines` off master.

# Open questions

- None. User confirmed outcome, scope, decisions, acceptance E1–E9, and non-goals on 2026-10-22.

# Verification expectations

- `cargo test`, clippy `-D warnings`, `fmt --check`, the existing release benchmarks and release UI runs, and a new enhance benchmark (A3 page).
- Unit tests: line detection (dark vs light strokes), darkening keeps hue, gap closing limits, thickness, edge crispness, grain smoothing.
- Fixture tests: E1–E5 metrics on the generated sheet.
- Headless UI tests through the real controls: Parts Layout Enhance Lines… (areas, sliders, preview, Apply, no re-pack, export, Revert), editor Filter ▸ Enhance Lines… (selection, preview, Cancel, undo).
