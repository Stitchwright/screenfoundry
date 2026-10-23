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
    pub width: usize,
    pub height: usize,
}

/// Lay `designs` out on one transparent sheet `sheet_w` pixels wide, `gap` pixels apart, in rows
/// (shelf packing): tallest first, left to right, a new row when the next design does not fit.
/// The sheet is as tall as the rows need. Designs are not rotated. Placements come back in the
/// order the designs were given.
pub fn gang_sheet(designs: &[Rgba8], sheet_w: usize, gap: usize) -> Result<(Rgba8, Vec<Placement>)> {
    use crate::error::param;
    if designs.is_empty() || designs.len() > MAX_DESIGNS {
        return Err(param("designs", format!("a gang sheet holds 1 to {MAX_DESIGNS} designs")));
    }
    if sheet_w == 0 {
        return Err(param("sheet_w", "the sheet has no width"));
    }
    if let Some((i, d)) = designs.iter().enumerate().find(|(_, d)| d.width() > sheet_w) {
        return Err(param("sheet_w", format!("design {} is {} px wide and the sheet {sheet_w}", i + 1, d.width())));
    }
    let mut order: Vec<usize> = (0..designs.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(designs.get(i).map_or(0, Rgba8::height)));
    let (mut x, mut y, mut row_h) = (0usize, 0usize, 0usize);
    let mut placed = Vec::with_capacity(designs.len());
    for &i in &order {
        let Some(d) = designs.get(i) else { continue };
        if x > 0 && x.saturating_add(d.width()) > sheet_w {
            y = y.saturating_add(row_h).saturating_add(gap);
            (x, row_h) = (0, 0);
        }
        placed.push(Placement { index: i, x, y, width: d.width(), height: d.height() });
        x = x.saturating_add(d.width()).saturating_add(gap);
        row_h = row_h.max(d.height());
    }
    let sheet_h = y.saturating_add(row_h);
    if sheet_w.checked_mul(sheet_h).is_none_or(|n| n > crate::image::MAX_PIXELS) {
        return Err(param("sheet_w", "the sheet would be too large: use a narrower sheet, a smaller gap or a lower resolution"));
    }
    let mut data = vec![0u8; sheet_w * sheet_h * 4];
    for pl in &placed {
        let Some(d) = designs.get(pl.index) else { continue };
        for (row, src) in d.data().chunks_exact(d.width() * 4).enumerate() {
            let at = ((pl.y + row) * sheet_w + pl.x) * 4;
            if let Some(dst) = data.get_mut(at..at + src.len()) {
                dst.copy_from_slice(src);
            }
        }
    }
    placed.sort_by_key(|p| p.index);
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
        let (sheet, at) = gang_sheet(&d, 100, 5).unwrap();
        // Tallest first: designs 2 and 4 (30 high) then 3, then 1. Row 1: 30 + 5 + 20 = 55 wide, then
        // design 3 (50) does not fit in the last 40: row 2 holds 3 and 1 (50 + 5 + 40 = 95).
        assert_eq!(
            at,
            [
                Placement { index: 0, x: 55, y: 35, width: 40, height: 10 },
                Placement { index: 1, x: 0, y: 0, width: 30, height: 30 },
                Placement { index: 2, x: 0, y: 35, width: 50, height: 20 },
                Placement { index: 3, x: 35, y: 0, width: 20, height: 30 },
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
    fn gang_sheets_refuse_what_does_not_fit() {
        assert!(gang_sheet(&[], 100, 5).is_err());
        assert!(gang_sheet(&[block(10, 10, 0)], 0, 5).is_err());
        assert!(gang_sheet(&[block(101, 10, 0)], 100, 5).is_err(), "wider than the sheet");
        assert!(gang_sheet(&[block(10, 10, 0)], usize::MAX, 0).is_err(), "too large a sheet");
        assert!(gang_sheet(&vec![block(1, 1, 0); MAX_DESIGNS + 1], 100, 0).is_err());
        let (one, _) = gang_sheet(&[block(7, 3, 9)], 7, 100).unwrap();
        assert_eq!((one.width(), one.height()), (7, 3));
    }
}
