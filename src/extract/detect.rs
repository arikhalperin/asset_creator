//! Connected-component object detection on a keyed (transparent) image.

use image::{GrayImage, RgbaImage};

use crate::document::PixelRect;

/// Connected-component labels: 0 for "off" pixels, 1… for regions.
pub struct Labels {
    pub label: Vec<u32>,
    /// Pixel count per label (index 0 counts nothing).
    pub sizes: Vec<u32>,
}

fn find(parent: &mut [u32], mut x: u32) -> u32 {
    while parent[x as usize] != x {
        parent[x as usize] = parent[parent[x as usize] as usize];
        x = parent[x as usize];
    }
    x
}

/// Labels the regions of pixels where `on` holds (8- or 4-connected).
pub fn label(w: u32, h: u32, on: impl Fn(u32, u32) -> bool, eight: bool) -> Labels {
    let mut provisional = vec![0u32; (w * h) as usize];
    let mut parent: Vec<u32> = vec![0];
    for y in 0..h {
        for x in 0..w {
            if !on(x, y) {
                continue;
            }
            let i = (y * w + x) as usize;
            let mut neighbours = [0u32; 4];
            let mut n = 0;
            let mut look = |nx: i64, ny: i64| {
                if nx >= 0 && ny >= 0 && (nx as u32) < w {
                    let l = provisional[(ny as u32 * w + nx as u32) as usize];
                    if l != 0 {
                        neighbours[n] = l;
                        n += 1;
                    }
                }
            };
            let (xi, yi) = (x as i64, y as i64);
            look(xi - 1, yi);
            look(xi, yi - 1);
            if eight {
                look(xi - 1, yi - 1);
                look(xi + 1, yi - 1);
            }
            if n == 0 {
                let l = parent.len() as u32;
                parent.push(l);
                provisional[i] = l;
            } else {
                let mut root = find(&mut parent, neighbours[0]);
                for &other in &neighbours[1..n] {
                    let r = find(&mut parent, other);
                    if r != root {
                        let (lo, hi) = (root.min(r), root.max(r));
                        parent[hi as usize] = lo;
                        root = lo;
                    }
                }
                provisional[i] = root;
            }
        }
    }
    // Compact roots to 1…n.
    let mut compact = vec![0u32; parent.len()];
    let mut next = 0;
    for l in 1..parent.len() as u32 {
        let r = find(&mut parent, l);
        if compact[r as usize] == 0 {
            next += 1;
            compact[r as usize] = next;
        }
        compact[l as usize] = compact[r as usize];
    }
    let mut sizes = vec![0u32; next as usize + 1];
    for l in provisional.iter_mut() {
        if *l != 0 {
            *l = compact[*l as usize];
            sizes[*l as usize] += 1;
        }
    }
    Labels {
        label: provisional,
        sizes,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DetectParams {
    /// Regions with fewer non-transparent pixels are dropped.
    pub min_size: u32,
    /// Regions whose boxes are at most this many pixels apart are merged.
    pub merge_distance: u32,
}

impl Default for DetectParams {
    fn default() -> Self {
        Self {
            min_size: 16,
            merge_distance: 2,
        }
    }
}

/// One proposed object.
#[derive(Debug, Clone, PartialEq)]
pub struct Proposal {
    /// Tight bounds of its pixels.
    pub rect: PixelRect,
    /// Coverage over `rect`: 255 on the object's pixels.
    pub mask: GrayImage,
    /// Non-transparent pixel count.
    pub pixels: u32,
}

#[derive(Debug, Clone, Copy)]
struct Group {
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
    pixels: u32,
}

impl Group {
    fn gap(&self, other: &Group) -> u32 {
        let gx = other
            .x0
            .saturating_sub(self.x1)
            .max(self.x0.saturating_sub(other.x1));
        let gy = other
            .y0
            .saturating_sub(self.y1)
            .max(self.y0.saturating_sub(other.y1));
        gx.max(gy)
    }
}

/// Finds the objects (alpha > 0 regions) of a keyed image.
pub fn detect(image: &RgbaImage, params: &DetectParams) -> Vec<Proposal> {
    let (w, h) = image.dimensions();
    let labels = label(w, h, |x, y| image.get_pixel(x, y)[3] > 0, true);
    let n = labels.sizes.len();
    let mut groups: Vec<Group> = (0..n)
        .map(|_| Group {
            x0: u32::MAX,
            y0: u32::MAX,
            x1: 0,
            y1: 0,
            pixels: 0,
        })
        .collect();
    for y in 0..h {
        for x in 0..w {
            let l = labels.label[(y * w + x) as usize] as usize;
            if l != 0 {
                let g = &mut groups[l];
                g.x0 = g.x0.min(x);
                g.y0 = g.y0.min(y);
                g.x1 = g.x1.max(x + 1);
                g.y1 = g.y1.max(y + 1);
                g.pixels += 1;
            }
        }
    }
    // Merge nearby regions until nothing changes. `owner[l]` = merged group.
    let mut owner: Vec<usize> = (0..n).collect();
    let mut alive: Vec<usize> = (1..n).collect();
    loop {
        let mut merged = false;
        'outer: for i in 0..alive.len() {
            for j in i + 1..alive.len() {
                let (a, b) = (alive[i], alive[j]);
                if groups[a].gap(&groups[b]) <= params.merge_distance {
                    let gb = groups[b];
                    let ga = &mut groups[a];
                    ga.x0 = ga.x0.min(gb.x0);
                    ga.y0 = ga.y0.min(gb.y0);
                    ga.x1 = ga.x1.max(gb.x1);
                    ga.y1 = ga.y1.max(gb.y1);
                    ga.pixels += gb.pixels;
                    for o in owner.iter_mut() {
                        if *o == b {
                            *o = a;
                        }
                    }
                    alive.remove(j);
                    merged = true;
                    break 'outer;
                }
            }
        }
        if !merged {
            break;
        }
    }
    let mut proposals: Vec<(usize, Proposal)> = alive
        .iter()
        .filter(|&&g| groups[g].pixels >= params.min_size.max(1))
        .map(|&g| {
            let gr = groups[g];
            let rect = PixelRect::new(gr.x0, gr.y0, gr.x1 - gr.x0, gr.y1 - gr.y0);
            let mut mask = GrayImage::new(rect.width, rect.height);
            for y in gr.y0..gr.y1 {
                for x in gr.x0..gr.x1 {
                    let l = labels.label[(y * w + x) as usize] as usize;
                    if l != 0 && owner[l] == g {
                        mask.put_pixel(x - gr.x0, y - gr.y0, image::Luma([255]));
                    }
                }
            }
            (
                g,
                Proposal {
                    rect,
                    mask,
                    pixels: gr.pixels,
                },
            )
        })
        .collect();
    proposals.sort_by_key(|(_, p)| (p.rect.y, p.rect.x));
    proposals.into_iter().map(|(_, p)| p).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn blob(img: &mut RgbaImage, x0: u32, y0: u32, w: u32, h: u32) {
        for y in y0..y0 + h {
            for x in x0..x0 + w {
                img.put_pixel(x, y, Rgba([200, 10, 10, 255]));
            }
        }
    }

    #[test]
    fn finds_objects_merges_nearby_parts_and_drops_specks() {
        let mut img = RgbaImage::new(100, 60);
        blob(&mut img, 50, 5, 10, 10);
        blob(&mut img, 5, 5, 10, 10);
        blob(&mut img, 17, 8, 2, 2); // 2 px gap from the first blob: merged
        blob(&mut img, 5, 40, 20, 5);
        img.put_pixel(90, 50, Rgba([1, 1, 1, 255])); // speck
        let found = detect(&img, &DetectParams::default());
        let rects: Vec<PixelRect> = found.iter().map(|p| p.rect).collect();
        assert_eq!(
            rects,
            vec![
                PixelRect::new(5, 5, 14, 10),
                PixelRect::new(50, 5, 10, 10),
                PixelRect::new(5, 40, 20, 5)
            ]
        );
        assert_eq!(found[0].pixels, 104);
        assert_eq!(found[0].mask.get_pixel(13, 4)[0], 255);
        assert_eq!(
            found[0].mask.get_pixel(11, 0)[0],
            0,
            "gap is not part of it"
        );
        let apart = detect(
            &img,
            &DetectParams {
                min_size: 1,
                merge_distance: 0,
            },
        );
        assert_eq!(apart.len(), 5, "no merging, speck kept");
    }

    #[test]
    fn diagonal_pixels_are_connected() {
        let mut img = RgbaImage::new(5, 5);
        for i in 0..5 {
            img.put_pixel(i, i, Rgba([9, 9, 9, 255]));
        }
        let l = label(5, 5, |x, y| img.get_pixel(x, y)[3] > 0, true);
        assert_eq!(l.sizes.len(), 2);
        let l4 = label(5, 5, |x, y| img.get_pixel(x, y)[3] > 0, false);
        assert_eq!(l4.sizes.len(), 6);
    }
}
