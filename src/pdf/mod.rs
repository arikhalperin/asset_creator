//! Paper-model PDFs: rendering pages, finding the printed parts, packing them
//! onto A4 sheets at true scale, splitting oversized parts, and exporting.

pub mod images;
pub mod layout;
pub mod pack;
pub mod parts;
pub mod pdfium;
pub mod render;
pub mod split;

/// Print resolution of rendered pages and exported sheets.
pub const DPI: f32 = 300.0;

/// Millimetres to pixels at [`DPI`].
pub fn mm_to_px(mm: f32) -> f32 {
    mm / 25.4 * DPI
}

/// Pixels at [`DPI`] to millimetres.
pub fn px_to_mm(px: f32) -> f32 {
    px / DPI * 25.4
}
