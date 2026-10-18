# Outcome

Game Asset Creator opens video files (mp4, mkv, avi, webm, mov) in their own center tab with a frame view, scrubbable timeline, play/pause, frame-step, and thumbnail strip; "Grab Frame" opens the exact current frame as a new image document; batch extraction (every Nth frame, or scene changes found by histogram difference) appends frames to an in-session Frame Set panel that the extraction pipeline (C8) will consume.

# Scope

- Video decoding through the linked ffmpeg libraries (`ffmpeg-next`), on background threads; display rotation metadata honoured.
- File → Open accepts the five video formats; each video opens in its own center tab.
- Video tab: fitted frame view, info line, timeline (fast keyframe preview while dragging, exact frame on release), play/pause, frame-step, start/end, thumbnail strip.
- Grab Frame → new image document, pixel-exact.
- Batch extraction dialog (Every Nth frame / Scene changes, frame range) with progress and Cancel.
- Frame Set dockable panel (thumbnail grid; open, remove, clear) and an in-session `FrameSet` API for later changes.
- Test fixtures (small clips in each container, a scene-cut clip with its cut list) and a release scrubbing benchmark.

# Non-goals

- Audio playback or audio tracks of any kind.
- Video editing, trimming, encoding, or exporting video.
- Saving the Frame Set to disk or into `.gac` projects (it lives for the session only).
- Hardware-accelerated decoding; streaming URLs; image sequences as video.
- The extraction pipeline itself (C8) and dedup/animation grouping (C9).
- Variable playback speed, looping, zooming the video view.

# Acceptance examples

- A1: File → Open lists the video formats; each fixture (H.264 .mp4, VP9 .webm, .mkv, MPEG-4 .avi, .mov) opens in a new center tab named after the file, showing frame 0 and an info line (resolution, frame rate, duration, current frame number and timecode). A file that cannot be decoded shows an error and adds no tab; closing the tab releases the decoder. A clip with 90° rotation metadata displays and grabs upright.
- A2: Dragging the timeline shows the nearest preceding keyframe while dragging and the exact frame at the released position afterwards; the frame counter and timecode follow the displayed frame; decoding never blocks the UI thread.
- A3: Play/Pause (button and Space) plays at the clip's frame rate, dropping frames if decoding falls behind, and stops on the last frame; Step Back/Forward (buttons and ←/→) move exactly one frame; Start/End (buttons and Home/End) jump to the first/last frame.
- A4: The thumbnail strip shows evenly spaced thumbnails across the clip, filled in on a background thread, marks the current position, and clicking a thumbnail seeks to its frame.
- A5: Grab Frame opens a new image document with the video's (display) dimensions, one layer containing the exact current frame, named `<video stem> frame <N>`. Grabbed frames are pixel-exact: identical to the source frames for a lossless (FFV1 RGB) fixture, and identical to `ffmpeg -pix_fmt rgba` output for that frame for the lossy fixtures — after opening, seeking, stepping, and scrubbing.
- A6: Extract Frames… offers Every Nth frame (N, default 30) over a frame range (default the whole clip); it runs in the background with a progress bar and Cancel, and on completion appends frames start, start+N, … to the Frame Set (Cancel adds nothing).
- A7: Scene-change mode (Threshold, default 30 %; Minimum scene length, default 10 frames) compares per-frame color histograms and appends the first frame and the first frame of every detected scene; on the scene-cut test clip it finds exactly the cut frames listed in the fixture file.
- A8: The Frame Set panel shows every extracted frame as a thumbnail labelled with video name, frame number, and timecode; double-click (or Open) opens a frame as a new image document pixel-identical to Grab Frame of that frame; Remove deletes selected frames and Clear empties the set; the frames are available in-session through the `FrameSet` API (source path, frame index, timestamp, RGBA image).
- A9: On a generated 1920×1080 H.264 30 fps clip with a keyframe every 60 frames, a release benchmark shows: keyframe preview while scrubbing < 60 ms per position, exact frame anywhere < 500 ms, and sequential playback decoding faster than real time (< 33 ms per frame).

# Constraints and invariants

- The UI thread never waits on decoding; all decoding runs on worker threads with stale requests dropped (latest seek wins).
- Frame numbers count from 0 in presentation order; frame N is the frame displayed at timestamp N / frame rate (average frame rate of the stream).
- Grabbing or extracting never changes existing documents or their undo stacks.

# Decisions

- Decoder: linked ffmpeg libraries via the `ffmpeg-next` crate (building needs the ffmpeg development libraries, e.g. Homebrew `ffmpeg`).
- Each video opens in its own center tab (several videos at once).
- Scrubbing: nearest keyframe while dragging, exact frame on release or pause; Grab Frame always uses the exact frame.
- Frame Set: a dockable panel; batches append; remove and clear available; session-only.
- Conventions adopted without asking: no audio; frame-step and transport shortcuts Space/←/→/Home/End when the video tab is focused; thumbnail strip spacing fills the tab width; scene detection uses 16-bin-per-channel RGB histograms of a downscaled frame and the normalized L1 difference; grabbed documents are new, unsaved, 72 DPI, layer "Background".
- Workspace: branch `comet/video-import` off master.

# Open questions

- None. User confirmed outcome, scope, decisions, acceptance A1–A9, and non-goals on 2026-10-18.

# Verification expectations

- `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, existing release benchmarks, and the new release video benchmark pass.
- Unit tests: frame-number/timestamp mapping, exact seek, rotation, histogram difference and scene detection, every-Nth selection, FrameSet operations.
- Fixture tests: every container opens; grabbed frames match committed reference PNGs byte for byte; scene detection matches the fixture cut list.
- Headless UI tests: open video tab, transport controls and shortcuts, scrub (preview then exact), thumbnail click, Grab Frame, Extract Frames dialog and cancel, Frame Set panel open/remove/clear, error on a corrupt file.
