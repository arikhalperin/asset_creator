---
generated_from_state_version: 13
---

# Verification

## Current result

- Result: **Archived**
- Verification status: **Checks completed; result confirmed**
- Goal cycle: 1
- Iteration: 2
- Verifier attempt: 1
- Completed: 2026-10-18T06:39:25.733Z
- Summary: All of A1-A8 pass. cargo fmt --check is clean, clippy --all-targets -D warnings is clean, cargo test passes in every suite, and the release preview benchmark's slowest case is 37.5 ms. All three repairs are implemented and tested: Save/Export are refused during a live edit with the dialog kept open, filter results on a mask are written as luminance (gray is exact), and Add Noise uses absolute coordinates with unchanged goldens. Only minor UX risks remain.

## Acceptance

| ID | Result | Source | Criterion | Reason |
| --- | --- | --- | --- | --- |
| A1 | passed | brief.md | A1: Every filter is defined through the plugin API (id, name, category, parameter schema, apply) in its own file and registered in one registry; the Filter menu and dialogs are generated from the registry and schemas, so adding a filter needs only its file and a registry line. | src/filters/mod.rs defines the Filter trait (id, name, category, params schema, margin, pure apply) and one static 14-entry registry(); each filter has its own file under src/filters/; the Filter menu (ui.rs) and the dialog (filter_dialog.rs) iterate registry()/params() only; registry_ids_are_unique_and_schemas_valid passes. |
| A2 | passed | brief.md | A2: The Filter menu lists all 14 filters grouped Adjustments / Blur / Sharpen / Noise; filters with parameters open a preview dialog, Desaturate, Invert, and Sharpen apply immediately; items are disabled without a document or during a transform. | The Filter menu loops over Category::ALL with separators/headings in registry order; parameterless filters (Desaturate, Invert, Sharpen) have no '…' and go through apply_filter immediately; enabled only with a current document, no transform, and no open dialog; parameterless_filters_apply_immediately and menu_disabled_without_document_or_during_transform... pass. |
| A3 | passed | brief.md | A3: A preview dialog shows the filter's parameters (sliders, checkboxes, choices, seed, curve editor; grouped parameters follow their group selector), a Preview checkbox (default on), Reset, OK, and Cancel; the canvas shows the filtered result live after each change (the original when Preview is off); OK records one undo step named after the filter; Cancel restores the layer exactly. | filter_dialog::show draws schema widgets (slider/checkbox/choice/seed/curve editor), hides params outside the selected group, plus Preview (default true), Reset (keeps the seed), Cancel, OK; preview_filter writes the render or the 'before' snapshot; commit_filter records one PixelPatch named filter.name(); cancel restores. dialog_previews_live_ok_records_one_step, cancel_restores_exactly, grouped_parameters_per_channel_levels, and curve_editor_drag_brightens_midtones pass. |
| A4 | passed | brief.md | A4: Filters apply to the active layer, or its mask when the mask is the edit target, through the selection: unselected pixels are unchanged and partially selected ones interpolated; undo/redo restore exactly. | FilterSession targets open.paint_target() (pixels or mask) over editable_bounds; render_filter blends via blend_by_mask with selection coverage; PixelPatch stores before/after for exact undo/redo; filters_respect_selection_and_mask_target, render_filter_uses_context_and_selection, and filter_preview_commit_and_cancel pass. Repair (2): write_target_region and rgba_to_mask_value now use luminance_u8 (0.299/0.587/0.114, rounded, so gray stays exact); paint strokes already convert colors with paint::mask_gray (the same formula), so mask painting is unaffected; filtering_a_mask_writes_luminance passes. |
| A5 | passed | brief.md | A5: Color adjustments follow the spec formulas: Brightness/Contrast, Levels (RGB + per-channel), Curves (RGB + per-channel monotone cubic), Hue/Saturation/Lightness, Color Balance (tone ranges, preserve luminosity), Desaturate, Invert, Posterize — verified by unit tests on known values (e.g. Invert twice is identity, Posterize 2 yields only 0/255, identity Levels/Curves change nothing). | Brightness/Contrast, Levels (channel then master LUT), Curves (Fritsch-Carlson), HSL, Color Balance, Desaturate, Invert, and Posterize match the spec formulas I spot-checked; unit tests (invert_twice_is_identity, two_levels_are_black_and_white, identity_and_known_values, identity_monotone_and_through_points, tones_and_preserve_luminosity, etc.) pass. |
| A6 | passed | brief.md | A6: Gaussian Blur, Motion Blur, Sharpen, Unsharp Mask, Add Noise (seeded, uniform/gaussian, monochromatic), and Median follow the spec, blur in premultiplied alpha with edge clamping, and are deterministic — verified by unit tests (e.g. blur of a flat image is unchanged, median removes isolated specks, same seed gives same noise and a different seed different noise). | Premul + edge-clamped helpers; exact Gaussian below sigma 2, three-box above; motion blur, sharpen, unsharp, seeded noise, median, each with passing unit tests (flat blur unchanged, median removes specks and matches brute force, same/different seed). Repair (3): render_filter sets hidden _origin_x/_origin_y/_image_width after Params::clamp (clamp only touches schema keys, and the dialog's own Params never holds hidden keys); add_noise indexes (oy+y)*image_w+ox+x, which reduces to the old y*w+x without hidden params, so goldens are unchanged; noise_is_keyed_to_absolute_positions passes. |
| A7 | passed | brief.md | A7: Previews update in under 100 ms on a 1920×1080 layer in a release build for every filter at default parameters and at the large settings in the spec (blur radius 50, motion distance 100, unsharp radius 50, median radius 10), measured by a release benchmark. | cargo test --release --test filters -- --ignored --nocapture: bench_previews_1080p ok. Slowest: unsharp_mask large 37.52 ms, unsharp default 34.16, median large 30.47, motion large 29.46, gaussian large 24.58; all under 100 ms. |
| A8 | passed | brief.md | A8: Every filter has a golden-image regression test comparing a fixed synthetic input and fixed parameters byte-for-byte with a committed PNG in `tests/golden/` (regenerated with `UPDATE_GOLDEN=1`). | tests/golden has 14 PNGs (one per filter id); every_filter_matches_its_golden_image compares bytes against render_filter output on a fixed synthetic 64x48 image (UPDATE_GOLDEN=1 rewrites them), and goldens_differ_from_the_input guards against filters that do nothing; both pass. |

## Checks

| Check | Command | Working directory | Status | Exit | Duration |
| --- | --- | --- | --- | ---: | ---: |
| cargo test | test | . | passed | 0 | 10033 ms |
| release filter preview benchmark | test --release --test filters -- --ignored --nocapture | . | passed | 0 | 1805 ms |
| release pan/zoom benchmark | test --release --test canvas -- --ignored --nocapture | . | passed | 0 | 480 ms |
| release transform benchmark | test --release --lib bench_transform -- --ignored --nocapture | . | passed | 0 | 402 ms |
| cargo clippy | clippy --all-targets -- -D warnings | . | passed | 0 | 523 ms |
| cargo fmt check | fmt --check | . | passed | 0 | 223 ms |

### Builder-reported evidence

These are Builder reports, not Runtime check receipts or independent verification results.

- cargo test: passed — All suites pass incl. 3 new tests (147 unit, filters 3 + 1 ignored bench, filters_ui 9).
- release filter benchmark: passed — 1920x1080 render_filter: adjustments ~7 ms, gaussian 26 ms (radius 50: 26 ms), motion 19/29 ms, sharpen 21 ms, unsharp 36/40 ms, noise 9 ms, median 14/36 ms; all < 100 ms
- other release benchmarks: passed — pan/zoom 0.11 ms; transform 2.13 ms
- cargo clippy --all-targets -- -D warnings: passed — —
- cargo fmt --check: passed — —
- manual launch: passed — Release app starts; menu bar shows File, Edit, Select, Layer, Filter, View. Filter dialogs were exercised headlessly, not opened by hand.
- Known limitation: The benchmark measures the filter render path (render_filter on a full 1080p layer); the following canvas tile upload is not included.
- Known limitation: Golden images are byte-exact on this platform (f32 math, deterministic per-pixel order); a different CPU/compiler could in principle differ in rounding and require regenerating with UPDATE_GOLDEN=1.
- Known limitation: The dialog's sliders, checkboxes and choice buttons are driven through EditorApp setters in tests; the curve editor is driven with real pointer events.
- Known limitation: Gaussian sigma >= 2 uses the three-box approximation (as specified), so it is close to but not identical with a true Gaussian.
- Known limitation: Add Noise default seed comes from the system clock when the dialog opens.

## Blockers

_None._

## Risks and skipped work

- The A7 benchmark times render_filter only; it leaves out write_target_region and the canvas texture refresh that a real preview also pays (there is still about 60 ms of headroom).
- Repair (1): the Export menu item and Ctrl+S are not disabled during a live preview. The native file dialog opens first and the error comes after the user picks a path. The export refusal reuses IoError::Encode, so the message reads as an encode error. The behaviour is correct: nothing is written and the dialog stays open (save_and_export_wait_for_the_filter_preview).
- Quit or Close with a filter dialog open: choosing Save in the prompt is refused because a live edit is in progress, and the prompt is dropped. Don't Save works.
- Undo while a filter dialog is open cancels the preview and also undoes the previous step, matching the existing stroke behaviour. The spec wording is ambiguous on this point.
- Mask target: commit_filter compares RGBA after/before, so a colored result whose luminance equals the original records a no-op undo step (cosmetic only).
- Golden PNGs are untracked, so 'goldens unchanged' by repair (3) is shown by reasoning plus the passing golden test, not by a git diff.

## Previous iterations

| Goal cycle | Iteration | Attempt | Outcome | Unresolved | Summary | Completed |
| ---: | ---: | ---: | --- | --- | --- | --- |
| 1 | 1 | 1 | pass | — | All 8 acceptance items pass. The plugin API and registry of 14 filters (one file each), the schema-generated preview dialog with live on-canvas preview, the one-step undo named after the filter, selection and mask targeting, the spec formulas, premultiplied edge-clamped blurs, a seeded deterministic noise, the release 1080p benchmark at defaults and large settings, and byte-exact goldens for all 14 filters are implemented and tested. Remaining risks are minor: benchmark scope, noise keyed relative to the region, mask write-back from the R channel, and goldens tied to this platform. | 2026-10-18T06:00:20.519Z |
| 1 | 1 | 1 | recovery | — | User asked to fix before archiving: block Save/Export while a filter preview is open, write luminance gray when filtering a mask, key Add Noise to absolute image positions. | 2026-10-18T06:23:07.294Z |
| 1 | 2 | 1 | pass | — | All of A1-A8 pass. cargo fmt --check is clean, clippy --all-targets -D warnings is clean, cargo test passes in every suite, and the release preview benchmark's slowest case is 37.5 ms. All three repairs are implemented and tested: Save/Export are refused during a live edit with the dialog kept open, filter results on a mask are written as luminance (gray is exact), and Add Noise uses absolute coordinates with unchanged goldens. Only minor UX risks remain. | 2026-10-18T06:39:25.733Z |



## Conclusion

All of A1-A8 pass. cargo fmt --check is clean, clippy --all-targets -D warnings is clean, cargo test passes in every suite, and the release preview benchmark's slowest case is 37.5 ms. All three repairs are implemented and tested: Save/Export are refused during a live edit with the dialog kept open, filter results on a mask are written as luminance (gray is exact), and Add Noise uses absolute coordinates with unchanged goldens. Only minor UX risks remain.
