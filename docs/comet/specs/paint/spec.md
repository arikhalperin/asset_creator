# Paint

## Purpose

Painting tools that edit the active layer: Brush, Eraser, Eyedropper, Paint Bucket, Gradient, and Clone Stamp, with foreground/background colors. All edits respect the selection and are undoable.

## Requirements

### Edit target

- Paint tools edit the active layer's pixels, or its mask when the mask is the edit target (see the layers spec). On a mask, colors are converted to gray using luminance `0.299 R + 0.587 G + 0.114 B` (alpha kept as blending strength): Brush, Paint Bucket, and Gradient write gray values; the Eraser paints white (reveals); the Clone Stamp copies mask values; the eyedropper still samples colors. Mask edits respect the selection and are undoable like pixel edits.

### Colors

- The application has a foreground and a background color (RGBA, default opaque black / opaque white), shared by all documents. Changing them is not a document edit and is not undoable.

### Brush dabs

- A brush has Size (diameter, 1–500 image pixels, default 20), Hardness (0–100 %, default 100 %), and Opacity (1–100 %, default 100 %). Brush, Eraser, and Clone Stamp each keep their own values.
- Anti-aliased dab coverage for a pixel whose center is at distance `d` from the dab center, with radius `r = size / 2` and hardness `h`: `w = max(r·(1 − h), 1)`, `c = clamp((r − d) / w, 0, 1)`, smoothed with smoothstep when `h < 1`. At 100 % hardness the edge is a 1-pixel anti-aliased ramp.
- Pixel mode (Brush and Eraser option, default off): dab centers snap to pixel centers and coverage is 1 where `d ≤ r` and 0 elsewhere (no partial values). A 1-pixel brush covers exactly one pixel per dab.

### Strokes

- Pressing the primary button on the canvas starts a stroke; pointer samples are joined by a Catmull-Rom curve, and dabs are placed along it every `max(0.15 × size, 0.5)` pixels (every 0.5 pixel in Pixel mode), so fast pointer movement leaves no gaps. Releasing ends the stroke.
- Within one stroke, coverage accumulates by maximum, then is scaled by Opacity: overlapping parts of a stroke never exceed the stroke's opacity. A later stroke builds on the result.
- Shift+click with Brush, Eraser, or Clone Stamp paints a straight stroke from the end of the previous stroke on the same document to the clicked point.
- Brush: each pixel moves toward the foreground color by `coverage × opacity` (premultiplied interpolation).
- Eraser: each pixel's alpha is multiplied by `1 − coverage × opacity`; color channels are kept.

### Eyedropper

- Clicking (or dragging) over the image sets the foreground color; with Option/Alt held it sets the background color. Sample: All layers (default) reads the visible composite, Active layer reads the active layer. Clicks outside the image do nothing.

### Paint Bucket

- Clicking a pixel fills the region of the active layer that the magic wand would select for that pixel (Tolerance 0–255, default 32; Contiguous, default on) with the foreground color at the tool's Opacity (default 100 %), moving each pixel toward the foreground color by the opacity. Clicks outside the image do nothing.

### Gradient

- Dragging from a start point to an end point paints a gradient over the active layer. Type: Linear (default; position = projection onto the start→end line, clamped to [0, 1]) or Radial (position = distance from start ÷ start-to-end distance, clamped). Colors: Foreground to Background (default) or Foreground to Transparent (the foreground color fading to alpha 0). Colors interpolate in premultiplied RGBA.
- The gradient color, with its alpha scaled by Opacity (default 100 %), is composited source-over onto each pixel. A drag shorter than 1 pixel does nothing.

### Clone Stamp

- Option/Alt+click sets the source point on the current document. A stroke copies pixels from the active layer as it was when the stroke began, at an offset of (source − first point of the first stroke after setting the source). Aligned (default on) keeps that offset for later strokes; with Aligned off, every stroke restarts at the source point.
- Each pixel moves toward its source pixel by `coverage × opacity`; pixels whose source lies outside the image are unchanged. Painting without a source shows a status message and changes nothing.
