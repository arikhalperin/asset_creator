# Layers

## Purpose

Managing a document's layer stack: creating, ordering, merging, and flattening layers; editing their visibility, opacity, blend mode, and name; and adding and editing layer masks.

## Requirements

### Layers panel

- Rows list layers top-first. Each row has a visibility toggle (click toggles visibility), the layer name (click selects the layer and its pixels as the edit target; double-click renames inline, Enter commits, Escape cancels, empty names are rejected), and, when the layer has a mask, a "Mask" chip (click selects the mask as the edit target; a disabled mask is shown struck through).
- The active row is highlighted, and the current edit target (pixels or mask) is highlighted within it.
- Above the rows, the active layer's blend mode (Normal, Multiply, Screen, Overlay, Add, Darken, Lighten) and opacity (0–100 %) are editable; opacity changes preview live while dragging and are recorded as one step when the drag ends.
- Below the rows: New, Duplicate, Delete, Move Up, Move Down, Merge Down, and Flatten buttons, enabled like the matching Layer menu items.

### Layer operations

- New Layer: a transparent layer named "Layer N" (N = the smallest positive number not used by an existing layer named "Layer N") above the active layer; it becomes active.
- Duplicate Layer: an exact copy (pixels, mask, properties) named "<name> copy" above the active layer; it becomes active.
- Delete Layer: removes the active layer (disabled when only one layer remains); the layer below (or the new bottom) becomes active.
- Move Layer Up / Down: swaps the active layer with its neighbour; it stays active. Disabled at the top / bottom.
- Merge Down: composites the active layer (with its blend mode, opacity, and enabled mask; a hidden layer contributes nothing) onto the layer below, whose own enabled mask is applied first; the lower layer keeps its name, visibility, opacity, and blend mode, loses its mask, and becomes active; the upper layer is removed. Disabled on the bottom layer.
- Flatten Image: replaces all layers with one Normal, 100 %, visible, mask-less layer named "Background" holding the document composite (transparency is kept).
- Changing visibility, opacity, blend mode, or name is one undo step per change.

### Layer masks

- Add Layer Mask: gives the active layer a fully white (reveal-all) mask and makes the mask the edit target. Add Mask from Selection: the mask is the current selection mask (disabled without a selection). Both are disabled when the layer already has a mask.
- Delete Mask removes the mask; Apply Mask multiplies the layer's alpha by the mask and removes it; Disable/Enable Mask toggles whether the mask affects compositing (a disabled mask is kept but ignored).
- When the mask is removed, deleted, or applied, the edit target returns to pixels.
- Switching the active layer selects its pixels as the edit target. The status bar shows "Editing mask" while the mask is the target.
