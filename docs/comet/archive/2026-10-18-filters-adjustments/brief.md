# Outcome

Game Asset Creator gains a Filter menu with 14 filters — Brightness/Contrast, Levels, Curves, Hue/Saturation/Lightness, Color Balance, Desaturate, Invert, Posterize, Gaussian Blur, Motion Blur, Sharpen, Unsharp Mask, Add Noise, Median — registered through an internal filter-plugin API (one file per filter), previewed live on the canvas, and applied to the active layer (or its mask) as one undo step clipped to the selection.

# Scope

- Filter-plugin API (id, name, category, parameter schema, pure apply) and registry; one source file per filter.
- The 14 filters with the formulas in the filters spec; deterministic results.
- Schema-generated preview dialog (sliders, checkboxes, choices, seed, grouped parameters, curve editor) with live on-canvas preview, Preview checkbox, Reset, OK/Cancel.
- Filter menu grouped by category; parameterless filters apply immediately.
- Golden-image regression tests and a release preview benchmark.

# Non-goals

- Adjustment layers or re-editable filters, filter presets, Repeat Last Filter, filter masks/smart filters.
- GPU acceleration; previewing on a reduced-resolution proxy.
- Third-party/dynamically loaded plugins (the API is internal).
- Histogram display in Levels/Curves.

# Acceptance examples

- A1: Every filter is defined through the plugin API (id, name, category, parameter schema, apply) in its own file and registered in one registry; the Filter menu and dialogs are generated from the registry and schemas, so adding a filter needs only its file and a registry line.
- A2: The Filter menu lists all 14 filters grouped Adjustments / Blur / Sharpen / Noise; filters with parameters open a preview dialog, Desaturate, Invert, and Sharpen apply immediately; items are disabled without a document or during a transform.
- A3: A preview dialog shows the filter's parameters (sliders, checkboxes, choices, seed, curve editor; grouped parameters follow their group selector), a Preview checkbox (default on), Reset, OK, and Cancel; the canvas shows the filtered result live after each change (the original when Preview is off); OK records one undo step named after the filter; Cancel restores the layer exactly.
- A4: Filters apply to the active layer, or its mask when the mask is the edit target, through the selection: unselected pixels are unchanged and partially selected ones interpolated; undo/redo restore exactly.
- A5: Color adjustments follow the spec formulas: Brightness/Contrast, Levels (RGB + per-channel), Curves (RGB + per-channel monotone cubic), Hue/Saturation/Lightness, Color Balance (tone ranges, preserve luminosity), Desaturate, Invert, Posterize — verified by unit tests on known values (e.g. Invert twice is identity, Posterize 2 yields only 0/255, identity Levels/Curves change nothing).
- A6: Gaussian Blur, Motion Blur, Sharpen, Unsharp Mask, Add Noise (seeded, uniform/gaussian, monochromatic), and Median follow the spec, blur in premultiplied alpha with edge clamping, and are deterministic — verified by unit tests (e.g. blur of a flat image is unchanged, median removes isolated specks, same seed gives same noise and a different seed different noise).
- A7: Previews update in under 100 ms on a 1920×1080 layer in a release build for every filter at default parameters and at the large settings in the spec (blur radius 50, motion distance 100, unsharp radius 50, median radius 10), measured by a release benchmark.
- A8: Every filter has a golden-image regression test comparing a fixed synthetic input and fixed parameters byte-for-byte with a committed PNG in `tests/golden/` (regenerated with `UPDATE_GOLDEN=1`).

# Constraints and invariants

- Filter `apply` functions are pure and window-independent.
- Filters never change the selection, layer order, or properties.

# Decisions

- Live preview is shown on the canvas (Photoshop-style) with a Preview checkbox; OK records one step, Cancel restores.
- Levels and Curves have RGB plus per-channel control (Channel selector), all in one step.
- Add Noise has an editable Seed (random per dialog open) so results are reproducible.
- Conventions adopted without asking: Filter menu grouped by category; parameterless filters apply immediately; filters follow the pixels/mask edit target; Gaussian radius is σ with a three-box approximation for σ ≥ 2; formulas as in the filters spec.
- One change: all filters share the plugin API, dialog, and preview machinery.
- Workspace: branch `comet/filters-adjustments` off master.

# Open questions

- None. User confirmed outcome, scope, decisions, acceptance A1–A8, and non-goals on 2026-10-18.

# Verification expectations

- `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, existing release benchmarks, and the new release filter-preview benchmark pass.
- Unit tests per filter for formulas and properties; golden-image tests for all 14; plugin registry tests (unique ids, schema defaults valid).
- Headless UI tests: Filter menu enablement, dialog open → parameter change → canvas preview → OK one step / Cancel restore, Preview toggle, Reset, grouped parameters, immediate filters, mask target, selection clipping.
