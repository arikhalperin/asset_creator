---
generated_from_state_version: 9
---

# Verification

## Current result

- Result: **Verification passed; ready to archive**
- Verification status: **Checks completed; result confirmed**
- Goal cycle: 2
- Iteration: 1
- Verifier attempt: 1
- Completed: 2026-10-18T15:09:03.207Z
- Summary: All nine acceptance items pass. cargo test (all suites, incl. 6 video + 11 video_ui tests), cargo clippy --all-targets -D warnings, and cargo fmt --check are clean; release benches pass with wide margins (A9: 12.8/39.6/0.72 ms vs 60/500/33). Independent probes confirmed byte-exact frames vs ffmpeg CLI for 60 non-committed frame/access-order combinations across all containers plus an unseen clip, scene detection follows the spec formula with large margins, and decoding stays off the UI thread for scrubbing/playback. Residual risks are minor UI-thread joins on open/close/cancel and ffmpeg-version coupling of the exactness guarantee.

## Acceptance

| ID | Result | Source | Criterion | Reason |
| --- | --- | --- | --- | --- |
| A1 | passed | brief.md | A1: File → Open lists the video formats; each fixture (H.264 .mp4, VP9 .webm, .mkv, MPEG-4 .avi, .mov) opens in a new center tab named after the file, showing frame 0 and an info line (resolution, frame rate, duration, current frame number and timecode). A file that cannot be decoded shows an error and adds no tab; closing the tab releases the decoder. A clip with 90° rotation metadata displays and grabs upright. | open_file_dialog adds a 'Videos' filter and video::EXTENSIONS to the combined filter; open_path routes is_video_path to open_video, which adds a center Tab::Video titled with info.name. tests/video.rs every_container_opens_with_its_info passes for mp4/webm/mkv/avi/mov/rotated/lossless/bt709 (90 frames, 30 fps). video_ui tests check frame 0 pixels, info line text ('160 × 120', '30.000 fps', 'frame 0 / 90', duration timecode), corrupt.mp4 -> error and no tab, close_video drops VideoTab (Player Drop sets quit and joins the worker), rotated.mov shows 120 × 160 and grabs upright. Verifier independently confirmed rotated.mov frame 50 is byte-identical to ffmpeg CLI autorotated output. |
| A2 | passed | brief.md | A2: Dragging the timeline shows the nearest preceding keyframe while dragging and the exact frame at the released position afterwards; the frame counter and timecode follow the displayed frame; decoding never blocks the UI thread. | Timeline slider sends Request::Preview while dragged and Request::Exact on drag_stopped/change (video_ui.rs show). Player is a dedicated worker thread with a single-slot Mutex+Condvar request (latest wins); VideoTab ignores answers whose request != latest; UI thread only polls via try_iter. Counter/timecode are drawn from displayed.index (with '(keyframe preview)' tag). keyframe_at seeks to keyframe at-or-before (test keyframe_preview_is_at_or_before_the_target; UI test scrubbing_previews_keyframes_then_shows_the_exact_frame passes). |
| A3 | passed | brief.md | A3: Play/Pause (button and Space) plays at the clip's frame rate, dropping frames if decoding falls behind, and stops on the last frame; Step Back/Forward (buttons and ←/→) move exactly one frame; Start/End (buttons and Home/End) jump to the first/last frame. | play() in player.rs paces by start + (index-from)/fps, skips conversion of frames more than one frame period late, any new request interrupts playback, and Event::Ended stops at the last frame. Step uses displayed index +/-1 clamped and seeks exactly; Start/End seek 0/last_frame. Space/Left/Right/Home/End consumed only when a video tab is focused and not typing. UI test transport_buttons_and_shortcuts passes (checks reference pixels after End/Step/Start, play/pause, play to end). |
| A4 | passed | brief.md | A4: The thumbnail strip shows evenly spaced thumbnails across the clip, filled in on a background thread, marks the current position, and clicking a thumbnail seeks to its frame. | layout_strip computes evenly spaced targets i*last/(n-1) for as many slots as fit the width (cap 64); Thumbnails worker thread opens its own VideoSource and sends keyframe thumbnails as they decode; current_slot highlights; click calls seek(frame index) (exact). UI test thumbnail_strip_fills_in_and_seeks_on_click passes. |
| A5 | passed | brief.md | A5: Grab Frame opens a new image document with the video's (display) dimensions, one layer containing the exact current frame, named `<video stem> frame <N>`. Grabbed frames are pixel-exact: identical to the source frames for a lossless (FFV1 RGB) fixture, and identical to `ffmpeg -pix_fmt rgba` output for that frame for the lossy fixtures — after opening, seeking, stepping, and scrubbing. | grab() decodes the exact frame first if a preview is shown; new_document_from_image creates a 72 DPI document with one 'Background' layer named '<stem> frame <N>', no source_path, existing documents untouched. Committed references match byte-for-byte in tests (all 8 clips, frames 0/17/45/89, in order, after preview, step fwd/back, sequential). Independent probe (scratch crate, public VideoSource API) vs fresh ffmpeg 7.1.1 '-vf select=eq(n,N) -pix_fmt rgba' PNGs for NON-committed frames: clip.mp4 f1/f63, clip.webm f31/f88, rotated.mov f50, clip.avi f59, clip.mkv f30, bt709.mp4 f72, clip.mov f2, scenes.mp4 f120, lossless.mkv f60 — each checked fresh, after scrubbing, step-forward, step-back, and sequential: 55/55 identical. A brand-new clip not in the repo (mandelbrot 176x100, 25 fps, H.264 -bf 3 -g 45) frames 99/0/46/44/77 in random order: 5/5 identical. |
| A6 | passed | brief.md | A6: Extract Frames… offers Every Nth frame (N, default 30) over a frame range (default the whole clip); it runs in the background with a progress bar and Cancel, and on completion appends frames start, start+N, … to the Frame Set (Cancel adds nothing). | ExtractDialog defaults n=30, from=0, to=last_frame; ExtractJob runs on a 'video-extract' thread with atomic progress and ProgressBar; Cancel sets the flag and drops the result (sender skipped when cancelled), dialog closed with nothing added; nth_targets = from, from+N, ... <= to. tests every_nth_extracts_exact_frames_in_range (17,45,73 pixel-exact) and UI extract_every_nth_frame_appends_to_the_frame_set (0,40,80 then appended 10,15,20) pass; scene test also verifies Cancel adds nothing. |
| A7 | passed | brief.md | A7: Scene-change mode (Threshold, default 30 %; Minimum scene length, default 10 frames) compares per-frame color histograms and appends the first frame and the first frame of every detected scene; on the scene-cut test clip it finds exactly the cut frames listed in the fixture file. | scene.rs matches the spec formula: box downscale to <=64 px longer side, 16 bins/channel normalized, d = (1/6) * sum \|h - h_prev\| over 48 bins, cut when d >= threshold and index >= last_cut + min_scene, From always included; defaults 30 % / 10 frames. scenes.mp4 detection equals [0,45,85,115,165,200] from scenes.json (unit + UI tests). Not overfit: verifier measured per-frame d on scenes.mp4 — cut frames score 0.56-0.90, the highest non-cut is 0.25 (frame 116, also within min-scene of 115), next 0.09, so threshold 0.30 has wide margin. |
| A8 | passed | brief.md | A8: The Frame Set panel shows every extracted frame as a thumbnail labelled with video name, frame number, and timecode; double-click (or Open) opens a frame as a new image document pixel-identical to Grab Frame of that frame; Remove deletes selected frames and Clear empties the set; the frames are available in-session through the `FrameSet` API (source path, frame index, timestamp, RGBA image). | frame_set_panel shows thumbnails labelled '<video name> #N' plus timecode and a frame count; click/Cmd/Shift selection; double-click or Open -> FrameSetAction::Open opens entry.image() (losslessly PNG-stored) as a document named like Grab; Remove/Clear implemented; FrameSet API exposes source, video_name, frame, timestamp, image(). Panel listed in Panel::ALL and default layout. UI test frame_set_panel_opens_removes_and_clears passes (Open is pixel-identical to lossless_f45 reference and to a second open), unit test round-trips the image. |
| A9 | passed | brief.md | A9: On a generated 1920×1080 H.264 30 fps clip with a keyframe every 60 frames, a release benchmark shows: keyframe preview while scrubbing < 60 ms per position, exact frame anywhere < 500 ms, and sequential playback decoding faster than real time (< 33 ms per frame). | cargo test --release -- --ignored: bench_scrub_1080p on generated 1920x1080 H.264 30 fps -g 60 clip: preview worst 12.8 ms (<60), exact worst 39.6 ms (<500, includes 59-frames-before-keyframe cases), sequential 0.72 ms/frame incl. RGBA conversion (<33). Independent verifier bench on a harder 1080p mandelbrot H.264 -g 60 clip: preview worst 26.2 ms, exact worst 69.6 ms, sequential 1.11 ms/frame. Other release benches pass (transform 3.13 ms, pan/zoom 0.112 ms, filters <=39.8 ms). |

