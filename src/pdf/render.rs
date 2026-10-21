//! Rendering PDF pages at print resolution.

use std::path::Path;

use image::RgbaImage;
use pdfium_render::prelude::*;

use super::{pdfium, DPI};

/// One rendered page.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    /// White background, `DPI` pixels per inch.
    pub image: RgbaImage,
    pub width_mm: f32,
    pub height_mm: f32,
}

/// Number of pages of the PDF at `path`.
pub fn page_count(lib: &Path, path: &Path) -> Result<usize, String> {
    let pdfium = pdfium::pdfium(lib)?;
    let doc = pdfium
        .load_pdf_from_file(path, None)
        .map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    Ok(doc.pages().len() as usize)
}

/// Renders page `index` of the PDF at `path` at [`DPI`].
pub fn render_page(lib: &Path, path: &Path, index: usize) -> Result<Page, String> {
    let pdfium = pdfium::pdfium(lib)?;
    let doc = pdfium
        .load_pdf_from_file(path, None)
        .map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let page = doc
        .pages()
        .get(index as i32)
        .map_err(|e| format!("page {}: {e}", index + 1))?;
    let (w_pt, h_pt) = (page.width().value, page.height().value);
    let w = (w_pt / 72.0 * DPI).round() as i32;
    let h = (h_pt / 72.0 * DPI).round() as i32;
    let config = PdfRenderConfig::new()
        .set_target_size(w, h)
        .set_clear_color(PdfColor::WHITE)
        .render_form_data(true);
    let bitmap = page
        .render_with_config(&config)
        .map_err(|e| format!("page {}: {e}", index + 1))?;
    let image = bitmap
        .as_image()
        .map_err(|e| format!("page {}: {e}", index + 1))?
        .into_rgba8();
    Ok(Page {
        image,
        width_mm: w_pt / 72.0 * 25.4,
        height_mm: h_pt / 72.0 * 25.4,
    })
}
