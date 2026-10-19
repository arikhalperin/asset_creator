---
generated_from_state_version: 12
---

# Verification

## Current result

- Result: **Verification passed; ready to archive**
- Verification status: **Checks completed; result confirmed**
- Goal cycle: 3
- Iteration: 1
- Verifier attempt: 1
- Completed: 2026-10-19T09:02:24.762Z
- Summary: All acceptance items A1-A8 pass. cargo test (all suites) passes; MobileSAM fixture and download->MobileSAM UI tests actually ran with the local models; clippy -D warnings and fmt --check are clean; release benchmarks are far inside the A8 limits (RB 26 ms, AD 19 ms, GrabCut 99 ms, SAM first 432 ms / later 49 ms). Independent probes confirmed lazy loading (runtime not loaded before the first prompt), non-blocking UI (<=1 ms frames during encoding), exact alpha scaling and tight crops, corrupt/truncated download handling with GrabCut fallback and later switch to MobileSAM, and fixture IoUs with alternate click points. Main risk is the thin IoU margin on the potion fixture and GrabCut click robustness on harder images.

## Acceptance

| ID | Result | Source | Criterion | Reason |
| --- | --- | --- | --- | --- |
| A1 | passed | brief.md | A1: Extract → Remove Background… opens a preview dialog: Background color (auto: the most common border color; or picked by clicking the canvas), Tolerance (default 10 %), Edge Softness (default 10 %), Contiguous (default on), Preview; OK records one undo step "Remove Background" on the active layer (through the selection), Cancel restores exactly. On the flat and near-flat fixture sheets every background pixel becomes fully transparent, sprite interiors keep their exact pixels, and anti-aliased edge pixels become partially transparent with the background color removed from their color. | Extract menu -> Remove Background... opens RemoveBackgroundDialog (Auto dominant border color quantized 5 bit + mean, Pick via canvas click, Tolerance 10, Edge Softness 10, Contiguous on, Preview, Reset, OK, Cancel) implemented as a hidden filter through begin/preview/commit/cancel_filter (selection-aware). extract_ui test remove_background_previews_picks_and_commits_one_step passes: live preview keys bg, Preview off restores original, Pick samples clicked color, Cancel restores byte-exact, OK adds exactly one undo step labelled 'Remove Background', undo restores. tests/extract.rs remove_background_clears_background_and_keeps_sprites passes on flat and near-flat sheets against a coverage ground truth: every bg pixel alpha 0, interiors unchanged, >=100 soft edge pixels all partially transparent and decontaminated. |
| A2 | passed | brief.md | A2: Extract → Auto-Detect Objects… finds the objects in the active layer (keying the background automatically, without changing the layer, when it has no transparency) and shows them as numbered boxes on the canvas and a checklist with Minimum Size (default 16 px²) and Merge Distance (default 2 px); on every fixture sprite sheet (flat, near-flat, transparent) it proposes exactly one box per sprite with the ground-truth bounds (exact; ±1 px on the near-flat sheet), keeping detached parts within the merge distance together; Add to Library adds the checked proposals. | AutoDetectDialog keys with KeyParams::auto when the layer is fully opaque (copy, layer untouched - asserted in UI test), 8-connected labelling, bbox-gap merge (default 2 px), Min Size 16; auto_detect_finds_every_sprite_with_exact_bounds (flat + transparent, exact) and auto_detect_on_a_near_flat_background_is_within_one_pixel pass with count == sprite count; UI test shows 'N objects found, N selected', numbered canvas boxes, unchecking 2 adds N-2 assets. Unit test covers merging detached parts and dropping specks. |
| A3 | passed | brief.md | A3: The Extract Object tool (Tools panel, shortcut O) segments the object under a click, inside a dragged box, or refined by Shift-click (include) / Option-click (exclude) points on the active layer; the mask shows as a tinted overlay; Add to Library (Enter), To Selection, and Clear (Esc) are in the tool options; segmentation runs in the background (the UI stays responsive) and the tool options show which engine produced the mask. | Tool::ExtractObject in Tools panel with shortcut O; canvas emits ExtractPoint (Replace / Shift Include / Alt Exclude) and ExtractBox on drag; tinted mask overlay + point/box markers drawn; tool options strip shows engine status and Add to Library (Enter), To Selection, Clear (Esc), Download Model.... Segmentation runs on the 'segmenter' worker thread (newest request wins). UI test extract_object_with_grabcut_click_box_points_and_accept passes (click, shift/alt points, box, Enter, Esc, undoable To Selection, 'GrabCut - ONNX Runtime not found' shown). Own probe: extract_prompt returned in 0.1 ms and the worst frame while MobileSAM encoded was 1.0 ms. |
| A4 | passed | brief.md | A4: Click-to-extract on each fixture photo returns a clean mask — IoU ≥ 0.90 with the ground truth, one connected region, no holes or islands larger than 0.5 % of the object — with MobileSAM and with the GrabCut fallback. | extract.rs grabcut_click_returns_a_clean_mask_on_every_photo and mobilesam_click_returns_a_clean_mask_on_every_photo pass (MobileSAM ran with GAC_MODEL_DIR set - no 'skipped' line). Independent probe on all 3 fixtures with given + alternate interior clicks: GrabCut IoU potion 0.905, chalice 0.960-0.961, skull 0.980; MobileSAM potion 0.904-0.905, chalice 0.993, skull 0.997; all exactly one region; hole/island check enforced by assert_clean. |
| A5 | passed | brief.md | A5: Models load lazily and degrade gracefully: nothing ML-related loads before the first Extract Object; without ONNX Runtime or the models the tool uses GrabCut and says why; the first use asks before downloading the MobileSAM models (~45 MB) into the app's data folder in the background with progress; files are SHA-256 checked; declining, a failed download, or a corrupt file leaves GrabCut in use (no partial file is used) and later extractions switch to MobileSAM once the models are ready. | Probe: sam::runtime_loaded()=false and segmenter not started after opening a doc, selecting the tool, and running Auto-Detect; loads only after first prompt. Without runtime -> Availability::Unavailable with reason shown and GrabCut used (UI test). Missing models -> consent 'Download MobileSAM?' (Download / Use GrabCut) while GrabCut answers; download on background thread with progress ('Downloading MobileSAM... n %'), writes .part, SHA-256 verifies, renames. Damaged-source download: UI test asserts error shown, model dir empty, GrabCut still used; probe with truncated second file left only the verified first file, no .part. Probe with same-size corrupted models: NeedsModels('...damaged (checksum mismatch)'), GrabCut used, decline keeps GrabCut, then Download re-fetches and the next prompt uses MobileSAM (verify Ok). downloaded_models_switch_extract_object_to_mobilesam passes with GAC_MODEL_SOURCE. |
| A6 | passed | brief.md | A6: Every asset — from Auto-Detect, Extract Object, or Add Selection to Library — is a tight transparent crop: its size is the bounding box of its non-transparent pixels, pixels outside the extraction are fully transparent (partial mask coverage scales alpha), and its saved PNG is RGBA and reloads identical. | All three paths use extract::tight_crop/tight_crop_region (alpha *= coverage rounded, crop to alpha>0 bbox, empty -> not added). every_extraction_is_a_tight_transparent_crop passes (Auto-Detect rects == proposal rects, PNG reloads as Rgba8 identical; GrabCut asset tight with transparent outside). UI test: Add Selection crop of a circle equals the detected asset (same rect and pixels). Probe: partial-coverage mask gives alpha round(180*c/255) exactly (7, 169) with rect excluding zero-coverage column; Extract Object accept via app produced a tight 190x241 asset. |
| A7 | passed | brief.md | A7: The Asset Library panel lists assets (thumbnail, name `<document> <n>`, size) in the order added; Save PNG… saves the selected asset, Save All… writes every asset into a chosen folder with unique file names, Open as Document opens it as a new image document, Remove deletes selected assets; the library lasts for the session and is available through the `AssetLibrary` API (name, image, source document, source rectangle). | asset_library_panel lists in insertion order with 48 px thumbnail, name '<stem> <n>', 'w x h'; click / Cmd toggle / Shift range selection; Save PNG... (single selection, rfd dialog -> save_png RGBA), Save All... (folder, unique ' (2)' names, never overwrites existing files - unit test), Open as Document (new image document named after the asset), Remove. AssetLibrary API exposes id, name, image, source, rect. asset_library_panel_saves_opens_and_removes and names_count_per_document_and_save_all_dedupes pass. Library is session-only (in AppState). |
| A8 | passed | brief.md | A8: Release benchmarks: Remove Background preview on 1920×1080 < 150 ms; Auto-Detect on a 2048×2048 sheet < 250 ms; GrabCut mask on 1920×1080 < 2 s; MobileSAM first mask on an image < 2 s and later masks on the same image < 200 ms. | cargo test --release --test extract -- --ignored with models: Remove Background preview 1080p 26.2 ms (<150), Auto-Detect 2048x2048 19.5 ms (<250), GrabCut 1080p 99 ms (<2000), MobileSAM first 432 ms (<2000), later 49 ms (<200); assertions enforced in release. Second run 25.9/18.0/100/421/50 ms. Own probe: Auto-Detect including keying on an opaque 2048x2048 sheet 55-60 ms. All other release benches pass (cargo test --release -- --ignored exit 0). |

