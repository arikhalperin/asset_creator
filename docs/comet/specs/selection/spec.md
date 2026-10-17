# Selection

## Purpose

Selection tools, mask operations, and combine modes that produce the document's 8-bit selection mask.

## Requirements

### Combine modes

- A new selection shape `b` combines with the existing mask `a` (missing mask = all 0) per pixel:
  - Replace: `b`.
  - Add: `max(a, b)`.
  - Subtract: `round(a × (255 − b) / 255)`.
  - Intersect: `min(a, b)`.
- The mode comes from the modifiers held when a gesture starts: Shift = Add, Option/Alt = Subtract, Shift+Option/Alt = Intersect; with no modifier, the mode selected in the tool-options strip applies (Replace by default).
- A result with no non-zero pixel clears the selection.

### Rectangle Select

- Dragging from one corner to the opposite corner selects (255) every pixel whose center lies inside the dragged rectangle, clipped to the document; edges are hard.
- A click without dragging in Replace mode clears the selection; in other modes it changes nothing.

### Ellipse Select

- Dragging defines a bounding rectangle; the selection is the inscribed axis-aligned ellipse.
- Anti-alias on: each pixel's value is the fraction of its area inside the ellipse, estimated with 4×4 supersampling (0–255). Anti-alias off: 255 where the pixel center is inside, else 0.

### Lasso (freehand)

- Dragging records the pointer path in image coordinates; releasing closes the path back to its start and selects the enclosed area using the even-odd rule.
- Anti-alias behaves as for Ellipse Select (4×4 supersampled coverage vs. pixel-center test). Paths with fewer than 3 distinct points select nothing (a Replace-mode click clears the selection).

### Polygon Lasso

- Each click adds a vertex. The polygon closes and is applied when the user clicks within 6 points of the first vertex, double-clicks, or presses Enter; it needs at least 3 vertices, otherwise closing cancels it.
- Backspace removes the last vertex; Escape cancels the polygon. Switching tools or documents cancels it.
- The enclosed area is filled like the Lasso (even-odd rule, same anti-alias option). Combine-mode modifiers are read at the first click.

### Magic Wand

- Clicking a pixel selects pixels of the active layer whose RGBA color is within Tolerance of the clicked pixel's color, where distance is the largest absolute difference over the R, G, B, and A channels.
- Contiguous on: only pixels connected to the clicked pixel through 4-neighbors that also match. Contiguous off: every matching pixel in the layer.
- Edges are hard (0/255). Clicks outside the image do nothing.

### Select All / None / Invert

- All selects every pixel (255). None clears the selection. Invert replaces every value `v` with `255 − v` (with no selection, Invert selects everything).

### Grow / Shrink

- Grow by radius r: each pixel takes the maximum mask value within Euclidean distance r (a disk).
- Shrink by radius r: each pixel takes the minimum mask value within the disk, considering only pixels inside the document (the canvas edge does not erode the selection).

### Feather

- Feather by radius r blurs the mask with a separable Gaussian of σ = r / 2 (kernel truncated at 3σ, edges clamped), producing a soft edge: across a straight hard edge, values ramp monotonically from 255 inside to 0 outside, staying 255 deeper than 2r inside and 0 farther than 2r outside.