## Checks

| Check | Command | Working directory | Status | Exit | Duration |
| --- | --- | --- | --- | ---: | ---: |
| cargo test | test | . | passed | 0 | 12315 ms |
| release video scrubbing benchmark | test --release --test video -- --ignored --nocapture | . | passed | 0 | 7431 ms |
| release filter preview benchmark | test --release --test filters -- --ignored --nocapture | . | passed | 0 | 2664 ms |
| release pan/zoom benchmark | test --release --test canvas -- --ignored --nocapture | . | passed | 0 | 1895 ms |
| release transform benchmark | test --release --lib bench_transform -- --ignored --nocapture | . | passed | 0 | 2386 ms |
| cargo clippy | clippy --all-targets -- -D warnings | . | passed | 0 | 536 ms |
| cargo fmt check | fmt --check | . | passed | 0 | 264 ms |

### Builder-reported evidence

These are Builder reports, not Runtime check receipts or independent verification results.

- cargo test: passed — 265 tests incl. tests/video.rs (6: containers/info, errors, exact frames vs ffmpeg references in in-order/backward/after-preview/step/sequential access for 8 clips, keyframe preview, scene cuts == fixture, every Nth) and tests/video_ui.rs (11: open + info, corrupt error, rotated upright grab, pointer scrub preview then exact, transport buttons/shortcuts/real-time playback/stop at end, thumbnail strip + click, Grab Frame pixel-exact incl. during preview, extract append, scene extraction + cancel, Frame Set double-click open/remove/clear, closing tabs).
- cargo clippy --all-targets -D warnings: passed — —
- cargo fmt --check: passed — —
- release benchmarks: passed — video 1080p H.264 GOP 60: preview worst 11.2 ms, exact worst 39.2 ms, sequential 0.77 ms/frame; filters, canvas, transform benchmarks unchanged and passing.
- Known limitation: Opening a video probes the file and decodes its first frame on the UI thread (like opening an image) so an undecodable file can be rejected without adding a tab; all later decoding runs on workers.
- Known limitation: The frame count of containers without one (mkv/webm) is estimated from the duration and corrected when decoding reaches the real end.
- Known limitation: Building now needs the ffmpeg development libraries (Homebrew ffmpeg); packaging them is left to C13.
- Known limitation: The 1080p benchmark clip is generated with the ffmpeg CLI (libx264) into the cargo target temp dir.

