# Document model

## Purpose

In-memory representation of an image being edited: a document holding metadata and an ordered stack of RGBA8 layers. No GUI dependency.

## Requirements

### Document

- A `Document` has `name` (display name), optional `source_path`, `width` and `height` in pixels (both > 0), `dpi` (> 0, default 72.0), and an ordered list of layers.
- Creating a document with zero width/height or non-positive/non-finite DPI fails with an error.
- `Document::new_blank(name, width, height, dpi)` creates a document with one fully transparent layer named "Background".
- Layer index 0 is the bottom of the stack; the last index is the top.

### Layer

- A `Layer` has `name`, `visible` (default true), `opacity` in [0.0, 1.0] (default 1.0, clamped on set), and a `pixels: image::RgbaImage` buffer.
- Adding or inserting a layer whose buffer size differs from the document size fails with an error and leaves the stack unchanged.

### Layer stack operations

- `add_layer` appends on top; `insert_layer(index, layer)` inserts at index (index ≤ len).
- `remove_layer(index)` removes and returns the layer; an out-of-range index fails.
- `move_layer(from, to)` reorders, preserving the relative order of other layers; out-of-range indices fail.
- Read access to layers by index and in order.

### Compositing

- `composite()` returns an `RgbaImage` of the document size: visible layers blended bottom-to-top with source-over (normal) alpha compositing, each layer's alpha scaled by its opacity, starting from fully transparent.
- Hidden layers and zero-opacity layers do not contribute.
- A single visible, opacity-1.0 layer composites to exactly its own pixels.

### Selection mask

- A document has an optional selection mask: an 8-bit single-channel image of the document's size (0 = unselected, 255 = fully selected, values between are partially selected).
- No mask means nothing is selected and edits apply to the whole document. Setting a mask whose size differs from the document fails with an error; setting a mask with no non-zero pixel stores no mask.
- The selection is metadata: it is not composited or exported.
