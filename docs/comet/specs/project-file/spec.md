# Project file

## Purpose

The native `.gac` project format, which stores a complete multi-layer document.

## Requirements

### Format

- A `.gac` file is a ZIP archive containing:
  - `manifest.json`: `{"format": "game-asset-creator", "version": 1, "width", "height", "dpi", "active_layer", "layers": [...], "selection": "selection.png" | null}`, with layers listed bottom-to-top as `{"name", "visible", "opacity", "blend_mode", "image", "mask", "mask_enabled"}` (`blend_mode` is one of `normal`, `multiply`, `screen`, `overlay`, `add`, `darken`, `lighten`; `mask` is a path or null).
  - `layers/<index>.png`: each layer's RGBA pixels (8-bit RGBA PNG).
  - `masks/<index>.png`: each mask (8-bit grayscale PNG), when present.
  - `selection.png`: the selection mask (8-bit grayscale PNG), when present.

### Saving and loading

- Saving writes the whole document atomically (to a temporary file in the same folder, then renamed over the target).
- Loading a file saved by the app reproduces the document exactly: size, DPI, layer order, names, visibility, opacity, blend modes, pixels, masks and their enabled state, the selection, and the active layer. Undo history is not stored.
- Loading fails with an error (and opens nothing) for files that are not ZIP archives, lack a valid manifest, declare another format or an unsupported version, reference missing entries, or contain images whose size does not match the document.