## Blockers

_None._

## Risks and skipped work

- VideoSource::open (including decoding/validating the first frame) runs synchronously on the UI thread in EditorApp::open_video; fast for normal files but a slow/huge/network file would stall the UI during open.
- Closing a video tab drops Player, whose Drop joins the worker thread on the UI thread (waits for one in-flight decode, bounded by ~exact-seek time); ExtractJob::cancel likewise joins the extraction thread on the UI thread.
- Every-Nth extraction silently skips a target if the decoder returns a different frame index (e.g. timestamp gaps), with no user notice.
- When playback reaches the end while behind schedule, the last frame may have been skipped, so the display can show an earlier frame while scrub is set to the last frame.
- Pixel-exactness relies on mirroring ffmpeg CLI behaviour (BICUBIC swscale flags, colorspace details, transpose filters); verified against ffmpeg 7.1.1 CLI only — a different linked ffmpeg version could diverge from committed reference PNGs.
- Thumbnail workers are detached on drop/resize (not joined); rapid window resizing can briefly run several thumbnail decoders concurrently.

## Previous iterations

| Goal cycle | Iteration | Attempt | Outcome | Unresolved | Summary | Completed |
| ---: | ---: | ---: | --- | --- | --- | --- |
| 1 | 0 | 0 | recovery | — | Native Shape artifacts changed | 2026-10-18T14:38:40.375Z |
| 2 | 1 | 1 | pass | — | All nine acceptance items pass. cargo test (all suites, incl. 6 video + 11 video_ui tests), cargo clippy --all-targets -D warnings, and cargo fmt --check are clean; release benches pass with wide margins (A9: 12.8/39.6/0.72 ms vs 60/500/33). Independent probes confirmed byte-exact frames vs ffmpeg CLI for 60 non-committed frame/access-order combinations across all containers plus an unseen clip, scene detection follows the spec formula with large margins, and decoding stays off the UI thread for scrubbing/playback. Residual risks are minor UI-thread joins on open/close/cancel and ffmpeg-version coupling of the exactness guarantee. | 2026-10-18T15:09:03.207Z |



## Conclusion

All nine acceptance items pass. cargo test (all suites, incl. 6 video + 11 video_ui tests), cargo clippy --all-targets -D warnings, and cargo fmt --check are clean; release benches pass with wide margins (A9: 12.8/39.6/0.72 ms vs 60/500/33). Independent probes confirmed byte-exact frames vs ffmpeg CLI for 60 non-committed frame/access-order combinations across all containers plus an unseen clip, scene detection follows the spec formula with large margins, and decoding stays off the UI thread for scrubbing/playback. Residual risks are minor UI-thread joins on open/close/cancel and ffmpeg-version coupling of the exactness guarantee.
