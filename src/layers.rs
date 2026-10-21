//! Layer stack operations as undoable commands: new, duplicate, delete,
//! reorder, merge down, flatten, property changes, and layer masks.

use image::{GrayImage, Luma, RgbaImage};

use crate::document::{composite_layer_onto, BlendMode, Document, Layer, PixelRect};
use crate::undo::{Command, CommandError};

/// Editable properties of a layer (everything except pixels and mask).
#[derive(Debug, Clone, PartialEq)]
pub struct LayerProps {
    pub name: String,
    pub visible: bool,
    pub opacity: f32,
    pub blend_mode: BlendMode,
    pub mask_enabled: bool,
}

impl LayerProps {
    pub fn of(layer: &Layer) -> Self {
        Self {
            name: layer.name.clone(),
            visible: layer.visible,
            opacity: layer.opacity(),
            blend_mode: layer.blend_mode,
            mask_enabled: layer.mask_enabled,
        }
    }

    pub fn apply_to(&self, layer: &mut Layer) {
        layer.name = self.name.clone();
        layer.visible = self.visible;
        layer.set_opacity(self.opacity);
        layer.blend_mode = self.blend_mode;
        layer.mask_enabled = self.mask_enabled;
    }
}

fn invalid(message: &str) -> CommandError {
    CommandError::Invalid(message.to_owned())
}

fn no_layer(doc: &Document, index: usize) -> CommandError {
    CommandError::NoSuchLayer {
        index,
        len: doc.layer_count(),
    }
}

/// The name for a new layer: "Layer N" with the smallest unused N.
pub fn next_layer_name(doc: &Document) -> String {
    (1..)
        .map(|n| format!("Layer {n}"))
        .find(|name| doc.layers().iter().all(|l| &l.name != name))
        .expect("unbounded range")
}

/// Inserts a layer and makes it active (New Layer, Duplicate Layer).
pub struct InsertLayer {
    label: &'static str,
    index: usize,
    layer: Option<Layer>,
    previous_active: usize,
}

impl Command for InsertLayer {
    fn label(&self) -> &str {
        self.label
    }

    fn apply(&mut self, doc: &mut Document) -> Result<(), CommandError> {
        let layer = self
            .layer
            .take()
            .ok_or_else(|| invalid("layer already inserted"))?;
        self.previous_active = doc.active_layer();
        if let Err(e) = doc.insert_layer(self.index, layer.clone()) {
            self.layer = Some(layer);
            return Err(CommandError::Invalid(e.to_string()));
        }
        doc.set_active_layer(self.index);
        Ok(())
    }

    fn revert(&mut self, doc: &mut Document) {
        self.layer = doc.remove_layer(self.index).ok();
        doc.set_active_layer(self.previous_active);
    }

    fn dirty_rect(&self, doc: &Document) -> PixelRect {
        doc.bounds()
    }

    fn memory_cost(&self) -> usize {
        layer_bytes(self.layer.as_ref())
    }
}

fn layer_bytes(layer: Option<&Layer>) -> usize {
    layer.map_or(0, |l| {
        l.pixels.as_raw().len() + l.mask().map_or(0, |m| m.as_raw().len())
    })
}

/// Inserts `layer` at `index` (made active) as one undo step named `label`.
pub fn insert_layer(label: &'static str, index: usize, layer: Layer) -> InsertLayer {
    InsertLayer {
        label,
        index,
        layer: Some(layer),
        previous_active: 0,
    }
}

/// Layer → New Layer: a transparent layer above the active one.
pub fn new_layer(doc: &Document) -> InsertLayer {
    InsertLayer {
        label: "New Layer",
        index: (doc.active_layer() + 1).min(doc.layer_count()),
        layer: Some(Layer::transparent(
            next_layer_name(doc),
            doc.width(),
            doc.height(),
        )),
        previous_active: doc.active_layer(),
    }
}

/// Layer → Duplicate Layer: an exact copy named "<name> copy" above the active layer.
pub fn duplicate_layer(doc: &Document) -> Option<InsertLayer> {
    let active = doc.active_layer();
    let mut copy = doc.layer(active)?.clone();
    copy.name = format!("{} copy", copy.name);
    Some(InsertLayer {
        label: "Duplicate Layer",
        index: active + 1,
        layer: Some(copy),
        previous_active: active,
    })
}

