//! Packing parts onto A4 sheets at true size (pixels at `DPI`).

use super::mm_to_px;

/// A4 portrait.
pub const SHEET_MM: (f32, f32) = (210.0, 297.0);
/// Unprinted border on every side.
pub const MARGIN_MM: f32 = 5.0;
/// Minimum gap between parts.
pub const SPACING_MM: f32 = 2.0;

/// The sheet's printable area in pixels (width, height).
pub fn printable_px() -> (u32, u32) {
    (
        mm_to_px(SHEET_MM.0 - 2.0 * MARGIN_MM).floor() as u32,
        mm_to_px(SHEET_MM.1 - 2.0 * MARGIN_MM).floor() as u32,
    )
}

pub fn spacing_px() -> u32 {
    mm_to_px(SPACING_MM).ceil() as u32
}

/// Whether a `w` × `h` part fits on a sheet in some 90° orientation.
pub fn fits(w: u32, h: u32) -> bool {
    let (pw, ph) = printable_px();
    (w <= pw && h <= ph) || (h <= pw && w <= ph)
}

/// Where one part sits: sheet, top-left inside the printable area, turned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub sheet: usize,
    pub x: u32,
    pub y: u32,
    /// Turned 90° clockwise.
    pub rotated: bool,
}

impl Placement {
    /// The occupied rectangle for a part of size `size` (x, y, w, h).
    pub fn rect(&self, size: (u32, u32)) -> (u32, u32, u32, u32) {
        let (w, h) = if self.rotated { (size.1, size.0) } else { size };
        (self.x, self.y, w, h)
    }
}

#[derive(Debug, Clone, Copy)]
struct Free {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

/// Max-rects free space of one sheet (sizes inflated by the spacing, so
/// placed parts keep at least `spacing` between them).
struct Bin {
    free: Vec<Free>,
}

impl Bin {
    fn new() -> Self {
        let (pw, ph) = printable_px();
        let s = spacing_px();
        Self {
            free: vec![Free {
                x: 0,
                y: 0,
                w: pw + s,
                h: ph + s,
            }],
        }
    }

    /// Best-short-side-fit position for an (inflated) `w` × `h` box.
    fn find(&self, w: u32, h: u32) -> Option<(u32, u32, u32)> {
        self.free
            .iter()
            .filter(|f| f.w >= w && f.h >= h)
            .map(|f| ((f.w - w).min(f.h - h), f.x, f.y))
            .min()
            .map(|(score, x, y)| (x, y, score))
    }

