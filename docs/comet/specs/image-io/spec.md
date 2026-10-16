# Image I/O

## Purpose

Load raster image files into documents and export documents to raster files. No GUI dependency.

## Requirements

### Supported formats

- PNG (`.png`), JPEG (`.jpg`, `.jpeg`), BMP (`.bmp`), WebP (`.webp`); extension matching is case-insensitive.
- Any other extension fails with an "unsupported format" error.

### Loading

- `load_document(path)` decodes the file, converts it to RGBA8, and returns a `Document` with one layer named "Background" of the image's size, `name` set to the file name, and `source_path` set to the path.
- Animated files load their first frame only.
- DPI is read from PNG `pHYs` (when the unit is meters) and JPEG JFIF density (dots per inch or per cm, converted to DPI); otherwise it is 72.
- An unreadable, corrupt, or unsupported file returns an error and produces no document.

### Exporting

- `export_document(doc, path)` composites visible layers and encodes by the path's extension.
- PNG: RGBA8, lossless, includes `pHYs` with the document DPI. Exporting a document loaded from a PNG reproduces identical RGBA pixels.
- WebP: RGBA8, lossless encoding.
- JPEG: quality 95; pixels are composited over opaque white and written as RGB; JFIF density set to the document DPI.
- BMP: pixels are composited over opaque white and written as RGB.
- Unsupported extensions or write failures return an error.