/// Layer → Delete Layer (never the last remaining layer).
pub struct DeleteLayer {
    index: usize,
    removed: Option<Layer>,
    previous_active: usize,
}

/// Deletes the active layer; `None` when it is the only layer.
pub fn delete_layer(doc: &Document) -> Option<DeleteLayer> {
    (doc.layer_count() > 1).then(|| DeleteLayer {
        index: doc.active_layer(),
        removed: None,
        previous_active: doc.active_layer(),
    })
}

impl Command for DeleteLayer {
    fn label(&self) -> &str {
        "Delete Layer"
    }

    fn apply(&mut self, doc: &mut Document) -> Result<(), CommandError> {
        if doc.layer_count() <= 1 {
            return Err(invalid("cannot delete the only layer"));
        }
        self.previous_active = doc.active_layer();
        let removed = doc
            .remove_layer(self.index)
            .map_err(|e| CommandError::Invalid(e.to_string()))?;
        self.removed = Some(removed);
        doc.set_active_layer(self.index.saturating_sub(1));
        Ok(())
    }

    fn revert(&mut self, doc: &mut Document) {
        if let Some(layer) = self.removed.take() {
            let _ = doc.insert_layer(self.index, layer);
            doc.set_active_layer(self.previous_active);
        }
    }

    fn dirty_rect(&self, doc: &Document) -> PixelRect {
        doc.bounds()
    }

    fn memory_cost(&self) -> usize {
        layer_bytes(self.removed.as_ref())
    }
}

/// Swaps two neighbouring layers; the moved layer stays active.
pub struct MoveLayer {
    from: usize,
    to: usize,
}

/// Moves the active layer up (`up`) or down; `None` at the top / bottom.
pub fn move_layer(doc: &Document, up: bool) -> Option<MoveLayer> {
    let from = doc.active_layer();
    let to = if up {
        (from + 1 < doc.layer_count()).then_some(from + 1)?
    } else {
        from.checked_sub(1)?
    };
    Some(MoveLayer { from, to })
}

impl Command for MoveLayer {
    fn label(&self) -> &str {
        if self.to > self.from {
            "Move Layer Up"
        } else {
            "Move Layer Down"
        }
    }

    fn apply(&mut self, doc: &mut Document) -> Result<(), CommandError> {
        doc.move_layer(self.from, self.to)
            .map_err(|e| CommandError::Invalid(e.to_string()))?;
        doc.set_active_layer(self.to);
        Ok(())
    }

    fn revert(&mut self, doc: &mut Document) {
        let _ = doc.move_layer(self.to, self.from);
        doc.set_active_layer(self.from);
    }

    fn dirty_rect(&self, doc: &Document) -> PixelRect {
        doc.bounds()
    }

    fn memory_cost(&self) -> usize {
        0
    }
}

/// Replaces a run of layers with others (merge down, flatten, mask edits).
pub struct ReplaceLayers {
    label: &'static str,
    start: usize,
    old: Vec<Layer>,
    new: Vec<Layer>,
    old_active: usize,
    new_active: usize,
}

impl Command for ReplaceLayers {
    fn label(&self) -> &str {
        self.label
    }

    fn apply(&mut self, doc: &mut Document) -> Result<(), CommandError> {
        swap_run(doc, self.start, &self.old, &self.new, self.new_active)
    }

    fn revert(&mut self, doc: &mut Document) {
        let _ = swap_run(doc, self.start, &self.new, &self.old, self.old_active);
    }

    fn dirty_rect(&self, doc: &Document) -> PixelRect {
        doc.bounds()
    }

    fn memory_cost(&self) -> usize {
        self.old
            .iter()
            .chain(&self.new)
            .map(|l| layer_bytes(Some(l)))
            .sum()
    }
}

/// Replaces `current` (expected at `start`) with `replacement`.
fn swap_run(
    doc: &mut Document,
    start: usize,
    current: &[Layer],
    replacement: &[Layer],
    active: usize,
) -> Result<(), CommandError> {
    let mut layers = doc.layers().to_vec();
    if start + current.len() > layers.len() {
        return Err(invalid("layer stack changed"));
    }
    layers.splice(start..start + current.len(), replacement.iter().cloned());
    doc.replace_layers(layers, active)
        .map_err(|e| CommandError::Invalid(e.to_string()))
}

