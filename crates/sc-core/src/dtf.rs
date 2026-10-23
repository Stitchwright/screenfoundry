//! Direct-to-film (DTF) files: the colour art mirrored for the film, and a white layer under it.
//!
//! A DTF printer prints the colours onto film, then white over them, and the print is transferred
//! to the garment, so the art goes on the film as a mirror image and the white is printed last
//! over the colours. Shops feed the printer's RIP a transparent PNG; whether the RIP makes the
//! white itself or takes it as a second file varies, so both files are made. The white layer is
//! the art's footprint pulled in a little (the choke), so no white shows past the colour edge.
//!
//! The research behind this is thin (one vendor page: transparent PNG or TIFF at 300 dpi, the RIP
//! mirroring and adding white). The choke default and the white as a plain white-on-transparent
//! PNG are choices, not a standard: they are options, and a shop's RIP decides what it needs.

use crate::error::Result;
use crate::image::{Gray8, Rgba8};
use crate::morph::choke;
use crate::separate::ALPHA_CUTOFF;

/// What to make.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DtfOptions {
    /// Flip the art left to right, as it must go on the film.
    pub mirror: bool,
    /// Make the white layer.
    pub white: bool,
    /// How far the white is pulled in from the colour's edges, in pixels.
    pub white_choke_px: f64,
    /// Graded white: inside the footprint the white is as opaque as the art is (soft edges and
    /// fades keep a matching white); otherwise it is solid wherever the art is.
    pub graded: bool,
}

/// The files of a DTF job: both the same size.
#[derive(Clone, Debug, PartialEq)]
pub struct DtfFiles {
    /// The colour art, mirrored when asked, transparent where there is none.
    pub color: Rgba8,
    /// White where white ink prints (opaque white, transparent elsewhere), when asked.
    pub white: Option<Rgba8>,
}

fn mirrored(img: &Rgba8) -> Result<Rgba8> {
    let (w, h) = (img.width(), img.height());
    let mut out = Vec::with_capacity(img.data().len());
    for row in img.data().chunks_exact(w * 4) {
        for px in row.chunks_exact(4).rev() {
            out.extend_from_slice(px);
        }
    }
    Rgba8::new(w, h, out)
}

/// The colour file and white layer for `img`.
pub fn prepare(img: &Rgba8, o: &DtfOptions) -> Result<DtfFiles> {
    let color = if o.mirror { mirrored(img)? } else { img.clone() };
    let white = if o.white {
        let footprint = Gray8::new(img.width(), img.height(), img.pixels().map(|p| if p[3] >= ALPHA_CUTOFF { 255 } else { 0 }).collect())?;
        let plate = choke(&footprint, o.white_choke_px)?;
        let mut px: Vec<u8> = plate
            .data()
            .iter()
            .zip(img.pixels())
            .flat_map(|(&v, art)| {
                [
                    255,
                    255,
                    255,
                    if v < 128 {
                        0
                    } else if o.graded {
                        art[3]
                    } else {
                        255
                    },
                ]
            })
            .collect();
        let layer = Rgba8::new(img.width(), img.height(), std::mem::take(&mut px))?;
        Some(if o.mirror { mirrored(&layer)? } else { layer })
    } else {
        None
    };
    Ok(DtfFiles { color, white })
}

/// Most designs a gang sheet holds.
pub const MAX_DESIGNS: usize = 200;

/// Where one design went on the sheet, in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    /// The design's position in the list given.
    pub index: usize,
    pub x: usize,
    pub y: usize,
    /// The size on the sheet (swapped from the design's own when it was turned).
    pub width: usize,
    pub height: usize,
    /// Turned a quarter turn clockwise.
    pub rotated: bool,
}

/// `img` turned a quarter turn clockwise.
fn rotated_cw(img: &Rgba8) -> Result<Rgba8> {
    let (w, h) = (img.width(), img.height());
    let mut out = vec![0u8; img.data().len()];
    for (i, px) in img.data().chunks_exact(4).enumerate() {
        let (x, y) = (i % w, i / w);
        let at = (x * h + (h - 1 - y)) * 4;
        if let Some(dst) = out.get_mut(at..at + 4) {
            dst.copy_from_slice(px);
        }
    }
    Rgba8::new(h, w, out)
}

