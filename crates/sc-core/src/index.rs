//! Index-colour separation: full-colour art reduced to a fixed palette of inks, one ink per square
//! pixel, with no overlap and no halftone dots.
//!
//! The art is averaged into square cells (a cell is a few film pixels wide, big enough to hold on
//! the mesh), and each cell takes the palette colour nearest to its own colour plus the error its
//! neighbours left behind (error diffusion, Floyd-Steinberg by default, serpentine, in linear light so the
//! average of the cells matches the art's brightness). The palette is the base (the colour under
//! the pixels: a white underbase, or the garment) followed by the inks. A cell that takes the base
//! prints no ink.
//!
//! `dither` scales the diffused error: 0 maps every cell to its nearest palette colour (flat
//! posterised art), 1 is full error diffusion (photographs).

use crate::error::{Result, param};
use crate::fm::Kernel;
use crate::image::{Bitmap1, Rgba8};
use crate::separate::ALPHA_CUTOFF;
use crate::simulated::to_linear;

/// Most inks an index-colour job separates (the base is extra).
pub const MAX_INDEX_INKS: usize = 12;

/// Largest cell, in film pixels, on a side.
pub const MAX_CELL_PX: usize = 256;

/// A carried error never pulls a cell further than this per channel, so colours the palette cannot
/// reach do not bounce error across the whole image.
const MAX_ERROR: f64 = 0.5;

fn dist2(a: &[f64; 3], b: &[f64; 3]) -> f64 {
    a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum()
}

/// For each cell of the grid, the palette index chosen, or `None` for a cell with no art.
struct Grid {
    cw: usize,
    ch: usize,
    index: Vec<Option<usize>>,
}

/// Average `img` into `cell`-pixel squares: the mean linear colour of each cell's opaque pixels,
/// `None` where fewer than half its pixels are opaque.
fn cells(img: &Rgba8, cell: usize) -> (usize, usize, Vec<Option<[f64; 3]>>) {
    let (w, h) = (img.width(), img.height());
    let (cw, ch) = (w.div_ceil(cell), h.div_ceil(cell));
    let mut sum = vec![[0.0f64; 3]; cw * ch];
    let mut opaque = vec![0usize; cw * ch];
    let mut area = vec![0usize; cw * ch];
    for (i, p) in img.pixels().enumerate() {
        let c = (i / w) / cell * cw + (i % w) / cell;
        if let Some(a) = area.get_mut(c) {
            *a += 1;
        }
        if p[3] < ALPHA_CUTOFF {
            continue;
        }
        if let (Some(s), Some(o)) = (sum.get_mut(c), opaque.get_mut(c)) {
            for (acc, v) in s.iter_mut().zip(to_linear([p[0], p[1], p[2]])) {
                *acc += v;
            }
            *o += 1;
        }
    }
    let out = sum
        .iter()
        .zip(opaque.iter().zip(&area))
        .map(|(s, (&o, &a))| (o > 0 && o * 2 >= a).then(|| [s[0] / o as f64, s[1] / o as f64, s[2] / o as f64]))
        .collect();
    (cw, ch, out)
}

fn dither_cells(cw: usize, ch: usize, colors: &[Option<[f64; 3]>], palette: &[[f64; 3]], strength: f64, kernel: Kernel) -> Grid {
    let taps = kernel.taps();
    let mut err = vec![[0.0f64; 3]; cw * ch];
    let mut index = vec![None; cw * ch];
    for y in 0..ch {
        let ltr = y % 2 == 0;
        for step in 0..cw {
            let x = if ltr { step } else { cw - 1 - step };
            let i = y * cw + x;
            let Some(Some(c)) = colors.get(i) else { continue };
            let e = err.get(i).copied().unwrap_or_default();
            let want = [(c[0] + e[0]).clamp(0.0, 1.0), (c[1] + e[1]).clamp(0.0, 1.0), (c[2] + e[2]).clamp(0.0, 1.0)];
            let Some((best, chosen)) = palette.iter().enumerate().min_by(|a, b| dist2(&want, a.1).total_cmp(&dist2(&want, b.1))) else {
                continue;
            };
            if let Some(slot) = index.get_mut(i) {
                *slot = Some(best);
            }
            let miss: [f64; 3] = std::array::from_fn(|k| (want[k] - chosen[k]) * strength);
            for &(dx, dy, wt) in &taps {
                // Mirror the horizontal step on right-to-left rows.
                let dx = if ltr { dx } else { -dx };
                let (nx, ny) = (x as isize + dx, y + dy);
                if nx < 0 || nx as usize >= cw || ny >= ch {
                    continue;
                }
                let n = ny * cw + nx as usize;
                // Error only flows into cells that have art.
                if !matches!(colors.get(n), Some(Some(_))) {
                    continue;
                }
                if let Some(slot) = err.get_mut(n) {
                    for (s, m) in slot.iter_mut().zip(miss) {
                        *s = (*s + m * wt).clamp(-MAX_ERROR, MAX_ERROR);
                    }
                }
            }
        }
    }
    Grid { cw, ch, index }
}