    fn occupy(&mut self, x: u32, y: u32, w: u32, h: u32) {
        let mut next = Vec::new();
        for f in &self.free {
            if x >= f.x + f.w || x + w <= f.x || y >= f.y + f.h || y + h <= f.y {
                next.push(*f);
                continue;
            }
            if x > f.x {
                next.push(Free {
                    x: f.x,
                    y: f.y,
                    w: x - f.x,
                    h: f.h,
                });
            }
            if x + w < f.x + f.w {
                next.push(Free {
                    x: x + w,
                    y: f.y,
                    w: f.x + f.w - x - w,
                    h: f.h,
                });
            }
            if y > f.y {
                next.push(Free {
                    x: f.x,
                    y: f.y,
                    w: f.w,
                    h: y - f.y,
                });
            }
            if y + h < f.y + f.h {
                next.push(Free {
                    x: f.x,
                    y: y + h,
                    w: f.w,
                    h: f.y + f.h - y - h,
                });
            }
        }
        // Drop free rectangles contained in others.
        let mut kept: Vec<Free> = Vec::new();
        for (i, a) in next.iter().enumerate() {
            let contained = next.iter().enumerate().any(|(j, b)| {
                let inside =
                    a.x >= b.x && a.y >= b.y && a.x + a.w <= b.x + b.w && a.y + a.h <= b.y + b.h;
                let same = (a.x, a.y, a.w, a.h) == (b.x, b.y, b.w, b.h);
                i != j && inside && (!same || j < i)
            });
            if !contained {
                kept.push(*a);
            }
        }
        self.free = kept;
    }
}

/// Packs parts of the given sizes onto as few sheets as it can (largest
/// first, both 90° orientations). Parts that fit no sheet get `None`.
pub fn pack(sizes: &[(u32, u32)]) -> (Vec<Option<Placement>>, usize) {
    let s = spacing_px();
    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by_key(|&i| {
        let (w, h) = sizes[i];
        (
            std::cmp::Reverse(w.max(h)),
            std::cmp::Reverse(w as u64 * h as u64),
        )
    });
    let mut bins: Vec<Bin> = Vec::new();
    let mut out = vec![None; sizes.len()];
    for i in order {
        let (w, h) = sizes[i];
        if !fits(w, h) {
            continue;
        }
        let mut best: Option<(u32, usize, u32, u32, bool)> = None;
        for (b, bin) in bins.iter().enumerate() {
            for rotated in [false, true] {
                let (pw, ph) = if rotated { (h, w) } else { (w, h) };
                if let Some((x, y, score)) = bin.find(pw + s, ph + s) {
                    if best.is_none_or(|bst| score < bst.0) {
                        best = Some((score, b, x, y, rotated));
                    }
                }
            }
            if best.is_some() {
                break; // Fill earlier sheets first.
            }
        }
        if best.is_none() {
            bins.push(Bin::new());
            let b = bins.len() - 1;
            for rotated in [false, true] {
                let (pw, ph) = if rotated { (h, w) } else { (w, h) };
                if let Some((x, y, score)) = bins[b].find(pw + s, ph + s) {
                    if best.is_none_or(|bst| score < bst.0) {
                        best = Some((score, b, x, y, rotated));
                    }
                }
            }
        }
        let (_, b, x, y, rotated) = best.expect("fits an empty sheet");
        let (pw, ph) = if rotated { (h, w) } else { (w, h) };
        bins[b].occupy(x, y, pw + s, ph + s);
        out[i] = Some(Placement {
            sheet: b,
            x,
            y,
            rotated,
        });
    }
    (out, bins.len())
}

/// Sheets a simple shelf packing (rows left to right, height-sorted, no
/// rotation unless needed) uses: the bound `pack` must not exceed.
pub fn shelf_sheets(sizes: &[(u32, u32)]) -> usize {
    let (pw, ph) = printable_px();
    let s = spacing_px();
    let mut items: Vec<(u32, u32)> = sizes
        .iter()
        .filter(|(w, h)| fits(*w, *h))
        .map(|&(w, h)| if w <= pw && h <= ph { (w, h) } else { (h, w) })
        .collect();
    items.sort_by_key(|&(_, h)| std::cmp::Reverse(h));
    let (mut sheets, mut x, mut y, mut row_h) = (0usize, 0u32, 0u32, 0u32);
    for (w, h) in items {
        if sheets == 0 {
            sheets = 1;
        }
        if x + w > pw {
            x = 0;
            y += row_h + s;
            row_h = 0;
        }
        if y + h > ph {
            sheets += 1;
            x = 0;
            y = 0;
            row_h = 0;
        }
        x += w + s;
        row_h = row_h.max(h);
    }
    sheets
}

/// Whether a part of `size` can sit at `place` given the other placements
/// (`others`: placement and size): inside the printable area and at least
/// the spacing away from every other part on that sheet.
pub fn valid(place: Placement, size: (u32, u32), others: &[(Placement, (u32, u32))]) -> bool {
    let (pw, ph) = printable_px();
    let (x, y, w, h) = place.rect(size);
    if x + w > pw || y + h > ph {
        return false;
    }
    let s = spacing_px();
    others
        .iter()
        .filter(|(p, _)| p.sheet == place.sheet)
        .all(|(p, sz)| {
            let (ox, oy, ow, oh) = p.rect(*sz);
            x >= ox + ow + s || ox >= x + w + s || y >= oy + oh + s || oy >= y + h + s
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(sizes: &[(u32, u32)], placed: &[Option<Placement>]) {
        let all: Vec<(Placement, (u32, u32))> = placed
            .iter()
            .zip(sizes)
            .filter_map(|(p, s)| p.map(|p| (p, *s)))
            .collect();
        for (i, (p, s)) in all.iter().enumerate() {
            let others: Vec<_> = all
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, o)| *o)
                .collect();
            assert!(valid(*p, *s, &others), "{p:?} {s:?}");
        }
    }

    #[test]
    fn packs_without_overlap_and_no_worse_than_shelves() {
        let mut seed = 7u64;
        let mut rnd = |n: u32| {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) % n as u64) as u32
        };
        for _ in 0..20 {
            let sizes: Vec<(u32, u32)> = (0..30)
                .map(|_| (100 + rnd(1500), 100 + rnd(1500)))
                .collect();
            let (placed, sheets) = pack(&sizes);
            assert!(placed.iter().all(Option::is_some));
            check(&sizes, &placed);
            assert!(sheets <= shelf_sheets(&sizes), "{sheets} > shelf");
        }
    }

    #[test]
    fn oversized_parts_are_left_out_and_long_ones_turn() {
        let (pw, ph) = printable_px();
        let (placed, _) = pack(&[(ph, 200), (ph + 50, 300), (pw + 10, pw + 10)]);
        assert!(placed[0].unwrap().rotated);
        assert!(placed[1].is_none() && placed[2].is_none());
        assert!(fits(ph, 10) && !fits(ph + 1, 10));
    }
}
