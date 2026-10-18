# Undo

## Purpose

Command-pattern undo/redo framework through which every document edit flows.

## Requirements

### Commands

- A `Command` has a user-visible `label`, `apply(&mut Document)`, `revert(&mut Document)`, a dirty region (the image rectangle it changes, or the whole document), and an approximate memory cost in bytes used for the history budget.
- `apply` followed by `revert` returns the document to bit-identical layer pixels and metadata; `revert` followed by `apply` reproduces the applied state.
- Commands that fail validation (for example an out-of-range layer index) return an error from `apply` and are not recorded.

### Undo stack

- Each open document has its own `UndoStack`.
- `AppState::execute(doc, command)` applies the command and pushes it; this clears any redo history. It is the only public way to modify a document; read access stays public.
- `undo` reverts the most recent applied command and moves it to redo history; `redo` re-applies the most recently undone command.
- `can_undo` / `can_redo` and `undo_label` / `redo_label` report availability and labels.
- History budget: steps are unlimited in count, but when the summed memory cost of recorded steps (undo + redo) exceeds 1 GB, the oldest undo steps are dropped until within budget; the most recent step is never dropped.
- Executing, undoing, and redoing mark the command's dirty region for the canvas and bump the document revision.

### Destructive edits respect the selection

- Every command that changes layer pixels applies its result through the shared masked-apply helper: with no selection the change applies everywhere; otherwise each pixel moves from its old value toward the new value in proportion to the mask (0 = unchanged, 255 = fully replaced), interpolating in premultiplied alpha.
- Such commands report the selection's bounding box (or the whole document without a selection) as their dirty region and store only that region's previous pixels for revert.

### Fill Layer command

- `FillLayer { layer, color }` fills the given layer with the RGBA color through the masked-apply helper; its label is "Fill Layer".

### Paint commands

- Each paint operation is exactly one undo step: a whole stroke from button press to release (labels "Brush Stroke", "Eraser Stroke", "Clone Stamp"), one bucket click ("Paint Bucket"), one gradient drag ("Gradient").
- Paint results are computed per pixel and applied through the masked-apply helper, so they respect the selection; they store the previous and new pixels of the affected region (stroke bounds ∩ editable bounds) and report that region as dirty.
- While a stroke is in progress the document shows it live; the step is recorded when the stroke ends. A stroke abandoned before release (tool or document switch) restores the previous pixels and records nothing.

### Selection commands

- Every selection change is a command: new selections from tools (labels "Rectangle Select", "Ellipse Select", "Lasso Select", "Polygon Select", "Magic Wand"), "Select All", "Deselect", "Invert Selection", "Grow Selection", "Shrink Selection", "Feather Selection".
- A selection command stores the previous and new masks, changes no layer pixels, and reports an empty dirty region (no tile refresh).

### Active layer

- Each open document has an active layer index, defaulting to the top layer when opened or created, always kept within range.
- The active layer can be set by index; edits such as Fill Layer target it.

### Layer commands

- Layer operations (new, duplicate, delete, reorder, merge down, flatten, rename, visibility, opacity, blend mode, mask add/delete/apply/enable) and transforms (move, free transform, flip) are each one undo step that restores the exact previous layer stack, pixels, masks, properties, active layer, and selection.
- Continuous property edits (dragging the opacity slider) and transform sessions preview live without recording history; the step is recorded when the edit ends, and an abandoned edit restores the previous state.

### Saved state

- Every recorded step has a unique id; the document is clean when the id of its most recent applied step equals the id recorded at the last save (or at open/creation). Undoing back to the saved step makes the document clean again.

### Filter commands

- Applying a filter is one undo step labelled with the filter's name. It stores the previous pixels of the affected region (editable bounds of the edit target) and restores them exactly on undo.
- While a filter dialog is open the canvas shows the preview without recording history; OK records the step, Cancel (or closing the dialog, switching document, or undo) restores the previous pixels.