fn replace_one(doc: &Document, label: &'static str, index: usize, new: Layer) -> ReplaceLayers {
    ReplaceLayers {
        label,
        start: index,
        old: vec![doc.layers()[index].clone()],
        new: vec![new],
        old_active: doc.active_layer(),
        new_active: index,
    }
}

/// The layer's pixels with its enabled mask multiplied into alpha.
fn pixels_with_mask(layer: &Layer) -> RgbaImage {
    let mut pixels = layer.pixels.clone();
    if layer.mask().is_some() && layer.mask_enabled {
        for (x, y, p) in pixels.enumerate_pixels_mut() {
            p[3] = (p[3] as u32 * layer.mask_at(x, y) as u32 / 255) as u8;
        }
    }
    pixels
}

/// Layer → Merge Down; `None` on the bottom layer.
pub fn merge_down(doc: &Document) -> Option<ReplaceLayers> {
    let upper_index = doc.active_layer();
    let lower_index = upper_index.checked_sub(1)?;
    let upper = doc.layer(upper_index)?;
    let lower = doc.layer(lower_index)?;
    let mut merged = lower.clone();
    let mut pixels = pixels_with_mask(lower);
    if upper.visible && upper.opacity() > 0.0 {
        composite_layer_onto(&mut pixels, upper, doc.bounds());
    }
    merged.pixels = pixels;
    merged.set_mask(None).expect("removing a mask cannot fail");
    Some(ReplaceLayers {
        label: "Merge Down",
        start: lower_index,
        old: vec![lower.clone(), upper.clone()],
        new: vec![merged],
        old_active: upper_index,
        new_active: lower_index,
    })
}

/// Layer → Flatten Image: one "Background" layer holding the composite.
pub fn flatten(doc: &Document) -> ReplaceLayers {
    ReplaceLayers {
        label: "Flatten Image",
        start: 0,
        old: doc.layers().to_vec(),
        new: vec![Layer::new("Background", doc.composite())],
        old_active: doc.active_layer(),
        new_active: 0,
    }
}

/// Layer → Add Layer Mask (reveal all) or Add Mask from Selection.
pub fn add_mask(doc: &Document, from_selection: bool) -> Option<ReplaceLayers> {
    let index = doc.active_layer();
    let layer = doc.layer(index)?;
    if layer.mask().is_some() {
        return None;
    }
    let mask = if from_selection {
        doc.selection()?.clone()
    } else {
        GrayImage::from_pixel(doc.width(), doc.height(), Luma([255]))
    };
    let mut new = layer.clone();
    new.set_mask(Some(mask)).ok()?;
    new.mask_enabled = true;
    let label = if from_selection {
        "Add Mask from Selection"
    } else {
        "Add Layer Mask"
    };
    Some(replace_one(doc, label, index, new))
}

/// Layer → Delete Mask.
pub fn delete_mask(doc: &Document) -> Option<ReplaceLayers> {
    let index = doc.active_layer();
    let layer = doc.layer(index)?;
    layer.mask()?;
    let mut new = layer.clone();
    new.set_mask(None).ok()?;
    Some(replace_one(doc, "Delete Mask", index, new))
}

/// Layer → Apply Mask: multiplies alpha by the mask and removes it.
pub fn apply_mask(doc: &Document) -> Option<ReplaceLayers> {
    let index = doc.active_layer();
    let layer = doc.layer(index)?;
    let mask = layer.mask()?;
    let mut new = layer.clone();
    for (x, y, p) in new.pixels.enumerate_pixels_mut() {
        p[3] = (p[3] as u32 * mask.get_pixel(x, y)[0] as u32 / 255) as u8;
    }
    new.set_mask(None).ok()?;
    Some(replace_one(doc, "Apply Mask", index, new))
}

/// Changes a layer's name, visibility, opacity, blend mode, or mask enabling.
pub struct SetLayerProps {
    label: &'static str,
    index: usize,
    old: LayerProps,
    new: LayerProps,
}

