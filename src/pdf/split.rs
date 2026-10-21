//! Splitting a part along a straight line, with a glue strip on one piece and
//! alignment marks across the cut on both.

use image::{Rgba, RgbaImage};

use super::mm_to_px;

/// Default glue strip width.
pub const DEFAULT_GLUE_MM: f32 = 8.0;
/// Spacing of the alignment marks along the cut.
pub const MARK_SPACING_MM: f32 = 20.0;
/// Length of each mark on either side of the cut.
pub const MARK_LENGTH_MM: f32 = 3.0;

const OUTLINE: Rgba<u8> = Rgba([0, 0, 0, 255]);
const MARK: Rgba<u8> = Rgba([200, 0, 0, 255]);

/// Which piece of a split gets the glue strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GlueOn {
    /// The piece with fewer pixels.
    #[default]
    Smaller,
    /// Piece 1: the side where [`side_of`] is ≥ 0 (marked "1" in the Split
    /// window).
    First,
    /// Piece 2: the other side.
    Second,
}

/// One piece of a split part.
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    /// Cropped to its pixels.
    pub image: RgbaImage,
    /// Where `image` sits relative to the original part image.
    pub offset: (i32, i32),
    /// Whether this piece carries the glue strip.
    pub glue: bool,
}

/// Signed distance (pixels) of `p` from the line through `a` and `b`.
pub fn side_of(a: (f32, f32), b: (f32, f32), p: (f32, f32)) -> f32 {
    side(a, b, p)
}

fn side(a: (f32, f32), b: (f32, f32), p: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = (dx * dx + dy * dy).sqrt().max(1e-6);
    (dx * (p.1 - a.1) - dy * (p.0 - a.0)) / len
}

fn crop(img: &RgbaImage) -> Option<(RgbaImage, (i32, i32))> {
    let r = crate::cleanup::content_bounds(&[img])?;
    Some((
        image::imageops::crop_imm(img, r.x, r.y, r.width, r.height).to_image(),
        (r.x as i32, r.y as i32),
    ))
}