/// Rows of items `sizes` apart by `gap` on a sheet `sheet_w` wide, tallest first, left to right, a
/// new row when the next does not fit: where each goes (x, y) in the order given, and the height
/// the rows need. `None` when an item is wider than the sheet.
fn shelf(sizes: &[(usize, usize)], sheet_w: usize, gap: usize) -> Option<(Vec<(usize, usize)>, usize)> {
    if sizes.iter().any(|&(w, _)| w > sheet_w) {
        return None;
    }
    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(sizes.get(i).map_or(0, |s| s.1)));
    let (mut x, mut y, mut row_h) = (0usize, 0usize, 0usize);
    let mut at = vec![(0, 0); sizes.len()];
    for &i in &order {
        let &(w, h) = sizes.get(i)?;
        if x > 0 && x.saturating_add(w) > sheet_w {
            y = y.saturating_add(row_h).saturating_add(gap);
            (x, row_h) = (0, 0);
        }
        *at.get_mut(i)? = (x, y);
        x = x.saturating_add(w).saturating_add(gap);
        row_h = row_h.max(h);
    }
    Some((at, y.saturating_add(row_h)))
}

/// Free-rectangle packing (MaxRects, best short side fit) of items `sizes` apart by `gap` into a
/// sheet `sheet_w` wide and at most `max_h` tall: where each goes (x, y) in the order given and the
/// height used. Items are placed largest first into the free space they leave the least room over,
/// so small designs fill the gaps beside tall ones that rows would leave empty. `None` when they
/// do not all fit.
fn free_rects(sizes: &[(usize, usize)], sheet_w: usize, max_h: usize, gap: usize) -> Option<(Vec<(usize, usize)>, usize)> {
    #[derive(Clone, Copy)]
    struct R {
        x: usize,
        y: usize,
        w: usize,
        h: usize,
    }
    // Work with each item grown by the gap on the right and below, in a bin grown the same way.
    let bin = R { x: 0, y: 0, w: sheet_w.checked_add(gap)?, h: max_h.checked_add(gap)? };
    let mut free = vec![bin];
    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(sizes.get(i).map_or(0, |s| s.0.saturating_mul(s.1))));
    let mut at = vec![(0, 0); sizes.len()];
    let mut used = 0usize;
    for &i in &order {
        let &(w, h) = sizes.get(i)?;
        let (gw, gh) = (w.checked_add(gap)?, h.checked_add(gap)?);
        // The free rectangle with the least room left on its short side, then the highest and leftmost.
        let spot =
            free.iter().filter(|f| f.w >= gw && f.h >= gh).min_by_key(|f| ((f.w - gw).min(f.h - gh), f.y, f.x, (f.w - gw).max(f.h - gh))).copied()?;
        let placed = R { x: spot.x, y: spot.y, w: gw, h: gh };
        *at.get_mut(i)? = (placed.x, placed.y);
        used = used.max(placed.y + h);
        // Cut the placed item out of every free rectangle it touches.
        let mut next: Vec<R> = Vec::with_capacity(free.len() + 4);
        for f in &free {
            let apart = placed.x >= f.x + f.w || placed.x + placed.w <= f.x || placed.y >= f.y + f.h || placed.y + placed.h <= f.y;
            if apart {
                next.push(*f);
                continue;
            }
            if placed.x > f.x {
                next.push(R { w: placed.x - f.x, ..*f });
            }
            if placed.x + placed.w < f.x + f.w {
                next.push(R { x: placed.x + placed.w, w: f.x + f.w - placed.x - placed.w, ..*f });
            }
            if placed.y > f.y {
                next.push(R { h: placed.y - f.y, ..*f });
            }
            if placed.y + placed.h < f.y + f.h {
                next.push(R { y: placed.y + placed.h, h: f.y + f.h - placed.y - placed.h, ..*f });
            }
        }
        // Drop rectangles lying inside another.
        let all = next.clone();
        free = next
            .into_iter()
            .enumerate()
            .filter(|(k, a)| {
                !all.iter().enumerate().any(|(j, b)| {
                    j != *k
                        && a.x >= b.x
                        && a.y >= b.y
                        && a.x + a.w <= b.x + b.w
                        && a.y + a.h <= b.y + b.h
                        && (j < *k || !(a.x == b.x && a.y == b.y && a.w == b.w && a.h == b.h))
                })
            })
            .map(|(_, r)| r)
            .collect();
    }
    Some((at, used))
}

