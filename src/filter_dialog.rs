//! The filter preview dialog, generated from a filter's parameter schema.

use egui::{Color32, Pos2, Rect, Sense, Shape, Stroke, Vec2};

use crate::filters::{self, curve_lut, Filter, ParamKind, ParamSpec, ParamValue, Params};

/// An open filter dialog.
#[derive(Debug, Clone, PartialEq)]
pub struct FilterDialog {
    pub filter_id: &'static str,
    pub params: Params,
    pub preview: bool,
    /// Curve point being dragged: (curve key, point index).
    curve_drag: Option<(&'static str, usize)>,
}

/// What the user did with the dialog this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogOutcome {
    Open { changed: bool },
    Ok,
    Cancel,
}

impl FilterDialog {
    pub fn new(filter: &dyn Filter) -> Self {
        Self {
            filter_id: filter.id(),
            params: Params::defaults(&filter.params()),
            preview: true,
            curve_drag: None,
        }
    }

    pub fn filter(&self) -> &'static dyn Filter {
        filters::find(self.filter_id).expect("registered filter")
    }

    /// Restores default values (keeping the current seed, so Reset is not a reroll).
    pub fn reset(&mut self) {
        let schema = self.filter().params();
        let mut defaults = Params::defaults(&schema);
        for spec in &schema {
            if spec.kind == ParamKind::Seed {
                if let Some(seed) = self.params.get(spec.key).cloned() {
                    defaults.set(spec.key, seed);
                }
            }
        }
        self.params = defaults;
    }

    /// Whether `spec` is shown under the current group selections.
    pub fn is_visible(&self, spec: &ParamSpec) -> bool {
        spec.group
            .is_none_or(|(selector, option)| self.params.choice(selector) == option)
    }
}

/// Draws the dialog window.
pub fn show(ctx: &egui::Context, dialog: &mut FilterDialog) -> DialogOutcome {
    let filter = dialog.filter();
    let schema = filter.params();
    let before = (dialog.params.clone(), dialog.preview);
    let mut outcome = None;
    egui::Window::new(filter.name())
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::RIGHT_TOP, Vec2::new(-24.0, 72.0))
        .show(ctx, |ui| {
            egui::Grid::new("filter_params")
                .num_columns(2)
                .show(ui, |ui| {
                    for spec in &schema {
                        if !dialog.is_visible(spec) {
                            continue;
                        }
                        if spec.kind == ParamKind::Curve {
                            continue;
                        }
                        ui.label(spec.label);
                        param_widget(ui, spec, &mut dialog.params);
                        ui.end_row();
                    }
                });
            for spec in &schema {
                if spec.kind == ParamKind::Curve && dialog.is_visible(spec) {
                    curve_editor(ui, spec.key, &mut dialog.params, &mut dialog.curve_drag);
                }
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.checkbox(&mut dialog.preview, "Preview");
                if ui.button("Reset").clicked() {
                    dialog.reset();
                }
                if ui.button("Cancel").clicked() {
                    outcome = Some(DialogOutcome::Cancel);
                }
                if ui.button("OK").clicked() {
                    outcome = Some(DialogOutcome::Ok);
                }
            });
        });
    dialog.params.clamp(&schema);
    outcome.unwrap_or(DialogOutcome::Open {
        changed: (dialog.params.clone(), dialog.preview) != before,
    })
}

fn param_widget(ui: &mut egui::Ui, spec: &ParamSpec, params: &mut Params) {
    let Some(value) = params.get_mut(spec.key) else {
        return;
    };
    match (&spec.kind, value) {
        (ParamKind::Int { min, max, .. }, ParamValue::Int(v)) => {
            ui.add(egui::Slider::new(v, *min..=*max));
        }
        (ParamKind::Float { min, max, step, .. }, ParamValue::Float(v)) => {
            let decimals = if *step >= 1.0 {
                0
            } else if *step >= 0.1 {
                1
            } else {
                2
            };
            ui.add(
                egui::Slider::new(v, *min..=*max)
                    .step_by(*step as f64)
                    .max_decimals(decimals)
                    .logarithmic(*max / min.max(0.01) > 100.0),
            );
        }
        (ParamKind::Bool { .. }, ParamValue::Bool(v)) => {
            ui.checkbox(v, "");
        }
        (ParamKind::Choice { options, .. }, ParamValue::Choice(v)) => {
            ui.horizontal(|ui| {
                for (i, option) in options.iter().enumerate() {
                    ui.selectable_value(v, i, *option);
                }
            });
        }
        (ParamKind::Seed, ParamValue::Seed(v)) => {
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(v));
                if ui.small_button("New").clicked() {
                    *v = filters::random_seed();
                }
            });
        }
        _ => {}
    }
}

