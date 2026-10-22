---
generated_from_state_version: 27
---

# Verification

## Current result

- Result: **Archived**
- Verification status: **Checks completed; result confirmed**
- Goal cycle: 4
- Iteration: 1
- Verifier attempt: 1
- Completed: 2026-10-22T10:44:14.478Z
- Summary: All 17 Runtime checks for candidate 2376aceb exited 0 (cargo test, clippy, fmt, benchmarks, release UI runs). I re-measured E1-E9 independently with a probe crate. E4 now holds at every tested angle and in every isolated or alone configuration (2.0x ratio 1.67-2.16, 1.0x change <= 0.8 px). The only out-of-range case comes from an image-border closing artefact that cannot occur with the app's 4 mm or larger area margin; it is listed as a risk. Verdict: pass.

## Acceptance

| ID | Result | Source | Criterion | Reason |
| --- | --- | --- | --- | --- |
| A1 | passed | brief.md | E1: On the fixture, with default settings, every faded line (insignia outline, flap and panel lines) has at least twice its original lightness contrast against the adjacent paint (or reaches L* ≤ 20), and each enhanced line pixel keeps its hue (within 15° for lines with chroma > 10) — a darker shade of its own colour; at Line strength 0 % lines are unchanged. | Independent probe on test_sheet, defaults: all 8741 detected line px with chroma>10 keep hue (worst 2.9 deg, none >15). Contrast ratio from profile-averaged cuts along all flap lines (every 3 mm) and the ring (every 10 deg) is >=2 everywhere except two ring spots at 1.95-1.99, where the enhanced line is L* 8.9-9.4 (meets the 'or L* <= 20' branch). Strength 0: 0 detected line px changed (gaps 0 and default gaps). Test faded_lines_get_twice_the_contrast_and_keep_their_hue passed in the 2376aceb cargo-test log. |
| A2 | passed | brief.md | E2: Paint is kept: with Smooth grain 0 %, every pixel farther than 1 mm from any line is unchanged (ΔE < 1); with Smooth grain 100 %, the grain (standard deviation of L* in flat paint) drops by at least half while the paint's mean colour moves by less than ΔE 2; light (white) lines on dark paint are unchanged (ΔE < 2). | Smooth 0: 0 px farther than 1 mm from any line (flaps, ring, diagonal) with dE>=1. Smooth 100 (thickness 1.0 and 2.0): L* SD in four flat-paint patches (light green, dark green, tan x2) drops to 0.16x, mean dE 0.00-0.01; white line on dark green worst dE 0.00 (centre +/-1 px, 35.5-54.5 mm). Repo test passed. |
| A3 | passed | brief.md | E3: Close gaps: gaps in a line shorter than the Close gaps setting are filled (every point along the line is at least 70 % as dark as the enhanced line), and gaps longer than 1.5× the setting stay open; at 0 mm no gap is filled. | Gap 0.3 mm with 0.5/0.4/1.0/2.0 mm settings is filled (min darkness 0.87-0.99 of the line). Gap 0.6 mm at 0.7/1.0/2.0 mm is filled at L* 11-12, the same as the adjacent enhanced line on dark green (an earlier 0.68 ratio came from comparing with the light-green line, so it was a measurement artefact). Gap 1.2 mm stays open at 0.5 mm (and 0.6 mm at 0.4 mm). At 0 mm no gap is filled (<=0.03). Repo test passed. |
| A4 | passed | brief.md | E4: Thickness 2.0× makes the fixture's lines 1.6–2.4× as wide as at 1.0× (measured across the line), and at 1.0× line width changes by at most 1 px. | Metric: equivalent width = integral across the line of the normalised darkness (paint ref 1.0-1.6 mm each side, bilinear, 0.25 px steps), from a mean profile over 41 cuts along the line. FWHM is a secondary check. Whole fixture (5 flap spots, ring every 15 deg, diagonal at 3 points): 2.0x/1.0x = 1.67-2.10, 1.5x/1.0x = 1.33-1.52, \|w1.0-w0\| <= 0.73 px. Ring alone (crops with 2 mm and 1 mm margin, every 30 deg): 1.70-2.15, \|dw\| <= 0.80. Diagonal alone: 1.75-1.77. Flap segment alone: 1.99. Isolated 0.25 mm lines, 10 L* contrast, fixture-style grain, on light green, dark green and tan, 8 mm and 20 mm areas, 18 angles from 0 to 165 deg (108 cases): 1.78-2.16 at 2.0x, 1.35-1.58 at 1.5x, \|dw at 1.0x\| <= 0.22 px. FWHM: 1.67-2.27 for fixture and ring/diagonal, and 2.43-2.45 for a few 2 px isolated lines, a quantisation artefact (pixel count is exactly 2->4). The only out-of-range case was a ring crop with just 0.5 mm margin, caused by an image-border artefact (see risks); the app always adds a margin of 4 mm or more around areas. Repo tests passed (thickness_widens_lines, thickness_is_the_same_for_curves_and_diagonals, thickness_is_the_same_at_every_angle_for_isolated_lines, thickness_holds_for_the_ring_and_diagonal_alone). |
| A5 | passed | brief.md | E5: Crisp edges: across an enhanced line, the transition from paint to line takes at most 2 px at 300 dpi. | Single-pixel columns across the flap lines (41 columns at each of 4 spots), counting px between 20 % and 80 % of paint->line per side, worst case: 0 px at 1.0x, 1 px at 2.0x, 2 px at 1.5x (soft edge <= 1 px). All <= 2 px. Repo enhanced_edges_are_crisp passed. |
| A6 | passed | brief.md | E6: Area limit: in the Parts Layout tab, only pixels inside the Enhance Lines areas change (outside: byte-identical; pages without areas unchanged); the areas are separate from Recolour Area's; rectangle and Shift-lasso both work; Clear Areas removes them. | enhance_ui parts_layout_enhance_lines_in_drawn_areas passes in the release log and in my own release run with PDFium: rectangle and Shift-lasso give 2 enhance_areas, while tab.areas (Recolour) stays empty. No pixel changes outside the area boxes, a page without areas is byte-identical, and Clear Areas empties the list. Code: enhance() skips pixels where allowed==0 (polygon fill) and replace() only writes the region crop, so pixels outside the polygon are byte-identical. |
| A7 | passed | brief.md | E7: Parts Layout Enhance Lines…: the preview follows every control change within 200 ms (release); Apply runs on a worker with progress and Cancel; afterwards parts keep their outlines and placements (no re-pack), Export A4 Pages… shows the enhanced lines, and Revert to Original restores the PDF render exactly. | My release run: preview frame 33.6 ms after a setting change, and 126 ms for whole-page areas on both pages (budget-scaled). Apply runs on a worker thread through the shared page job (progress bar, Cancel calls cancel_recolor). Placements are unchanged (no re-pack), the A4 export differs after Apply, and Revert restores page images exactly (asserted). The preview pauses after Apply. |
| A8 | passed | brief.md | E8: Filter ▸ Enhance Lines… in the editor: limited to the selection (pixels outside unchanged), live preview, Cancel restores the layer exactly, OK is one undo step "Enhance Lines". | editor_enhance_lines_filter_on_the_selection passes in release: Filter menu has 'Enhance Lines…' with the 4 params, the preview darkens the line, Cancel restores the layer exactly, and OK adds 1 undo step labelled 'Enhance Lines'. Pixels right of the selection are unchanged. with_dpi is applied in the preview, OK and recompute paths. Open Page in Editor sets DPI 300. |
| A9 | passed | brief.md | E9: Enhancing a whole A3 page at 300 dpi takes under 3 s (release). | bench_enhance_a3_page (release, thickness 1.5): 658 ms in the Runtime log and 752 ms in my run, both under 3 s. |