/// One way of laying the designs out: which are turned, where each goes and how tall the rows are.
struct Layout {
    turned: Vec<bool>,
    at: Vec<(usize, usize)>,
    height: usize,
}

/// Lay `designs` out on one transparent sheet `sheet_w` pixels wide, `gap` pixels apart, in rows
/// (shelf packing): tallest first, left to right, a new row when the next design does not fit; when
/// free-rectangle packing (small designs filling the space beside tall ones) makes the sheet shorter, that layout is used. The
/// sheet is as tall as the rows need. With `rotate` each of three layouts is tried (every design
/// as it is, every design turned to lie wider than tall, every design turned to stand taller
/// than wide, where it fits) and the one that makes the shortest sheet is used, so turning never
/// does worse than not turning; a design too wide for the sheet may be turned to fit. Placements
/// come back in the order the designs were given.
pub fn gang_sheet(designs: &[Rgba8], sheet_w: usize, gap: usize, rotate: bool) -> Result<(Rgba8, Vec<Placement>)> {
    use crate::error::param;
    if designs.is_empty() || designs.len() > MAX_DESIGNS {
        return Err(param("designs", format!("a gang sheet holds 1 to {MAX_DESIGNS} designs")));
    }
    if sheet_w == 0 {
        return Err(param("sheet_w", "the sheet has no width"));
    }
    let own: Vec<(usize, usize)> = designs.iter().map(|d| (d.width(), d.height())).collect();
    // Each candidate: for every design whether it is turned.
    let mut candidates: Vec<Vec<bool>> = vec![vec![false; designs.len()]];
    if rotate {
        // Turn a design when that gives the wanted shape and it still fits across the sheet.
        let wanted = |landscape: bool| -> Vec<bool> {
            own.iter().map(|&(w, h)| if landscape { h > w && h <= sheet_w } else { w > h && h <= sheet_w }).collect()
        };
        candidates.push(wanted(true));
        candidates.push(wanted(false));
        // A design wider than the sheet can only go turned.
        candidates.push(own.iter().map(|&(w, _)| w > sheet_w).collect());
    }
    let size_of = |turned: &[bool]| -> Vec<(usize, usize)> { own.iter().zip(turned).map(|(&(w, h), &t)| if t { (h, w) } else { (w, h) }).collect() };
    let mut best: Option<Layout> = None;
    for turned in candidates {
        let sizes = size_of(&turned);
        let Some((at, height)) = shelf(&sizes, sheet_w, gap) else { continue };
        // Free-rectangle packing inside the height the rows need can only do as well or better.
        let (at, height) = match free_rects(&sizes, sheet_w, height, gap) {
            Some((packed, h)) if h < height => (packed, h),
            _ => (at, height),
        };
        if best.as_ref().is_none_or(|b| height < b.height) {
            best = Some(Layout { turned, at, height });
        }
    }
    let Some(Layout { turned, at, height: sheet_h }) = best else {
        let (i, d) = designs.iter().enumerate().find(|(_, d)| d.width() > sheet_w).map_or((0, 0), |(i, d)| (i, d.width()));
        return Err(param("sheet_w", format!("design {} is {d} px wide and the sheet {sheet_w}", i + 1)));
    };
    if sheet_w.checked_mul(sheet_h).is_none_or(|n| n > crate::image::MAX_PIXELS) {
        return Err(param("sheet_w", "the sheet would be too large: use a narrower sheet, a smaller gap or a lower resolution"));
    }
    let mut data = vec![0u8; sheet_w * sheet_h * 4];
    let sizes = size_of(&turned);
    let mut placed = Vec::with_capacity(designs.len());
    for (i, design) in designs.iter().enumerate() {
        let (&(x, y), &(width, height), &rotated) = (
            at.get(i).ok_or_else(|| param("designs", "internal: no place"))?,
            sizes.get(i).ok_or_else(|| param("designs", "internal: no size"))?,
            turned.get(i).ok_or_else(|| param("designs", "internal: no turn"))?,
        );
        let art = if rotated { rotated_cw(design)? } else { design.clone() };
        for (row, src) in art.data().chunks_exact(width * 4).enumerate() {
            let from = ((y + row) * sheet_w + x) * 4;
            if let Some(dst) = data.get_mut(from..from + src.len()) {
                dst.copy_from_slice(src);
            }
        }
        placed.push(Placement { index: i, x, y, width, height, rotated });
    }
    Ok((Rgba8::new(sheet_w, sheet_h, data)?, placed))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 12 × 6: red in the left half, blue in the right, a transparent column at the far right.
    fn art() -> Rgba8 {
        let mut d = Vec::new();
        for _ in 0..6 {
            for x in 0..12 {
                d.extend_from_slice(&match x {
                    0..=5 => [200, 30, 30, 255],
                    6..=10 => [20, 40, 170, 255],
                    _ => [0, 0, 0, 0],
                });
            }
        }
        Rgba8::new(12, 6, d).unwrap()
    }

    fn opts(mirror: bool, choke: f64) -> DtfOptions {
        DtfOptions { mirror, white: true, white_choke_px: choke, graded: false }
    }

    #[test]
    fn the_colour_file_is_the_art_flipped_left_to_right() {
        let f = prepare(&art(), &opts(true, 0.0)).unwrap();
        assert_eq!(f.color.pixel(0, 0), Some([0, 0, 0, 0]), "the transparent column is now at the left");
        assert_eq!(f.color.pixel(1, 3), Some([20, 40, 170, 255]));
        assert_eq!(f.color.pixel(11, 3), Some([200, 30, 30, 255]));
        let flat = prepare(&art(), &opts(false, 0.0)).unwrap();
        assert_eq!(flat.color, art());
        // Mirroring twice gives the art back.
        assert_eq!(mirrored(&f.color).unwrap(), art());
    }

    #[test]
    fn the_white_layer_is_the_footprint_pulled_in_and_mirrored_with_the_colour() {
        let f = prepare(&art(), &opts(false, 0.0)).unwrap();
        let w = f.white.unwrap();
        assert_eq!(w.pixel(3, 2), Some([255, 255, 255, 255]));
        assert_eq!(w.pixel(11, 2), Some([255, 255, 255, 0]), "no white where there is no art");
        // A 1.5 px choke takes the white in from the art's edge (and the canvas edge counts as empty).
        let c = prepare(&art(), &opts(false, 1.5)).unwrap().white.unwrap();
        assert_eq!(c.pixel(0, 3).map(|p| p[3]), Some(0));
        assert_eq!(c.pixel(10, 3).map(|p| p[3]), Some(0), "pulled in from the transparent column");
        assert_eq!(c.pixel(5, 3).map(|p| p[3]), Some(255), "inside the art");
        // Mirrored together: white stays under its colour.
        let m = prepare(&art(), &opts(true, 1.5)).unwrap();
        let (col, wh) = (m.color, m.white.unwrap());
        for y in 0..6 {
            for x in 0..12 {
                if wh.pixel(x, y).unwrap()[3] == 255 {
                    assert_eq!(col.pixel(x, y).unwrap()[3], 255, "white without colour at ({x}, {y})");
                }
            }
        }
        assert!(wh.pixel(6, 3).unwrap()[3] == 255);
    }

    #[test]
    fn no_white_when_not_asked() {
        let f = prepare(&art(), &DtfOptions { mirror: false, white: false, white_choke_px: 1.0, graded: false }).unwrap();
        assert!(f.white.is_none());
    }

    #[test]
    fn graded_white_follows_the_art_opacity_inside_the_footprint() {
        // A fade: alpha 255, 200, 140 and 100 (under the cutoff of 128), red.
        let img = Rgba8::new(4, 1, [255u8, 200, 140, 100].iter().flat_map(|&a| [200, 30, 30, a]).collect()).unwrap();
        let g = prepare(&img, &DtfOptions { mirror: false, white: true, white_choke_px: 0.0, graded: true }).unwrap().white.unwrap();
        assert_eq!((0..4).map(|x| g.pixel(x, 0).unwrap()[3]).collect::<Vec<_>>(), [255, 200, 140, 0], "no white where the art is under half opaque");
        let solid = prepare(&img, &DtfOptions { mirror: false, white: true, white_choke_px: 0.0, graded: false }).unwrap().white.unwrap();
        assert_eq!((0..4).map(|x| solid.pixel(x, 0).unwrap()[3]).collect::<Vec<_>>(), [255, 255, 255, 0]);
    }

    fn block(w: usize, h: usize, v: u8) -> Rgba8 {
        Rgba8::new(w, h, (0..w * h).flat_map(|_| [v, v, v, 255]).collect()).unwrap()
    }

    #[test]
    fn a_gang_sheet_packs_rows_tallest_first_with_gaps_and_reports_where() {
        // Widths 40, 30, 50, 20 and heights 10, 30, 20, 30 on a 100 wide sheet with 5 gaps.
        let d = [block(40, 10, 1), block(30, 30, 2), block(50, 20, 3), block(20, 30, 4)];
        let (sheet, at) = gang_sheet(&d, 100, 5, false).unwrap();
        // Tallest first: designs 2 and 4 (30 high) then 3, then 1. Row 1: 30 + 5 + 20 = 55 wide, then
        // design 3 (50) does not fit in the last 40: row 2 holds 3 and 1 (50 + 5 + 40 = 95).
        assert_eq!(
            at,
            [
                Placement { index: 0, x: 55, y: 35, width: 40, height: 10, rotated: false },
                Placement { index: 1, x: 0, y: 0, width: 30, height: 30, rotated: false },
                Placement { index: 2, x: 0, y: 35, width: 50, height: 20, rotated: false },
                Placement { index: 3, x: 35, y: 0, width: 20, height: 30, rotated: false },
            ]
        );
        assert_eq!((sheet.width(), sheet.height()), (100, 55));
        assert_eq!(sheet.pixel(10, 10), Some([2, 2, 2, 255]));
        assert_eq!(sheet.pixel(40, 10), Some([4, 4, 4, 255]));
        assert_eq!(sheet.pixel(60, 40), Some([1, 1, 1, 255]));
        assert_eq!(sheet.pixel(32, 10), Some([0, 0, 0, 0]), "the gap is clear");
        // No two designs overlap.
        for (i, a) in at.iter().enumerate() {
            for b in &at[i + 1..] {
                assert!(a.x + a.width <= b.x || b.x + b.width <= a.x || a.y + a.height <= b.y || b.y + b.height <= a.y, "{a:?} {b:?}");
            }
        }
    }

    #[test]
    fn small_designs_fill_the_space_beside_a_tall_one() {
        // Rows would stack the third design under the first row (height 150); packed, it sits beside the tall one.
        let designs = [block(60, 100, 1), block(40, 50, 2), block(40, 50, 3)];
        let (sheet, placed) = gang_sheet(&designs, 100, 0, false).unwrap();
        assert_eq!(sheet.height(), 100, "{placed:?}");
        // Nothing overlaps and everything is inside the sheet.
        for (i, a) in placed.iter().enumerate() {
            assert!(a.x + a.width <= 100 && a.y + a.height <= 100, "{a:?}");
            for b in &placed[i + 1..] {
                let apart = a.x + a.width <= b.x || b.x + b.width <= a.x || a.y + a.height <= b.y || b.y + b.height <= a.y;
                assert!(apart, "{a:?} overlaps {b:?}");
            }
        }
        // Placements still come back in the order the designs were given.
        assert_eq!(placed.iter().map(|p| p.index).collect::<Vec<_>>(), [0, 1, 2]);
    }

    #[test]
    fn packing_never_loses_designs_or_overlaps_with_gaps() {
        let designs: Vec<Rgba8> = [(30, 40), (25, 25), (50, 10), (10, 60), (40, 40), (20, 35), (15, 15)]
            .iter()
            .enumerate()
            .map(|(i, &(w, h))| block(w, h, i as u8 + 1))
            .collect();
        let (sheet, placed) = gang_sheet(&designs, 90, 4, true).unwrap();
        assert_eq!(placed.len(), 7);
        for (i, a) in placed.iter().enumerate() {
            assert!(a.x + a.width <= sheet.width() && a.y + a.height <= sheet.height(), "{a:?}");
            for b in &placed[i + 1..] {
                // At least the gap apart on one axis.
                let apart = a.x + a.width + 4 <= b.x || b.x + b.width + 4 <= a.x || a.y + a.height + 4 <= b.y || b.y + b.height + 4 <= a.y;
                assert!(apart, "{a:?} too near {b:?}");
            }
        }
    }

    #[test]
    fn gang_sheets_refuse_what_does_not_fit() {
        assert!(gang_sheet(&[], 100, 5, false).is_err());
        assert!(gang_sheet(&[block(10, 10, 0)], 0, 5, false).is_err());
        assert!(gang_sheet(&[block(101, 10, 0)], 100, 5, false).is_err(), "wider than the sheet");
        assert!(gang_sheet(&[block(10, 10, 0)], usize::MAX, 0, false).is_err(), "too large a sheet");
        assert!(gang_sheet(&vec![block(1, 1, 0); MAX_DESIGNS + 1], 100, 0, false).is_err());
        let (one, _) = gang_sheet(&[block(7, 3, 9)], 7, 100, false).unwrap();
        assert_eq!((one.width(), one.height()), (7, 3));
    }

    #[test]
    fn a_quarter_turn_is_clockwise_and_turning_four_times_gives_the_art_back() {
        // 3 wide, 2 high: row 0 is a b c, row 1 is d e f.
        let img = Rgba8::new(3, 2, (0..6u8).flat_map(|i| [i, 0, 0, 255]).collect()).unwrap();
        let r = rotated_cw(&img).unwrap();
        assert_eq!((r.width(), r.height()), (2, 3));
        // Clockwise: the left column read bottom to top becomes the top row: d a / e b / f c.
        let reds: Vec<u8> = (0..3).flat_map(|y| (0..2).map(move |x| (x, y))).map(|(x, y)| r.pixel(x, y).unwrap()[0]).collect();
        assert_eq!(reds, [3, 0, 4, 1, 5, 2]);
        let back = rotated_cw(&rotated_cw(&rotated_cw(&r).unwrap()).unwrap()).unwrap();
        assert_eq!(back, img);
    }

    #[test]
    fn turning_designs_makes_a_shorter_sheet_when_it_can_and_never_a_longer_one() {
        // Four 70 × 20 designs on a 100 wide sheet: lying down only one fits a row (height 95 with
        // gaps of 5), but standing up (20 × 70) four fit across (height 70).
        let d = vec![block(70, 20, 1); 4];
        let (flat, at) = gang_sheet(&d, 100, 5, false).unwrap();
        assert_eq!(flat.height(), 95);
        assert!(at.iter().all(|p| !p.rotated));
        let (turned, at) = gang_sheet(&d, 100, 5, true).unwrap();
        assert_eq!((turned.width(), turned.height()), (100, 70));
        assert!(at.iter().all(|p| p.rotated && p.width == 20 && p.height == 70), "{at:?}");
        // The artwork is in there turned: the design is solid, so its box is filled.
        let p = at[0];
        assert_eq!(turned.pixel(p.x + 3, p.y + 60), Some([1, 1, 1, 255]));
        // The same set that packed well stays as it was.
        let d = [block(40, 10, 1), block(30, 30, 2), block(50, 20, 3), block(20, 30, 4)];
        let (a, _) = gang_sheet(&d, 100, 5, false).unwrap();
        let (b, _) = gang_sheet(&d, 100, 5, true).unwrap();
        assert!(b.height() <= a.height());
        // A design wider than the sheet goes if it can be turned to fit.
        assert!(gang_sheet(&[block(150, 40, 0)], 100, 5, false).is_err());
        let (_, at) = gang_sheet(&[block(150, 40, 0)], 100, 5, true).unwrap();
        assert_eq!((at[0].rotated, at[0].width, at[0].height), (true, 40, 150));
        assert!(gang_sheet(&[block(150, 120, 0)], 100, 5, true).is_err(), "too big turned or not");
    }
}