impl SetLayerProps {
    /// A change from `old` to `new` on layer `index`; the label names the field changed.
    pub fn new(index: usize, old: LayerProps, new: LayerProps) -> Self {
        let label = if old.name != new.name {
            "Rename Layer"
        } else if old.visible != new.visible {
            if new.visible {
                "Show Layer"
            } else {
                "Hide Layer"
            }
        } else if old.blend_mode != new.blend_mode {
            "Blend Mode"
        } else if old.mask_enabled != new.mask_enabled {
            if new.mask_enabled {
                "Enable Mask"
            } else {
                "Disable Mask"
            }
        } else {
            "Layer Opacity"
        };
        Self {
            label,
            index,
            old,
            new,
        }
    }
}

impl Command for SetLayerProps {
    fn label(&self) -> &str {
        self.label
    }

    fn apply(&mut self, doc: &mut Document) -> Result<(), CommandError> {
        if self.new.name.trim().is_empty() {
            return Err(invalid("layer names cannot be empty"));
        }
        let layer = doc
            .layer_mut(self.index)
            .ok_or_else(|| invalid("no such layer"))?;
        self.new.apply_to(layer);
        Ok(())
    }

    fn revert(&mut self, doc: &mut Document) {
        if let Some(layer) = doc.layer_mut(self.index) {
            self.old.apply_to(layer);
        }
    }

    fn dirty_rect(&self, doc: &Document) -> PixelRect {
        doc.bounds()
    }

    fn memory_cost(&self) -> usize {
        0
    }
}

