# Video

## Purpose

Open video files, browse their frames, grab single frames as image documents, and batch-extract frames into an in-session Frame Set for the asset-extraction pipeline.

## Requirements

### Decoding

- Videos are decoded through the linked ffmpeg libraries (`ffmpeg-next`). Supported containers: .mp4, .mkv, .avi, .webm, .mov (any video codec the linked ffmpeg decodes). Only the best video stream is used; audio is ignored.
- Frames are converted to 8-bit RGBA. For RGB sources the conversion is lossless; for YUV sources it matches `ffmpeg -i <file> -pix_fmt rgba` byte for byte.
- Display rotation metadata (90°, 180°, 270°) is applied, so frames display, grab, and extract upright; reported dimensions are the display dimensions.
- Frame numbers count from 0 in presentation order. Frame N is the frame shown at time N / r, where r is the stream's average frame rate; the frame count is the stream's frame count or duration × r.
- All decoding runs on worker threads. A newer request replaces pending older ones (latest seek wins); the UI thread never blocks on decoding.

### Opening

- File → Open accepts the video extensions alongside images and projects ("Videos" filter). Opening a video adds a center tab titled with the file name and shows frame 0. If the file cannot be opened or has no decodable video stream, an error message is shown and no tab is added. Closing the tab stops its workers and releases the decoder.

### Video tab

- Layout: the frame, fitted to the available area with its aspect ratio preserved; an info line (width × height, frame rate, duration, `frame N / total`, timecode `hh:mm:ss.mmm`); the thumbnail strip; the timeline; the transport buttons (Start, Step Back, Play/Pause, Step Forward, End) and actions (Grab Frame, Extract Frames…).
- Timeline: a slider over frame numbers. While dragging, the view shows the nearest keyframe at or before the pointer position (fast preview); on release the exact frame at the released position is decoded and shown. The counter and timecode always describe the frame being displayed.
- Playback: Play shows frames at the clip's frame rate, skipping frames when decoding falls behind, and stops (paused) on the last frame. Pausing shows the exact current frame.
- Frame-step: Step Back/Forward move exactly one frame (pausing playback); Start/End go to frame 0 / the last frame.
- Shortcuts while a video tab is focused: Space play/pause, ← / → step, Home / End start/end.
- Thumbnail strip: thumbnails at evenly spaced frames across the clip (as many as fit the width), decoded in the background and filled in as they arrive; the thumbnail containing the current position is highlighted; clicking a thumbnail seeks exactly to its frame.

### Grab Frame

- Grab Frame opens a new image document of the video's display size with one layer "Background" holding the exact current frame (the exact frame is decoded first if a keyframe preview is showing), named `<video stem> frame <N>`, 72 DPI, unsaved. Existing documents are unchanged.

### Batch extraction

- Extract Frames… opens a dialog: Mode (Every Nth frame, Scene changes), From/To frame (default the whole clip), and the mode's parameters. It runs in the background with a progress bar and Cancel; on completion the frames are appended to the Frame Set in frame order; Cancel adds nothing.
- Every Nth frame: N 1–100000 (default 30); frames From, From+N, … ≤ To.
- Scene changes: Threshold 1–100 % (default 30), Minimum scene length 1–1000 frames (default 10). Each frame is downscaled to at most 64 px on its longer side, a 16-bin histogram per R, G, B channel is built and normalized, and the difference to the previous frame is `d = (1/6) Σ |h − h_prev|` over the 48 bins (0 … 1). A cut is placed at frame i when `d ≥ threshold` and i is at least the minimum scene length after the previous cut (or From). The result is From plus every cut frame.

### Frame Set

- The Frame Set is an in-session list of frames: source video path and name, frame number, timestamp, and RGBA image. It is not saved.
- The Frame Set panel (dockable, listed with the other panels) shows a grid of thumbnails labelled `<video name> #N` and the timecode, with the frame count. Clicking selects (Cmd/Shift extend); double-click or Open opens the frame as a new image document (as Grab Frame); Remove deletes the selected frames; Clear empties the set.
- Later changes read frames through the `FrameSet` API.

### Performance

- On a 1920×1080 H.264 30 fps clip with a keyframe every 60 frames, in a release build: keyframe preview < 60 ms per position, exact frame at any position < 500 ms, sequential decoding < 33 ms per frame.
