//! Enhance Lines (see [`crate::enhance`]).

use super::{Category, Filter, ParamKind, ParamSpec, Params, DPI};
use crate::enhance::{self, Settings};
use image::RgbaImage;

pub struct EnhanceLines;

fn settings(params: &Params) -> Settings {
    Settings {
        strength: params.float("strength") / 100.0,
        gaps_mm: params.float("gaps"),
        thickness: params.float("thickness"),
        smooth: params.float("smooth") / 100.0,
    }
}

fn px_per_mm(params: &Params) -> f32 {
    let dpi = params.float(DPI);
    enhance::px_per_mm(if dpi > 0.0 { dpi } else { 300.0 })
}

impl Filter for EnhanceLines {
    fn id(&self) -> &'static str {
        "enhance_lines"
    }
    fn name(&self) -> &'static str {
        "Enhance Lines"
    }
    fn category(&self) -> Category {
        Category::Sharpen
    }
    fn params(&self) -> Vec<ParamSpec> {
        let d = Settings::default();
        let float = |min: f32, max: f32, default: f32, step: f32| ParamKind::Float {
            min,
            max,
            default,
            step,
        };
        vec![
            ParamSpec::new(
                "strength",
                "Line strength (%)",
                float(0.0, 100.0, d.strength * 100.0, 1.0),
            ),
            ParamSpec::new("gaps", "Close gaps (mm)", float(0.0, 2.0, d.gaps_mm, 0.1)),
            ParamSpec::new(
                "thickness",
                "Thickness (×)",
                float(1.0, 2.0, d.thickness, 0.1),
            ),
            ParamSpec::new(
                "smooth",
                "Smooth grain (%)",
                float(0.0, 100.0, d.smooth * 100.0, 1.0),
            ),
        ]
    }
    fn margin(&self, params: &Params) -> u32 {
        settings(params).margin_px(px_per_mm(params))
    }
    fn apply(&self, source: &RgbaImage, params: &Params) -> RgbaImage {
        enhance::enhance(source, &settings(params), px_per_mm(params), None)
    }
}