/// A property change on the active layer, or `None` if nothing changes.
pub fn set_props(
    doc: &Document,
    index: usize,
    edit: impl FnOnce(&mut LayerProps),
) -> Result<Option<SetLayerProps>, CommandError> {
    let layer = doc.layer(index).ok_or_else(|| no_layer(doc, index))?;
    let old = LayerProps::of(layer);
    let mut new = old.clone();
    edit(&mut new);
    Ok((new != old).then(|| SetLayerProps::new(index, old, new)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::undo::UndoStack;
    use image::Rgba;

    fn solid(color: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(4, 2, Rgba(color))
    }

    fn doc3() -> Document {
        let mut doc = Document::new("d", 4, 2, 72.0).unwrap();
        doc.add_layer(Layer::new("bottom", solid([255, 255, 255, 255])))
            .unwrap();
        doc.add_layer(Layer::new("middle", solid([255, 0, 0, 128])))
            .unwrap();
        doc.add_layer(Layer::new("top", solid([0, 0, 255, 255])))
            .unwrap();
        doc.set_active_layer(1);
        doc
    }

    fn names(doc: &Document) -> Vec<&str> {
        doc.layers().iter().map(|l| l.name.as_str()).collect()
    }

    fn run<C: Command + 'static>(
        doc: &mut Document,
        stack: &mut UndoStack,
        make: impl FnOnce(&Document) -> C,
    ) {
        let cmd = make(doc);
        stack.execute(doc, Box::new(cmd)).unwrap();
    }

    #[test]
    fn new_and_duplicate_insert_above_active() {
        let mut doc = doc3();
        let original = doc.clone();
        let mut stack = UndoStack::new();
        run(&mut doc, &mut stack, new_layer);
        assert_eq!(names(&doc), ["bottom", "middle", "Layer 1", "top"]);
        assert_eq!(doc.active_layer(), 2);
        assert!(doc.layer(2).unwrap().pixels.pixels().all(|p| p[3] == 0));
        run(&mut doc, &mut stack, new_layer);
        assert_eq!(doc.layer(3).unwrap().name, "Layer 2");

        doc.set_active_layer(1);
        run(&mut doc, &mut stack, |d| duplicate_layer(d).unwrap());
        assert_eq!(doc.layer(2).unwrap().name, "middle copy");
        assert_eq!(doc.layer(2).unwrap().pixels, doc.layer(1).unwrap().pixels);
        assert_eq!(doc.active_layer(), 2);
        assert_eq!(stack.undo_label(), Some("Duplicate Layer"));

        while stack.undo(&mut doc).is_some() {}
        doc.set_active_layer(1);
        assert_eq!(doc, original);
    }

    #[test]
    fn delete_keeps_at_least_one_layer() {
        let mut doc = doc3();
        let original = doc.clone();
        let mut stack = UndoStack::new();
        run(&mut doc, &mut stack, |d| delete_layer(d).unwrap());
        assert_eq!(names(&doc), ["bottom", "top"]);
        assert_eq!(doc.active_layer(), 0, "the layer below becomes active");
        stack.undo(&mut doc);
        assert_eq!(doc, original);

        let single = Document::new_blank("s", 2, 2, 72.0).unwrap();
        assert!(delete_layer(&single).is_none());
    }

    #[test]
    fn move_up_and_down() {
        let mut doc = doc3();
        let mut stack = UndoStack::new();
        run(&mut doc, &mut stack, |d| move_layer(d, true).unwrap());
        assert_eq!(names(&doc), ["bottom", "top", "middle"]);
        assert_eq!(doc.active_layer(), 2);
        assert_eq!(stack.undo_label(), Some("Move Layer Up"));
        assert!(move_layer(&doc, true).is_none(), "already on top");
        run(&mut doc, &mut stack, |d| move_layer(d, false).unwrap());
        run(&mut doc, &mut stack, |d| move_layer(d, false).unwrap());
        assert_eq!(names(&doc), ["middle", "bottom", "top"]);
        assert!(move_layer(&doc, false).is_none());
        stack.undo(&mut doc);
        stack.undo(&mut doc);
        stack.undo(&mut doc);
        assert_eq!(names(&doc), ["bottom", "middle", "top"]);
        assert_eq!(doc.active_layer(), 1);
    }

    #[test]
    fn merge_down_matches_composite_of_the_pair() {
        let mut doc = doc3();
        doc.layer_mut(1).unwrap().blend_mode = BlendMode::Multiply;
        doc.layer_mut(1).unwrap().set_opacity(0.5);
        doc.layer_mut(0).unwrap().set_opacity(0.8);
        let before = doc.clone();
        let mut stack = UndoStack::new();
        run(&mut doc, &mut stack, |d| merge_down(d).unwrap());
        assert_eq!(names(&doc), ["bottom", "top"]);
        assert_eq!(doc.active_layer(), 0);
        let merged = doc.layer(0).unwrap();
        assert_eq!(merged.opacity(), 0.8, "keeps the lower layer's properties");
        // Expected: the middle layer composited onto the bottom layer's pixels.
        let mut pair = Document::new("p", 4, 2, 72.0).unwrap();
        pair.add_layer(Layer::new("b", before.layer(0).unwrap().pixels.clone()))
            .unwrap();
        pair.add_layer(before.layer(1).unwrap().clone()).unwrap();
        assert_eq!(merged.pixels, pair.composite());
        stack.undo(&mut doc);
        assert_eq!(doc, before);

        let mut bottom = doc3();
        bottom.set_active_layer(0);
        assert!(merge_down(&bottom).is_none());
    }

    #[test]
    fn merge_down_applies_masks_and_ignores_hidden_upper() {
        let mut doc = doc3();
        let mut mask = GrayImage::new(4, 2);
        mask.put_pixel(0, 0, Luma([255]));
        doc.layer_mut(0).unwrap().set_mask(Some(mask)).unwrap();
        doc.layer_mut(1).unwrap().visible = false;
        let mut stack = UndoStack::new();
        run(&mut doc, &mut stack, |d| merge_down(d).unwrap());
        let merged = doc.layer(0).unwrap();
        assert!(merged.mask().is_none());
        assert_eq!(merged.pixels.get_pixel(0, 0).0, [255, 255, 255, 255]);
        assert_eq!(merged.pixels.get_pixel(1, 0)[3], 0, "lower mask applied");
    }

    #[test]
    fn flatten_equals_composite_and_keeps_transparency() {
        let mut doc = doc3();
        doc.layer_mut(0)
            .unwrap()
            .pixels
            .put_pixel(0, 0, Rgba([0, 0, 0, 0]));
        doc.layer_mut(2).unwrap().visible = false;
        let composite = doc.composite();
        let before = doc.clone();
        let mut stack = UndoStack::new();
        run(&mut doc, &mut stack, flatten);
        assert_eq!(names(&doc), ["Background"]);
        assert_eq!(doc.layer(0).unwrap().pixels, composite);
        assert!(doc.layer(0).unwrap().pixels.get_pixel(0, 0)[3] < 255);
        stack.undo(&mut doc);
        assert_eq!(doc, before);
    }

    #[test]
    fn masks_add_delete_apply_toggle() {
        let mut doc = doc3();
        let before = doc.clone();
        let mut stack = UndoStack::new();
        run(&mut doc, &mut stack, |d| add_mask(d, false).unwrap());
        assert!(doc
            .layer(1)
            .unwrap()
            .mask()
            .unwrap()
            .iter()
            .all(|v| *v == 255));
        assert!(add_mask(&doc, false).is_none(), "already has a mask");
        assert_eq!(
            doc.composite(),
            before.composite(),
            "reveal-all changes nothing"
        );

        // Paint the mask black on the left half, then toggle and apply.
        for y in 0..2 {
            for x in 0..2 {
                doc.layer_mut(1)
                    .unwrap()
                    .mask_mut()
                    .unwrap()
                    .put_pixel(x, y, Luma([0]));
            }
        }
        let toggle = set_props(&doc, 1, |p| p.mask_enabled = false)
            .unwrap()
            .unwrap();
        assert_eq!(toggle.label(), "Disable Mask");
        run(&mut doc, &mut stack, |_| toggle);
        assert_eq!(doc.composite(), before.composite());
        stack.undo(&mut doc);
        run(&mut doc, &mut stack, |d| apply_mask(d).unwrap());
        let layer = doc.layer(1).unwrap();
        assert!(layer.mask().is_none());
        assert_eq!(layer.pixels.get_pixel(0, 0)[3], 0);
        assert_eq!(layer.pixels.get_pixel(3, 0)[3], 128);
        stack.undo(&mut doc);
        run(&mut doc, &mut stack, |d| delete_mask(d).unwrap());
        assert_eq!(
            doc.layer(1).unwrap().pixels,
            before.layer(1).unwrap().pixels
        );
        assert!(doc.layer(1).unwrap().mask().is_none());

        // From selection.
        let mut doc = doc3();
        assert!(add_mask(&doc, true).is_none(), "needs a selection");
        let sel = crate::selection::rect_mask(4, 2, (0.0, 0.0), (2.0, 2.0));
        doc.set_selection(Some(sel.clone())).unwrap();
        let cmd = add_mask(&doc, true).unwrap();
        assert_eq!(cmd.label(), "Add Mask from Selection");
        let mut stack = UndoStack::new();
        run(&mut doc, &mut stack, |_| cmd);
        assert_eq!(doc.layer(1).unwrap().mask(), Some(&sel));
    }

    #[test]
    fn property_changes_are_labelled_and_undoable() {
        let mut doc = doc3();
        let before = doc.clone();
        let mut stack = UndoStack::new();
        let rename = set_props(&doc, 1, |p| p.name = "Shadow".into())
            .unwrap()
            .unwrap();
        assert_eq!(rename.label(), "Rename Layer");
        run(&mut doc, &mut stack, |_| rename);
        let hide = set_props(&doc, 2, |p| p.visible = false).unwrap().unwrap();
        assert_eq!(hide.label(), "Hide Layer");
        run(&mut doc, &mut stack, |_| hide);
        let blend = set_props(&doc, 1, |p| p.blend_mode = BlendMode::Screen)
            .unwrap()
            .unwrap();
        assert_eq!(blend.label(), "Blend Mode");
        run(&mut doc, &mut stack, |_| blend);
        let opacity = set_props(&doc, 1, |p| p.opacity = 0.25).unwrap().unwrap();
        assert_eq!(opacity.label(), "Layer Opacity");
        run(&mut doc, &mut stack, |_| opacity);
        assert_eq!(doc.layer(1).unwrap().opacity(), 0.25);
        assert!(set_props(&doc, 1, |_| {}).unwrap().is_none(), "no-op");
        let empty = set_props(&doc, 1, |p| p.name = "  ".into())
            .unwrap()
            .unwrap();
        assert!(stack.execute(&mut doc, Box::new(empty)).is_err());
        while stack.undo(&mut doc).is_some() {}
        assert_eq!(doc, before);
        assert!(set_props(&doc, 9, |_| {}).is_err());
    }

    #[test]
    fn next_layer_name_skips_used_numbers() {
        let mut doc = Document::new_blank("d", 1, 1, 72.0).unwrap();
        assert_eq!(next_layer_name(&doc), "Layer 1");
        doc.add_layer(Layer::transparent("Layer 1", 1, 1)).unwrap();
        doc.add_layer(Layer::transparent("Layer 3", 1, 1)).unwrap();
        assert_eq!(next_layer_name(&doc), "Layer 2");
    }
}