/// One film bitmap per ink: the cells (expanded back to `cell`-pixel squares) that took that ink.
/// `base` is the colour under the pixels and takes no film. The films never overlap.
pub fn index_films(img: &Rgba8, inks: &[[u8; 3]], base: [u8; 3], cell_px: usize, dither: f64, kernel: Kernel) -> Result<Vec<Bitmap1>> {
    if inks.is_empty() || inks.len() > MAX_INDEX_INKS {
        return Err(param("inks", format!("need 1..={MAX_INDEX_INKS} inks")));
    }
    if cell_px == 0 || cell_px > MAX_CELL_PX {
        return Err(param("cell_px", format!("must be 1..={MAX_CELL_PX}")));
    }
    if !dither.is_finite() || !(0.0..=1.0).contains(&dither) {
        return Err(param("dither", "must be 0..=1"));
    }
    let mut palette = vec![to_linear(base)];
    palette.extend(inks.iter().map(|&c| to_linear(c)));
    let (cw, ch, colors) = cells(img, cell_px);
    let grid = dither_cells(cw, ch, &colors, &palette, dither, kernel);
    let (w, h) = (img.width(), img.height());
    let mut films = Vec::with_capacity(inks.len());
    for ink in 1..=inks.len() {
        let mut bits = Vec::with_capacity(w * h);
        for y in 0..h {
            let row = (y / cell_px).min(grid.ch.saturating_sub(1)) * grid.cw;
            for x in 0..w {
                let c = row + (x / cell_px).min(grid.cw.saturating_sub(1));
                bits.push(grid.index.get(c).copied().flatten() == Some(ink));
            }
        }
        films.push(Bitmap1::from_bits(w, h, bits)?);
    }
    Ok(films)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: [u8; 3] = [255, 255, 255];
    const BLACK: [u8; 3] = [0, 0, 0];
    const RED: [u8; 3] = [200, 30, 30];

    fn solid(w: usize, h: usize, rgb: [u8; 3]) -> Rgba8 {
        Rgba8::new(w, h, (0..w * h).flat_map(|_| [rgb[0], rgb[1], rgb[2], 255]).collect()).unwrap()
    }

    #[test]
    fn a_palette_colour_is_all_that_ink_and_the_base_is_none() {
        let films = index_films(&solid(16, 16, RED), &[RED, BLACK], WHITE, 4, 1.0, Kernel::FloydSteinberg).unwrap();
        assert_eq!((films[0].coverage(), films[1].coverage()), (1.0, 0.0));
        let films = index_films(&solid(16, 16, WHITE), &[RED, BLACK], WHITE, 4, 1.0, Kernel::FloydSteinberg).unwrap();
        assert_eq!((films[0].coverage(), films[1].coverage()), (0.0, 0.0));
    }

    #[test]
    fn films_never_overlap() {
        // A horizontal ramp from white to black, dithered into black on white with a red ink too.
        let (w, h) = (64, 32);
        let d: Vec<u8> = (0..w * h).flat_map(|i| [(255 - (i % w) * 4) as u8, (255 - (i % w) * 4) as u8, (255 - (i % w) * 4) as u8, 255]).collect();
        let img = Rgba8::new(w, h, d).unwrap();
        let films = index_films(&img, &[BLACK, RED], WHITE, 2, 1.0, Kernel::FloydSteinberg).unwrap();
        for y in 0..h {
            for x in 0..w {
                assert!(!(films[0].get(x, y) && films[1].get(x, y)), "x={x} y={y}");
            }
        }
    }

    #[test]
    fn dithering_keeps_the_average_brightness() {
        // Mid grey (sRGB 188 is half the light of white) on a white and black palette: about half the cells are black.
        let films = index_films(&solid(128, 128, [188, 188, 188]), &[BLACK], WHITE, 2, 1.0, Kernel::FloydSteinberg).unwrap();
        assert!((films[0].coverage() - 0.5).abs() < 0.03, "{}", films[0].coverage());
    }

    #[test]
    fn no_dither_maps_to_the_nearest_colour() {
        // Light grey is nearer the white base than black; with no dither it prints nothing.
        let films = index_films(&solid(32, 32, [200, 200, 200]), &[BLACK], WHITE, 2, 0.0, Kernel::FloydSteinberg).unwrap();
        assert_eq!(films[0].coverage(), 0.0);
        // Dark grey is nearer black and prints solid.
        let films = index_films(&solid(32, 32, [40, 40, 40]), &[BLACK], WHITE, 2, 0.0, Kernel::FloydSteinberg).unwrap();
        assert_eq!(films[0].coverage(), 1.0);
    }

    #[test]
    fn pixels_come_in_whole_cells() {
        let (w, h) = (40, 40);
        let d: Vec<u8> = (0..w * h).flat_map(|i| [(i % 251) as u8, ((i / 7) % 251) as u8, ((i * 3) % 251) as u8, 255]).collect();
        let img = Rgba8::new(w, h, d).unwrap();
        let films = index_films(&img, &[BLACK, RED], WHITE, 5, 1.0, Kernel::FloydSteinberg).unwrap();
        for f in &films {
            for y in 0..h {
                for x in 0..w {
                    assert_eq!(f.get(x, y), f.get(x - x % 5, y - y % 5), "x={x} y={y}");
                }
            }
        }
    }

    #[test]
    fn transparent_cells_print_nothing_and_take_no_error() {
        let (w, h) = (16, 8);
        let mut d = Vec::new();
        for _ in 0..h {
            for x in 0..w {
                d.extend_from_slice(&if x < 8 { [0, 0, 0, 255] } else { [0, 0, 0, 0] });
            }
        }
        let films = index_films(&Rgba8::new(w, h, d).unwrap(), &[BLACK], WHITE, 2, 1.0, Kernel::FloydSteinberg).unwrap();
        for y in 0..h {
            for x in 0..w {
                assert_eq!(films[0].get(x, y), x < 8, "x={x} y={y}");
            }
        }
    }

    #[test]
    fn sizes_that_do_not_divide_by_the_cell_are_covered() {
        let films = index_films(&solid(13, 7, BLACK), &[BLACK], WHITE, 4, 1.0, Kernel::FloydSteinberg).unwrap();
        assert_eq!(films[0].coverage(), 1.0);
    }

    #[test]
    fn every_kernel_keeps_the_average_and_the_cells_apart() {
        for kernel in [Kernel::FloydSteinberg, Kernel::Jarvis, Kernel::Stucki] {
            let films = index_films(&solid(128, 128, [188, 188, 188]), &[BLACK], WHITE, 2, 1.0, kernel).unwrap();
            assert!((films[0].coverage() - 0.5).abs() < 0.03, "{kernel:?} {}", films[0].coverage());
        }
        let fs = index_films(&solid(64, 64, [188, 188, 188]), &[BLACK], WHITE, 2, 1.0, Kernel::FloydSteinberg).unwrap();
        let st = index_films(&solid(64, 64, [188, 188, 188]), &[BLACK], WHITE, 2, 1.0, Kernel::Stucki).unwrap();
        assert_ne!(fs, st, "different kernels, different patterns");
    }

    #[test]
    fn rejects_nonsense_options() {
        let img = solid(8, 8, BLACK);
        assert!(index_films(&img, &[], WHITE, 2, 1.0, Kernel::FloydSteinberg).is_err());
        assert!(index_films(&img, &[BLACK; MAX_INDEX_INKS + 1], WHITE, 2, 1.0, Kernel::FloydSteinberg).is_err());
        assert!(index_films(&img, &[BLACK], WHITE, 0, 1.0, Kernel::FloydSteinberg).is_err());
        assert!(index_films(&img, &[BLACK], WHITE, MAX_CELL_PX + 1, 1.0, Kernel::FloydSteinberg).is_err());
        for bad in [-0.1, 1.1, f64::NAN, f64::INFINITY] {
            assert!(index_films(&img, &[BLACK], WHITE, 2, bad, Kernel::FloydSteinberg).is_err(), "{bad}");
        }
    }
}