## Checks

| Check | Command | Working directory | Status | Exit | Duration |
| --- | --- | --- | --- | ---: | ---: |
| cargo test (with the tagging model) | GAC_CLIP_DIR=/private/tmp/claude-502/-Users-arikha-game-asset-creator/d82aeee0-b73c-4042-b7f8-98127421b63b/scratchpad/clip GAC_MODEL_DIR=/private/tmp/claude-502/-Users-arikha-game-asset-creator/d82aeee0-b73c-4042-b7f8-98127421b63b/scratchpad/models GAC_PDFIUM_DIR=/private/tmp/claude-502/-Users-arikha-game-asset-creator/d82aeee0-b73c-4042-b7f8-98127421b63b/scratchpad/pdfium-installed GAC_PDFIUM_ARCHIVE=/private/tmp/claude-502/-Users-arikha-game-asset-creator/d82aeee0-b73c-4042-b7f8-98127421b63b/scratchpad/pdfium-dl cargo test | . | passed | 0 | 714358 ms |
| clippy -D warnings | clippy --all-targets -- -D warnings | . | passed | 0 | 6242 ms |
| fmt --check | fmt --check | . | passed | 0 | 479 ms |
| release pattern detection / make seamless benchmark | test --release --test patterns -- --ignored --nocapture | . | passed | 0 | 15760 ms |
| release pattern preview benchmark | GAC_CLIP_DIR=/private/tmp/claude-502/-Users-arikha-game-asset-creator/d82aeee0-b73c-4042-b7f8-98127421b63b/scratchpad/clip cargo test --release --test pattern_ui -- --include-ignored --nocapture | . | passed | 0 | 3948 ms |
| release filter preview benchmark | test --release --test filters -- --ignored --nocapture | . | passed | 0 | 3633 ms |
| release library/tagging benchmark | GAC_CLIP_DIR=/private/tmp/claude-502/-Users-arikha-game-asset-creator/d82aeee0-b73c-4042-b7f8-98127421b63b/scratchpad/clip cargo test --release --test library -- --ignored --nocapture | . | passed | 0 | 4947 ms |
| release extraction benchmarks | GAC_MODEL_DIR=/private/tmp/claude-502/-Users-arikha-game-asset-creator/d82aeee0-b73c-4042-b7f8-98127421b63b/scratchpad/models cargo test --release --test extract -- --ignored --nocapture | . | passed | 0 | 2991 ms |
| release video benchmark | test --release --test video -- --ignored --nocapture | . | passed | 0 | 4307 ms |
| release pan/zoom benchmark | test --release --test canvas -- --ignored --nocapture | . | passed | 0 | 2333 ms |
| release transform benchmark | test --release --lib bench_transform -- --ignored --nocapture | . | passed | 0 | 9859 ms |
| release PDF render/detect/pack benchmark | GAC_PDFIUM_DIR=/private/tmp/claude-502/-Users-arikha-game-asset-creator/d82aeee0-b73c-4042-b7f8-98127421b63b/scratchpad/pdfium-installed cargo test --release --test pdf_parts -- --ignored --nocapture | . | passed | 0 | 2221 ms |
| release PDF UI tests (frame-time limits) | GAC_PDFIUM_DIR=/private/tmp/claude-502/-Users-arikha-game-asset-creator/d82aeee0-b73c-4042-b7f8-98127421b63b/scratchpad/pdfium-installed GAC_PDFIUM_ARCHIVE=/private/tmp/claude-502/-Users-arikha-game-asset-creator/d82aeee0-b73c-4042-b7f8-98127421b63b/scratchpad/pdfium-dl GAC_MODEL_DIR=/private/tmp/claude-502/-Users-arikha-game-asset-creator/d82aeee0-b73c-4042-b7f8-98127421b63b/scratchpad/models cargo test --release --test pdf_ui | . | passed | 0 | 10363 ms |
| release recolour benchmark | GAC_PDFIUM_DIR=/private/tmp/claude-502/-Users-arikha-game-asset-creator/d82aeee0-b73c-4042-b7f8-98127421b63b/scratchpad/pdfium-installed cargo test --release --test recolor -- --ignored --nocapture | . | passed | 0 | 2490 ms |
| release recolour UI tests (preview frame limit, MobileSAM protect) | GAC_PDFIUM_DIR=/private/tmp/claude-502/-Users-arikha-game-asset-creator/d82aeee0-b73c-4042-b7f8-98127421b63b/scratchpad/pdfium-installed GAC_MODEL_DIR=/private/tmp/claude-502/-Users-arikha-game-asset-creator/d82aeee0-b73c-4042-b7f8-98127421b63b/scratchpad/models cargo test --release --test recolor_ui | . | passed | 0 | 6063 ms |
| enhance fixture tests + A3 benchmark (release) | GAC_PDFIUM_DIR=/private/tmp/claude-502/-Users-arikha-game-asset-creator/d82aeee0-b73c-4042-b7f8-98127421b63b/scratchpad/pdfium-installed cargo test --release --test enhance -- --include-ignored --nocapture | . | passed | 0 | 2943 ms |
| enhance UI tests (release, PDFium) | GAC_PDFIUM_DIR=/private/tmp/claude-502/-Users-arikha-game-asset-creator/d82aeee0-b73c-4042-b7f8-98127421b63b/scratchpad/pdfium-installed GAC_MODEL_DIR=/private/tmp/claude-502/-Users-arikha-game-asset-creator/d82aeee0-b73c-4042-b7f8-98127421b63b/scratchpad/models cargo test --release --test enhance_ui | . | passed | 0 | 5536 ms |

