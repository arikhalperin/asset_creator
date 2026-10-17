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

### Fill Layer command

- `FillLayer { layer, color }` sets every pixel of the given layer to the RGBA color; it stores the layer's previous pixels for revert; its dirty region is the whole document; its label is "Fill Layer".

### Active layer

- Each open document has an active layer index, defaulting to the top layer when opened or created, always kept within range.
- The active layer can be set by index; edits such as Fill Layer target it.