## Checks

| Check | Command | Working directory | Status | Exit | Duration |
| --- | --- | --- | --- | ---: | ---: |
| cargo test | test | . | passed | 0 | 56466 ms |
| extraction tests with MobileSAM models | GAC_MODEL_DIR=/private/tmp/claude-502/-Users-arikha-game-asset-creator/d82aeee0-b73c-4042-b7f8-98127421b63b/scratchpad/models GAC_MODEL_SOURCE=/private/tmp/claude-502/-Users-arikha-game-asset-creator/d82aeee0-b73c-4042-b7f8-98127421b63b/scratchpad/models cargo test --test extract --test extract_ui -- --nocapture | . | passed | 0 | 16056 ms |
| release extraction benchmark (with MobileSAM) | GAC_MODEL_DIR=/private/tmp/claude-502/-Users-arikha-game-asset-creator/d82aeee0-b73c-4042-b7f8-98127421b63b/scratchpad/models cargo test --release --test extract -- --ignored --nocapture | . | passed | 0 | 51827 ms |
| release video benchmark | test --release --test video -- --ignored --nocapture | . | passed | 0 | 5537 ms |
| release filter preview benchmark | test --release --test filters -- --ignored --nocapture | . | passed | 0 | 4668 ms |
| release pan/zoom benchmark | test --release --test canvas -- --ignored --nocapture | . | passed | 0 | 2543 ms |
| release transform benchmark | test --release --lib bench_transform -- --ignored --nocapture | . | passed | 0 | 9740 ms |
| cargo clippy | clippy --all-targets -- -D warnings | . | passed | 0 | 3828 ms |
| cargo fmt check | fmt --check | . | passed | 0 | 279 ms |

