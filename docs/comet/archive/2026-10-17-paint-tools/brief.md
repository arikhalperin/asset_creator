# Outcome

Game Asset Creator gains painting tools — Brush, Eraser, Eyedropper, Paint Bucket, Gradient, Clone Stamp — plus foreground/background color swatches with a color picker. Strokes are smooth and anti-aliased with no gaps at fast pointer speeds; every tool edits the active layer through the undo framework (one step per stroke, click, or drag) and is clipped to the active selection; each tool's options appear in the tool-options strip.

# Scope

- Pure, unit-tested paint core: dab coverage (anti-aliased with hardness, Pixel mode), Catmull-Rom stroke interpolation and dab spacing, per-stroke coverage accumulation, brush/eraser/clone pixel rules, bucket fill, linear/radial gradients.
- Paint commands on the undo stack with live stroke preview and abandon-on-switch.
- Tools panel: six paint tools, color swatches (picker, swap, reset), shortcuts B/E/I/G/S/X/D/[ ].
- Tool-options strip entries for every paint tool; brush cursor circle; clone source marker; gradient preview line.

# Non-goals

- Pressure/tilt input, brush textures/shapes other than round, scattering, blend modes for paint (C5 adds layer blend modes).
- Pencil/smudge/blur/dodge tools, pattern fills, multi-stop or saved gradients, color history/palettes.
- Changing the Edit → Fill Layer dialog (it keeps its own remembered color).
- Sample-all-layers for the paint bucket or clone stamp (both use the active layer).

# Acceptance examples

- A1: The Tools panel adds Brush (B), Eraser (E), Eyedropper (I), Paint Bucket and Gradient (G toggles between them), and Clone Stamp (S); only Move remains as a disabled placeholder; shortcuts are ignored while typing or with Cmd/Ctrl held.
- A2: Foreground/background swatches (default opaque black / opaque white) appear in the Tools panel; clicking one opens an RGBA color picker for it; the swap button or X exchanges them; the reset button or D restores black/white.
- A3: Brush paints the foreground color on the active layer with Size (1–500, default 20), Hardness (0–100 %, default 100 %), and Opacity (1–100 %, default 100 %). Strokes are anti-aliased: edge pixels get partial coverage even at 100 % hardness, and lower hardness widens a smooth falloff. Verified by unit tests of dab coverage.
- A4: Stroke interpolation: dabs are spaced at most 15 % of the size (minimum 0.5 px) along a Catmull-Rom curve through the pointer samples, so a stroke whose samples are far apart (e.g. 200 px between frames) leaves a continuous band with no gaps; verified by a unit test and a canvas test with sparse pointer events.
- A5: Opacity is capped per stroke: where a stroke overlaps itself the result never exceeds the stroke opacity (e.g. a 50 % black stroke over white stays ~50 % gray even where it crosses), while a second stroke builds on top. Verified by tests.
- A6: Pixel mode (Brush and Eraser, default off) produces only fully covered or untouched pixels; a 1-pixel brush draws a connected single-pixel line. Verified by tests.
- A7: Eraser reduces alpha toward transparent by coverage × opacity using its own Size/Hardness/Opacity/Pixel options.
- A8: Shift+click with Brush, Eraser, or Clone Stamp paints a straight stroke from the previous stroke's end point to the click.
- A9: Eyedropper: click sets the foreground color, Option/Alt+click the background color, sampling the visible composite by default or the active layer when Sample is "Active layer"; it creates no undo step.
- A10: Paint Bucket fills the clicked color region of the active layer (Tolerance 0–255, default 32; Contiguous default on) with the foreground color at the tool opacity.
- A11: Gradient: dragging paints Linear (default) or Radial gradients from start to end, Foreground→Background (default) or Foreground→Transparent, composited over the active layer at the tool opacity; the start pixel shows the foreground color and pixels at/after the end the end color; a start-to-end line is previewed while dragging.
- A12: Clone Stamp: Option/Alt+click sets the source; strokes copy from the active layer as it was when the stroke began, offset by (source − first stroke point); Aligned (default on) keeps the offset across strokes, off restarts at the source each stroke; painting without a source shows a message and changes nothing.
- A13: Every paint tool respects the selection mask: unselected pixels are unchanged and partially selected pixels change proportionally. Verified per tool by tests.
- A14: Each stroke, bucket click, and gradient drag is exactly one undo step ("Brush Stroke", "Eraser Stroke", "Clone Stamp", "Paint Bucket", "Gradient"); undo/redo restore exact pixels; only tiles touched by the edit are refreshed; a stroke abandoned by switching tool or document mid-stroke restores the pixels and records nothing.
- A15: The tool-options strip shows each paint tool's options: Brush/Eraser Size, Hardness, Opacity, Pixel; Clone Stamp Size, Hardness, Opacity, Aligned; Paint Bucket Tolerance, Contiguous, Opacity; Gradient Type, Colors, Opacity; Eyedropper Sample. Each tool keeps its own values.
- A16: Strokes appear live on the canvas while painting; Brush/Eraser/Clone Stamp show a brush-size circle cursor; Clone Stamp shows its source marker; [ and ] change the current brush size; Space/middle drag and wheel/pinch still navigate with paint tools active.

# Constraints and invariants

- All pixel changes are computed per pixel and applied through the C3 masked-apply rule; paint never changes the selection.
- Paint core functions are pure and window-independent.
- Undo/redo of every paint command restores exact pixels.

# Decisions

- Pixel mode: Brush and Eraser offer a hard-edged Pixel checkbox, default off (anti-aliased by default).
- Eyedropper samples the visible composite by default, with an "Active layer" option; click = foreground, Option/Alt+click = background.
- Opacity is capped per stroke (Photoshop behaviour); separate strokes build up.
- Gradient Colors option: Foreground→Background (default) or Foreground→Transparent.
- Conventions adopted without asking (Photoshop-like): shortcuts B/E/I/G/S/X/D/[ ], Shift+click straight lines, Aligned clone stamp by default, bucket uses the active layer with Tolerance 32 / Contiguous on, per-tool option values.
- Workspace: branch `comet/paint-tools` off master.

# Open questions

- None. User confirmed outcome, scope, decisions, acceptance A1–A16, and non-goals on 2026-10-17.

# Verification expectations

- `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, and the release pan/zoom benchmark pass.
- Unit tests: dab coverage (hardness, anti-aliasing, Pixel mode), spacing and Catmull-Rom interpolation, gap-free sparse strokes, per-stroke opacity cap, eraser, bucket, linear/radial gradients and both color modes, clone offset and aligned/non-aligned, selection clipping for every tool, undo exactness.
- Headless canvas tests: brush stroke with sparse fast pointer events, Shift+click line, eyedropper FG/BG, bucket click, gradient drag, clone stamp with Option+click source, abandon-on-switch, shortcuts, live preview tile refresh, undo labels.
- Manual launch to paint and see strokes, cursor, and options strip.
