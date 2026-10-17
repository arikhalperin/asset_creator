# Transform

## Purpose

Moving, scaling, rotating, and flipping layer content interactively.

## Requirements

### Content

- A transform acts on the active layer's content: with a selection, the selected pixels (weighted by the mask), leaving the unselected remainder in place; without one, the whole layer. The layer mask (if any) is transformed together with the pixels, and the selection mask is transformed together with the content.
- Pixels that end up outside the canvas are clipped when the transform is committed; while a transform is still in progress, moving content back recovers them.

### Move tool

- The Move tool (V) drags the content by the pointer movement (in image pixels), previewing live; releasing commits one "Move" step. With the Move tool active, the arrow keys nudge by 1 pixel (10 with Shift), each nudge one step.

### Free Transform

- Free Transform (Ctrl+T) starts a session around the content's bounding box (the selection bounds, or the layer's non-transparent bounds; the whole canvas if the layer is empty).
- Dragging inside the box moves it; dragging a corner or edge handle scales from the opposite corner/edge (Shift on a corner keeps proportions); dragging just outside a corner rotates around the box center (Shift snaps to 15° steps).
- The tool-options strip shows Interpolation (Bilinear, default; or Nearest), Commit, and Cancel. Enter commits and Escape cancels. Committing records one "Free Transform" step; cancelling restores the content. Switching tool or document cancels the session.
- The preview updates live: each pointer move re-renders only the region covered by the content's old and new transformed bounds.

### Resampling

- Each output pixel center is mapped back through the inverse transform. Nearest takes the source pixel containing that point; Bilinear interpolates the four nearest source pixels in premultiplied alpha. Points outside the content are transparent.
- An identity transform, a translation by whole pixels, and (with Nearest) rotations by multiples of 90° and integer scale factors reproduce the source pixels exactly.

### Flip

- Flip Horizontal / Flip Vertical mirror the content about the center of its bounding box (selection bounds, or the canvas without a selection) as one step; flipping twice restores the original exactly.