### Builder-reported evidence

These are Builder reports, not Runtime check receipts or independent verification results.

- all-dispatch-checks: passed — c15_all.sh: all 17 exit 0 (enhance A3 635 ms; previews 34 ms / 122 ms).
- Known limitation: Editor documents from image files without DPI metadata are treated as 72 dpi, so millimetre settings scale accordingly.
- Known limitation: Square gap closing can also fill between parallel lines closer than the gap setting.
- Known limitation: Thickness uses one typical stroke width per enhanced region.
- Known limitation: A 1 px (0.1 mm) break is bridged by the 3x3 median even at Close gaps 0 (below grain scale).
- Known limitation: Large-area previews are computed at reduced scale on the UI thread.
- Known limitation: Strokes under ~2 px (0.15 mm at 300 dpi) widen mostly by sub-pixel coverage, so a half-threshold pixel count of their width is coarse (1.5-3.4x at 2.0x); darkness-integrated width scales correctly.

## Blockers

_None._

## Risks and skipped work

- Binary gap closing pads the erosion with foreground at the image border, so a line within about g/2 of the enhanced image's edge is smeared out to the edge. Confirmed: a line 2 px from the bottom edge at gaps 0.5 mm, and 4 px at 1.0 mm, fills every pixel below it to the edge. The app adds a margin of 4 mm or more around areas, so this only affects lines at a page or document edge (or a selection at the document edge in the editor).
- Typical stroke width is one global value per enhanced region, so lines of different widths in one area widen by the same absolute amount, not each by the factor. The fixture is fine because all its lines are 0.25 mm.
- The 45 deg diagonal sits at the low end of the E4 range (1.67-1.77 at 2.0x; 1.27 at 1.5x for the diagonal alone).
- The preview of large areas runs at reduced scale (px_per_mm scaled), so it can differ slightly from the full-resolution Apply. Preview timing with many pages that have areas was only measured for 2 pages.
- Results depend on correct DPI. On the low-resolution user example (insignia screenshot), processing at 300 dpi turns the insignia into a blob. At about 72 dpi it enhances well, but straight panel lines look dashed.
- The Builder handoff text is partly stale: it mentions dilate_disc, but the code now uses an exact Euclidean distance transform.