### Builder-reported evidence

These are Builder reports, not Runtime check receipts or independent verification results.

- cargo test: passed — 288 tests incl. tests/extract.rs (exact sprite bounds on flat/transparent sheets, +-1 on near-flat, Remove Background vs coverage map, GrabCut and MobileSAM click masks IoU>=0.9/one region/no holes, tight transparent crops with PNG round trip) and tests/extract_ui.rs (7: remove background preview/pick/cancel/OK/undo, auto-detect add, Extract Object click/points/box/selection/accept/clear with GrabCut, consent + damaged download keeps GrabCut, real download switches to MobileSAM when GAC_MODEL_SOURCE is set, asset library save/open/remove, menu enablement). Without GAC_MODEL_DIR/GAC_MODEL_SOURCE the two MobileSAM-dependent tests report 'skipped'; the extract-sam check sets them.
- clippy -D warnings: passed — —
- fmt --check: passed — —
- release benchmarks: passed — remove background 25 ms, auto-detect 2048^2 18 ms, GrabCut 1080p 99 ms, MobileSAM first 385 ms / later 46 ms; video, filters, canvas, transform unchanged.
- Known limitation: MobileSAM models are not in the repo; checks use the copy downloaded into the scratchpad (same files and SHA-256 as the app downloads). The user's app data folder has no models yet: the first Extract Object will ask to download.
- Known limitation: GrabCut on the potion photo scores IoU 0.905 (threshold 0.90): the pale cork differs from the bottle colour; chalice 0.96, skull 0.98.
- Known limitation: ONNX Runtime was installed with Homebrew (user-approved) and is loaded at run time; without it the app uses GrabCut.
- Known limitation: Auto-Detect runs synchronously when the dialog opens or its parameters change (18 ms on 2048x2048).