/// Size of the curve graph in points.
const GRAPH: f32 = 220.0;

/// A draggable curve editor: drag points, click to add, drag a point off the
/// graph to remove it (endpoints stay, moving only vertically).
fn curve_editor(
    ui: &mut egui::Ui,
    key: &'static str,
    params: &mut Params,
    drag: &mut Option<(&'static str, usize)>,
) {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(GRAPH), Sense::click_and_drag());
    let to_screen = |p: (f32, f32)| {
        Pos2::new(
            rect.left() + p.0 / 255.0 * GRAPH,
            rect.bottom() - p.1 / 255.0 * GRAPH,
        )
    };
    let to_value = |s: Pos2| {
        (
            ((s.x - rect.left()) / GRAPH * 255.0).clamp(0.0, 255.0),
            ((rect.bottom() - s.y) / GRAPH * 255.0).clamp(0.0, 255.0),
        )
    };
    let mut points = params.curve(key);

    if let Some(pointer) = response.interact_pointer_pos() {
        if response.drag_started() || response.clicked() {
            let near = points
                .iter()
                .enumerate()
                .map(|(i, p)| (i, to_screen(*p).distance(pointer)))
                .filter(|(_, d)| *d <= 8.0)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i);
            let index = near.unwrap_or_else(|| {
                points.push(to_value(pointer));
                points.sort_by(|a, b| a.0.total_cmp(&b.0));
                let v = to_value(pointer);
                points.iter().position(|p| *p == v).unwrap_or(0)
            });
            *drag = Some((key, index));
        }
        if let Some((k, index)) = *drag {
            if k == key && index < points.len() && (response.dragged() || response.drag_started()) {
                let endpoint = index == 0 || index + 1 == points.len();
                let v = to_value(pointer);
                points[index] = if endpoint { (points[index].0, v.1) } else { v };
            }
        }
        if response.drag_stopped() {
            if let Some((k, index)) = drag.take() {
                let endpoint = index == 0 || index + 1 == points.len();
                if k == key && !endpoint && !rect.expand(20.0).contains(pointer) {
                    points.remove(index);
                }
            }
        }
    } else if response.drag_stopped() {
        *drag = None;
    }
    params.set(key, ParamValue::Curve(points.clone()));

    let painter = ui.painter_at(rect.expand(2.0));
    painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);
    let grid = Stroke::new(1.0, Color32::from_gray(120).gamma_multiply(0.4));
    for i in 1..4 {
        let t = i as f32 / 4.0 * GRAPH;
        painter.line_segment(
            [
                Pos2::new(rect.left() + t, rect.top()),
                Pos2::new(rect.left() + t, rect.bottom()),
            ],
            grid,
        );
        painter.line_segment(
            [
                Pos2::new(rect.left(), rect.top() + t),
                Pos2::new(rect.right(), rect.top() + t),
            ],
            grid,
        );
    }
    painter.line_segment([rect.left_bottom(), rect.right_top()], grid);
    let lut = curve_lut(&points);
    let line: Vec<Pos2> = (0..256)
        .map(|x| to_screen((x as f32, lut[x] as f32)))
        .collect();
    painter.add(Shape::line(
        line,
        Stroke::new(2.0, ui.visuals().strong_text_color()),
    ));
    for p in &points {
        let r = Rect::from_center_size(to_screen(*p), Vec2::splat(7.0));
        painter.rect_filled(r, 0.0, ui.visuals().strong_text_color());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_keeps_seed_and_groups_follow_selector() {
        let noise = filters::find("add_noise").unwrap();
        let mut d = FilterDialog::new(noise);
        d.params.set("seed", ParamValue::Seed(7));
        d.params.set("amount", ParamValue::Int(80));
        d.reset();
        assert_eq!(d.params.int("amount"), 10);
        assert_eq!(d.params.seed("seed"), 7);

        let levels = filters::find("levels").unwrap();
        let mut d = FilterDialog::new(levels);
        let schema = levels.params();
        let visible = |d: &FilterDialog| {
            schema
                .iter()
                .filter(|s| d.is_visible(s))
                .map(|s| s.key)
                .collect::<Vec<_>>()
        };
        assert!(visible(&d).contains(&"rgb.gamma") && !visible(&d).contains(&"red.gamma"));
        d.params.set("channel", ParamValue::Choice(1));
        assert!(visible(&d).contains(&"red.gamma") && !visible(&d).contains(&"rgb.gamma"));
    }
}