## Previous iterations

| Goal cycle | Iteration | Attempt | Outcome | Unresolved | Summary | Completed |
| ---: | ---: | ---: | --- | --- | --- | --- |
| 1 | 1 | 0 | recovery | — | Native Shape artifacts changed | 2026-10-22T08:19:21.107Z |
| 2 | 1 | 1 | fail | A7 | A1-A6, A8, and A9 pass, verified independently on the fixture, generated gap and width sheets, the PDF fixture through the real UI, and the user's example images. All 17 Runtime checks passed. A7 fails: in a release build, the Parts Layout live preview takes 245 ms for a 200x150 mm area and about 0.6-0.7 s for a whole A3 page, over the 200 ms limit. The cause is that enhanced_preview always enhances at full resolution before taking its reduced-scale path. | 2026-10-22T08:43:14.429Z |
| 2 | 2 | 1 | fail | A4 | All 17 runtime checks for this candidate passed: cargo test, clippy -D warnings, fmt, 11 release benchmarks, and the enhance/pdf/recolor release UI runs. The A7 fix holds: every Parts Layout preview change measured 8-131 ms in release, from small areas up to whole pages on both pages, including real slider drags. Apply/Cancel, no re-pack, export and an exact Revert also work. A1-A3 and A5-A9 pass when probed independently. A4 fails. Thickness 2.0x gives the flap lines 2.33x, but the fixture's insignia ring gets 2.67x on average (2.33-3.67x by angle), because the widening uses a square structuring element. Use an orientation-independent (round) dilation and extend the thickness test to the ring or diagonal lines. | 2026-10-22T09:23:17.337Z |
| 2 | 3 | 0 | recovery | — | Native Shape artifacts changed | 2026-10-22T09:39:25.148Z |
| 3 | 1 | 1 | fail | A4 | All 17 Runtime check logs for ddc31bbf passed: cargo test 445 passed with 0 failed, clippy clean, fmt clean, and all benches and release UI runs ok. Independent release probes confirm A1-A3 and A5-A9; the preview speed and stacking problems from the last round are fixed. A4 fails: on the whole fixture, thickening is 2.0-2.35x everywhere, but the per-region stroke-width estimate from square erosion plus a coarse small-radius disc keeps it orientation-dependent. The fixture's ring selected alone gives 1.57-1.60x, its 45 deg stroke alone 1.50x, the flap line alone 2.33x. Added 30/60 deg diagonals give 1.59x, and single 0.25 mm diagonal lines 1.40-1.57x. | 2026-10-22T10:01:34.539Z |
| 3 | 2 | 0 | recovery | — | Native Shape artifacts changed | 2026-10-22T10:19:48.898Z |
| 4 | 1 | 1 | pass | — | All 17 Runtime checks for candidate 2376aceb exited 0 (cargo test, clippy, fmt, benchmarks, release UI runs). I re-measured E1-E9 independently with a probe crate. E4 now holds at every tested angle and in every isolated or alone configuration (2.0x ratio 1.67-2.16, 1.0x change <= 0.8 px). The only out-of-range case comes from an image-border closing artefact that cannot occur with the app's 4 mm or larger area margin; it is listed as a risk. Verdict: pass. | 2026-10-22T10:44:14.478Z |



## Conclusion

All 17 Runtime checks for candidate 2376aceb exited 0 (cargo test, clippy, fmt, benchmarks, release UI runs). I re-measured E1-E9 independently with a probe crate. E4 now holds at every tested angle and in every isolated or alone configuration (2.0x ratio 1.67-2.16, 1.0x change <= 0.8 px). The only out-of-range case comes from an image-border closing artefact that cannot occur with the app's 4 mm or larger area margin; it is listed as a risk. Verdict: pass.