/// Splits `part` along the line through `a` and `b` (part-image pixels).
/// The piece chosen by `on` (by default the smaller) gets a `glue_mm` strip copied from the other piece
/// along the cut (outlined, with a dashed fold line on the cut); both get
/// alignment marks across the cut. `None` if the line does not divide the
/// part into two non-empty pieces.
pub fn split(
    part: &RgbaImage,
    a: (f32, f32),
    b: (f32, f32),
    glue_mm: f32,
    on: GlueOn,
) -> Option<[Piece; 2]> {
    let (w, h) = part.dimensions();
    let glue = mm_to_px(glue_mm.clamp(0.0, 20.0));
    let mut left = RgbaImage::new(w, h);
    let mut right = RgbaImage::new(w, h);
    let (mut nl, mut nr) = (0usize, 0usize);
    for (x, y, p) in part.enumerate_pixels() {
        if p[3] == 0 {
            continue;
        }
        if side(a, b, (x as f32 + 0.5, y as f32 + 0.5)) >= 0.0 {
            left.put_pixel(x, y, *p);
            nl += 1;
        } else {
            right.put_pixel(x, y, *p);
            nr += 1;
        }
    }
    if nl == 0 || nr == 0 {
        return None;
    }
    let left_gets_glue = match on {
        GlueOn::Smaller => nl <= nr,
        GlueOn::First => true,
        GlueOn::Second => false,
    };
    if glue > 0.0 {
        let (receiver, sign) = if left_gets_glue {
            (&mut left, -1.0)
        } else {
            (&mut right, 1.0)
        };
        for (x, y, p) in part.enumerate_pixels() {
            if p[3] == 0 {
                continue;
            }
            let d = side(a, b, (x as f32 + 0.5, y as f32 + 0.5)) * sign;
            if d > 0.0 && d <= glue {
                receiver.put_pixel(x, y, *p);
                // Outline at the strip's outer edge.
                if d > glue - 1.5 {
                    receiver.put_pixel(x, y, OUTLINE);
                }
            }
            // Dashed fold line on the cut.
            if d.abs() < 1.0 {
                let along = (x as f32 - a.0).hypot(y as f32 - a.1);
                if (along / mm_to_px(2.0)) as i32 % 2 == 0 {
                    receiver.put_pixel(x, y, OUTLINE);
                }
            }
        }
    }
    // Alignment marks: perpendicular ticks across the cut, spread over the
    // part's extent along the line, at least two.
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = (dx * dx + dy * dy).sqrt().max(1e-6);
    let (ux, uy) = (dx / len, dy / len);
    let (nx, ny) = (-uy, ux);
    let mut ts: Vec<f32> = part
        .enumerate_pixels()
        .filter(|(x, y, p)| p[3] > 0 && side(a, b, (*x as f32 + 0.5, *y as f32 + 0.5)).abs() < 1.0)
        .map(|(x, y, _)| (x as f32 - a.0) * ux + (y as f32 - a.1) * uy)
        .collect();
    ts.sort_by(f32::total_cmp);
    if let (Some(&t0), Some(&t1)) = (ts.first(), ts.last()) {
        let span = t1 - t0;
        let count = ((span / mm_to_px(MARK_SPACING_MM)).floor() as usize).max(2);
        let mark = mm_to_px(MARK_LENGTH_MM);
        for k in 0..count {
            let t = t0 + span * (k as f32 + 0.5) / count as f32;
            let (cx, cy) = (a.0 + ux * t, a.1 + uy * t);
            let steps = (mark * 2.0) as i32;
            for s in -steps..=steps {
                let off = s as f32 / 2.0;
                for width in [-1.0f32, 0.0, 1.0] {
                    let (px, py) = (cx + nx * off + ux * width, cy + ny * off + uy * width);
                    let (rx, ry) = (px.round(), py.round());
                    if rx < 0.0 || ry < 0.0 || rx >= w as f32 || ry >= h as f32 {
                        continue;
                    }
                    let (ix, iy) = (rx as u32, ry as u32);
                    // Marks only over a piece's own pixels (the receiving
                    // piece's strip included), never beyond them.
                    for piece in [&mut left, &mut right] {
                        if piece.get_pixel(ix, iy)[3] > 0 {
                            piece.put_pixel(ix, iy, MARK);
                        }
                    }
                }
            }
        }
    }
    let (li, lo) = crop(&left)?;
    let (ri, ro) = crop(&right)?;
    Some([
        Piece {
            image: li,
            offset: lo,
            glue: left_gets_glue && glue > 0.0,
        },
        Piece {
            image: ri,
            offset: ro,
            glue: !left_gets_glue && glue > 0.0,
        },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wing() -> RgbaImage {
        // 400 × 60 mm at 300 dpi would be big; use a 1200 × 200 px panel.
        RgbaImage::from_fn(1200, 200, |x, y| {
            Rgba([100, (x / 5) as u8, (y / 2) as u8, 255])
        })
    }

    #[test]
    fn a_vertical_cut_gives_two_pieces_and_a_glue_strip() {
        let part = wing();
        let [l, r] = split(
            &part,
            (500.0, -10.0),
            (500.0, 210.0),
            DEFAULT_GLUE_MM,
            GlueOn::Smaller,
        )
        .unwrap();
        let glue = mm_to_px(DEFAULT_GLUE_MM).floor() as u32;
        // The smaller (left) piece carries the strip.
        assert!(l.glue && !r.glue);
        assert_eq!(l.offset, (0, 0));
        assert!((l.image.width() as i32 - (500 + glue as i32)).abs() <= 1);
        assert_eq!((r.offset, r.image.width()), ((500, 0), 700));
        // Strip content matches the other side.
        assert_eq!(
            l.image.get_pixel(510, 100).0[1],
            part.get_pixel(510, 100).0[1]
        );
        // Marks on both pieces at the cut.
        assert!(l.image.pixels().any(|p| *p == MARK) && r.image.pixels().any(|p| *p == MARK));
    }

    #[test]
    fn the_glue_strip_goes_on_the_chosen_piece() {
        let part = wing();
        // Line downwards at x = 500: piece 1 (side ≥ 0) is x < 500.
        assert!(side_of((500.0, -10.0), (500.0, 210.0), (100.0, 50.0)) > 0.0);
        let [l, r] = split(&part, (500.0, -10.0), (500.0, 210.0), 8.0, GlueOn::Second).unwrap();
        assert!(!l.glue && r.glue);
        let [l, r] = split(&part, (500.0, -10.0), (500.0, 210.0), 8.0, GlueOn::First).unwrap();
        assert!(l.glue && !r.glue);
        // The larger piece can carry it: it grows by the strip.
        let glue = mm_to_px(8.0).floor() as u32;
        assert!(r.image.width() <= 700);
        let [_, r] = split(&part, (500.0, -10.0), (500.0, 210.0), 8.0, GlueOn::Second).unwrap();
        assert!((r.image.width() as i32 - (700 + glue as i32)).abs() <= 1);
    }

    #[test]
    fn no_glue_means_a_plain_cut_and_a_missed_line_does_nothing() {
        let part = wing();
        let [l, r] = split(&part, (600.0, 0.0), (600.0, 200.0), 0.0, GlueOn::Smaller).unwrap();
        assert_eq!(l.image.width() + r.image.width(), 1200);
        assert!(split(&part, (-5.0, 0.0), (-5.0, 200.0), 8.0, GlueOn::Smaller).is_none());
    }
}
