# Filters

## Purpose

Color adjustments and image filters, registered through an internal filter-plugin API, previewed live on the canvas, and applied to the active layer (or its mask) through the undo stack and the selection.

## Requirements

### Plugin API

- A filter provides: a stable `id`, a display `name`, a `category` (Adjustments, Blur, Sharpen, Noise), a parameter schema, and a pure `apply(source, params) -> result` on an RGBA image region. The registry lists every filter; each filter lives in its own source file and adding one requires only that file plus one registry entry.
- Parameter kinds: integer (min, max, default), number (min, max, default, step), checkbox (default), choice (options, default), seed (integer, random default), and curve (control points, default identity). A parameter may belong to a group selected by a choice parameter (e.g. Levels' Channel), in which case the dialog shows only the selected group's parameters.
- The preview dialog is generated from the schema; parameter values are validated against it (out-of-range values are clamped).

### Applying

- Filters process the active layer's pixels, or its mask when the mask is the edit target (the mask is treated as opaque gray and written back as gray). Results go through the masked-apply rule, so unselected pixels are unchanged and partially selected pixels are interpolated.
- Neighbourhood filters read pixels outside the image as the nearest edge pixel and blur in premultiplied alpha. Color adjustments change only R, G, B (unpremultiplied); alpha is unchanged.
- Results are deterministic: the same input, parameters, and seed give identical output.

### Preview dialog

- Opening a filter shows a dialog with its parameters, a Preview checkbox (default on), OK, Cancel, and Reset (restore defaults). While open, the canvas shows the filtered result (or the original with Preview off) and updates after each parameter change.
- Updating the preview of any filter on a 1920×1080 layer takes under 100 ms in a release build at default parameters and at the large settings listed below.
- OK records one undo step named after the filter; Cancel restores the layer.

### Adjustments (colors in [0, 1], applied per channel)

- Brightness/Contrast: Brightness −100…100 (default 0), Contrast −100…100 (default 0). `f = 1 + c/100` for `c < 0`, `f = 1 / (1 − 0.99·c/100)` for `c ≥ 0`; `v' = (v − 0.5)·f + 0.5 + b/200`.
- Levels: Channel (RGB, Red, Green, Blue) groups Input Black (0–253, default 0), Input White (2–255, default 255, kept above black), Gamma (0.10–9.99, default 1.00), Output Black (0–255, default 0), Output White (0–255, default 255). `levels(v) = ob + (ow − ob)·clamp((v − ib)/(iw − ib), 0, 1)^(1/γ)`. Each channel applies its own levels, then the RGB levels.
- Curves: Channel (RGB, Red, Green, Blue) groups one curve each, edited by dragging control points (click to add, drag off the graph to remove; endpoints stay). Points are interpolated with a monotone cubic (Fritsch–Carlson) spline through the sorted points; default is the identity line. Each channel applies its curve, then the RGB curve.
- Hue/Saturation/Lightness: Hue −180…180°, Saturation −100…100, Lightness −100…100 (defaults 0), in HSL: `h' = h + hue`, `s' = clamp(s·(1 + sat/100))`, `l' = l + (light/100)·(light > 0 ? 1 − l : l)`.
- Color Balance: Tone (Shadows, Midtones, Highlights) groups Cyan–Red, Magenta–Green, Yellow–Blue sliders (−100…100, default 0); Preserve Luminosity (default on). With luminance `L = 0.299R + 0.587G + 0.114B`, tone weights `ws = max(0, 1 − 2L)`, `wm = 1 − |2L − 1|`, `wh = max(0, 2L − 1)`, each channel shifts by `Σ w_tone · slider/100 · 0.5`. Preserve Luminosity then adds the same amount to all channels so `L` is unchanged.
- Desaturate: `R = G = B = L`.
- Invert: `v' = 1 − v`.
- Posterize: Levels 2–255 (default 4): `v' = round(v·(n − 1))/(n − 1)`.

### Filters

- Gaussian Blur: Radius 0.1–250 px (default 2.0) used as σ. σ < 2 uses an exact separable Gaussian kernel truncated at 3σ; σ ≥ 2 uses three successive box blurs whose widths approximate the Gaussian. Large setting: radius 50.
- Motion Blur: Angle −180…180° (default 0), Distance 1–200 px (default 10): each pixel averages samples taken every pixel along a line of that length through it, at that angle (bilinear). Large setting: distance 100.
- Sharpen: fixed 3×3 kernel `[0 −1 0; −1 5 −1; 0 −1 0]` on RGB.
- Unsharp Mask: Amount 1–500 % (default 100), Radius 0.1–250 px (default 1.0), Threshold 0–255 (default 0): `v' = v + amount·(v − gaussian(v, radius))` where `|v − gaussian| ≥ threshold/255`, else `v`. Large setting: radius 50.
- Add Noise: Amount 0–100 % (default 10), Distribution (Uniform, Gaussian; default Uniform), Monochromatic (default off), Seed (random when the dialog opens, editable). Each channel (or all three equally when monochromatic) gets an offset of `amount·r`, where `r` is uniform in [−0.5, 0.5] or Gaussian with σ = 0.25, from a counter-based generator keyed by seed and pixel position.
- Median: Radius 1–20 px (default 1): each channel (and alpha) becomes the median of its `(2r+1)²` neighbourhood. Large setting: radius 10.

### Regression tests

- Every filter has a golden-image test: a fixed synthetic RGBA input processed with fixed parameters must match a committed PNG in `tests/golden/` byte for byte. Setting `UPDATE_GOLDEN=1` rewrites the golden files.
