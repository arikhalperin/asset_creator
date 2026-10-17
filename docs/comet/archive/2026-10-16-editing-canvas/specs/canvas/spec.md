# Canvas

## Purpose

The editing canvas shown in each document tab: an egui widget that displays the document composite with zoom, pan, transparency backdrop, and pixel grid, refreshing only regions that changed.

## Requirements

### View transform

- Each document tab has its own view state: `zoom` (image pixels → physical screen pixels scale) and `pan` (screen position of the image origin).
- Zoom is clamped to 0.01 (1%) through 64.0 (6400%).
- 100% (Actual Size) means one image pixel per physical screen pixel.
- Fit to Window chooses the largest zoom (within the clamp) at which the whole image fits in the view, and centers the image.
- On first display, a document is shown at Fit if it is larger than the view in either dimension, otherwise at 100%, centered.

### Navigation

- Ctrl/Cmd+mouse wheel and trackpad pinch zoom continuously by the input's zoom factor, keeping the image point under the pointer fixed on screen.
- Plain mouse wheel / two-finger scroll pans the view by the scroll delta.
- Holding Space while left-dragging pans the view; the pointer shows a grab cursor while Space is held over the canvas and a grabbing cursor while dragging. Middle-button drag also pans.
- Zoom In / Zoom Out (Cmd/Ctrl+= and Cmd/Ctrl+-, View menu) multiply / divide zoom by √2, anchored at the view center.
- Fit to Window (Cmd/Ctrl+0) and Actual Size (Cmd/Ctrl+1) as above, also in the View menu.
- Navigation applies to the current document's canvas.

### Rendering

- Behind the image bounds, a checkerboard of 8×8-point squares in light and dark gray is drawn in screen space (it does not scale with zoom); outside the image bounds the canvas shows its plain background.
- The composite is drawn on top of the checkerboard.
- Sampling: nearest-neighbor when magnified; linear with mipmaps when minified.
- When zoom is strictly greater than 8.0 and Show Pixel Grid is enabled (default on, View menu toggle), 1-physical-pixel lines are drawn on every pixel boundary within the visible part of the image, in a semi-transparent color that contrasts with typical content.
- Only tiles intersecting the visible area are drawn.

### Tile cache and dirty regions

- The composite is cached as square tiles (256×256 image pixels; edge tiles smaller), each uploaded as its own texture.
- Building the cache the first time uploads every tile once.
- When a command reports a dirty rectangle, only tiles intersecting that rectangle are recomposited and re-uploaded on the next frame; other tiles are untouched.
- Pan and zoom never recomposite or re-upload tiles.
- The cache exposes counters (tiles uploaded) so tests can observe what was refreshed.

### Status

- The status bar shows the current document's zoom as a percentage, and while the pointer is over the image, the image pixel coordinate under it (`x, y`).