## Blockers

_None._

## Risks and skipped work

- Potion fixture IoU margin is thin for both engines (GrabCut 0.905, MobileSAM 0.904-0.905 vs 0.90 threshold): a body click never includes the differently coloured neck, so small fixture or algorithm changes could flip A4.
- GrabCut click robustness beyond the fixtures is limited: on an independent synthetic striped object with a cast shadow (800x600) a click gave IoU 0.80 (box prompt 1.00; MobileSAM 0.99); a click on the exact object edge of the chalice gave IoU 0.01.
- The A8 Auto-Detect benchmark times detection on an already-transparent sheet (keying excluded); measured independently with keying on an opaque 2048x2048 sheet it is still ~60 ms, well under the limit.
- Spec says the mask overlay has an outline; only the tinted fill plus point/box markers are drawn (A3 only requires the tint).
- Remove Background preview and Auto-Detect keying/detection run synchronously on the UI thread (permitted: the constraint covers segmentation/model/download only; they take <60 ms on large images).
- Download: if the final rename fails the .part file is not removed (rare); models::present checks sizes only, but SamModel::load SHA-verifies before use so damaged files are never used.
- Save PNG.../Save All... file dialogs (rfd) are not exercised headlessly; the underlying save_asset_to/save_all_assets_to are tested.

## Previous iterations

| Goal cycle | Iteration | Attempt | Outcome | Unresolved | Summary | Completed |
| ---: | ---: | ---: | --- | --- | --- | --- |
| 1 | 0 | 0 | recovery | — | Native Shape artifacts changed | 2026-10-18T15:56:18.067Z |
| 2 | 1 | 0 | recovery | — | Native Shape artifacts changed | 2026-10-19T08:50:57.944Z |
| 3 | 1 | 1 | pass | — | All acceptance items A1-A8 pass. cargo test (all suites) passes; MobileSAM fixture and download->MobileSAM UI tests actually ran with the local models; clippy -D warnings and fmt --check are clean; release benchmarks are far inside the A8 limits (RB 26 ms, AD 19 ms, GrabCut 99 ms, SAM first 432 ms / later 49 ms). Independent probes confirmed lazy loading (runtime not loaded before the first prompt), non-blocking UI (<=1 ms frames during encoding), exact alpha scaling and tight crops, corrupt/truncated download handling with GrabCut fallback and later switch to MobileSAM, and fixture IoUs with alternate click points. Main risk is the thin IoU margin on the potion fixture and GrabCut click robustness on harder images. | 2026-10-19T09:02:24.762Z |



## Conclusion

All acceptance items A1-A8 pass. cargo test (all suites) passes; MobileSAM fixture and download->MobileSAM UI tests actually ran with the local models; clippy -D warnings and fmt --check are clean; release benchmarks are far inside the A8 limits (RB 26 ms, AD 19 ms, GrabCut 99 ms, SAM first 432 ms / later 49 ms). Independent probes confirmed lazy loading (runtime not loaded before the first prompt), non-blocking UI (<=1 ms frames during encoding), exact alpha scaling and tight crops, corrupt/truncated download handling with GrabCut fallback and later switch to MobileSAM, and fixture IoUs with alternate click points. Main risk is the thin IoU margin on the potion fixture and GrabCut click robustness on harder images.
