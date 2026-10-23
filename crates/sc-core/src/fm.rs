//! Stochastic (frequency-modulated, FM) screening: the tone of a plate as a scatter of equal dots
//! whose number, not whose size, follows the tone. There is no screen angle and no ruling, so
//! nothing for two screens to moiré against.
//!
//! The method is error diffusion, serpentine, run on cells of [`FmOptions::cell_px`] film pixels a
//! side, the smallest dot the screen has to hold: every dot is one whole cell or a run of touching
//! cells, never smaller. Each cell takes the mean tone of the plate under it, so the area printed
//! follows the tone and the error left at a cell goes to its unprinted neighbours. The default
//! kernel is Floyd and Steinberg's (1976); Jarvis, Judice and Ninke's and Stucki's (both 1970s)
//! spread the error wider and are offered as alternatives ([`Kernel`]). None of them is the
//! void-and-cluster method (a patented method of the 1990s); error diffusion is older and public.
//!
//! Tones lighter than `min_dot` print nothing and tones darker than `max_dot` print solid, as for
//! the AM screens; dot gain is compensated the same way ([`crate::halftone::compensate_gain`]).
//!
//! [`hybrid_screen`] uses scattered dots only where the tone is very light or very dark, and the
//! angled AM dots of [`crate::halftone`] in between.

use crate::error::{Result, param};
use crate::halftone::{HalftoneOptions, MAX_DOT_GAIN, compensate_gain, halftone};
use crate::image::{Bitmap1, Gray8};
use serde::{Deserialize, Serialize};

/// Largest cell, in film pixels.
pub const MAX_FM_CELL_PX: usize = 64;

/// Where error goes when a cell is printed or not.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kernel {
    /// Floyd and Steinberg: 4 neighbours, the sharpest and the one with the most visible texture.
    #[default]
    FloydSteinberg,
    /// Jarvis, Judice and Ninke: 12 neighbours over 3 rows, smoother and a little softer.
    Jarvis,
    /// Stucki: Jarvis's reach with weights that favour the nearest cells.
    Stucki,
}

impl Kernel {
    /// The neighbours the error goes to on a left-to-right row, as (columns right, rows below,
    /// share). The shares add up to 1.
    pub fn taps(self) -> Vec<(isize, usize, f64)> {
        let (rows, den): (&[(isize, usize, f64)], f64) = match self {
            Self::FloydSteinberg => (&[(1, 0, 7.0), (-1, 1, 3.0), (0, 1, 5.0), (1, 1, 1.0)], 16.0),
            Self::Jarvis => (
                &[
                    (1, 0, 7.0),
                    (2, 0, 5.0),
                    (-2, 1, 3.0),
                    (-1, 1, 5.0),
                    (0, 1, 7.0),
                    (1, 1, 5.0),
                    (2, 1, 3.0),
                    (-2, 2, 1.0),
                    (-1, 2, 3.0),
                    (0, 2, 5.0),
                    (1, 2, 3.0),
                    (2, 2, 1.0),
                ],
                48.0,
            ),
            Self::Stucki => (
                &[
                    (1, 0, 8.0),
                    (2, 0, 4.0),
                    (-2, 1, 2.0),
                    (-1, 1, 4.0),
                    (0, 1, 8.0),
                    (1, 1, 4.0),
                    (2, 1, 2.0),
                    (-2, 2, 1.0),
                    (-1, 2, 2.0),
                    (0, 2, 4.0),
                    (1, 2, 2.0),
                    (2, 2, 1.0),
                ],
                42.0,
            ),
        };
        rows.iter().map(|&(dx, dy, w)| (dx, dy, w / den)).collect()
    }
}

/// How to screen.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FmOptions {
    /// Side of one dot, in film pixels (1 to [`MAX_FM_CELL_PX`]).
    pub cell_px: usize,
    /// Coverage below this prints nothing. 0..1.
    pub min_dot: f64,
    /// Coverage above this prints solid. 0..1.
    pub max_dot: f64,
    /// Dot gain to compensate (0 to [`MAX_DOT_GAIN`]), as in [`crate::halftone::HalftoneOptions`].
    pub dot_gain: f64,
    /// How the error is spread.
    #[serde(default)]
    pub kernel: Kernel,
}

fn validate(o: &FmOptions) -> Result<()> {
    if o.cell_px == 0 || o.cell_px > MAX_FM_CELL_PX {
        return Err(param("cell_px", format!("must be 1..={MAX_FM_CELL_PX}")));
    }
    if !(0.0..=1.0).contains(&o.min_dot) || !(0.0..=1.0).contains(&o.max_dot) || o.min_dot > o.max_dot {
        return Err(param("min_dot/max_dot", "need 0 <= min_dot <= max_dot <= 1"));
    }
    if !(0.0..=MAX_DOT_GAIN).contains(&o.dot_gain) {
        return Err(param("dot_gain", format!("must be between 0 and {MAX_DOT_GAIN}")));
    }
    Ok(())
}

/// The mean tone (0 to 1) of the plate under each `cell`-pixel cell: the grid size and the tones.
fn cell_tones(plate: &Gray8, cell: usize) -> (usize, usize, Vec<f64>) {
    let (w, h) = (plate.width(), plate.height());
    let (cw, ch) = (w.div_ceil(cell), h.div_ceil(cell));
    let mut sum = vec![0u64; cw * ch];
    let mut area = vec![0u64; cw * ch];
    for (i, &v) in plate.data().iter().enumerate() {
        let c = (i / w) / cell * cw + (i % w) / cell;
        if let (Some(s), Some(a)) = (sum.get_mut(c), area.get_mut(c)) {
            *s += u64::from(v);
            *a += 1;
        }
    }
    let tone = sum.iter().zip(&area).map(|(&s, &a)| s as f64 / (255.0 * a.max(1) as f64)).collect();
    (cw, ch, tone)
}

/// Screen a tone plate into 1-bit film.
pub fn fm_screen(plate: &Gray8, o: &FmOptions) -> Result<Bitmap1> {
    validate(o)?;
    let (w, h, cell) = (plate.width(), plate.height(), o.cell_px);
    let (cw, ch, raw) = cell_tones(plate, cell);
    // The tone each cell should print: the limits and the dot gain applied.
    let tone: Vec<f64> = raw
        .iter()
        .map(|&t| {
            if t < o.min_dot.max(1.0 / 255.0) {
                0.0
            } else if t > o.max_dot {
                1.0
            } else {
                compensate_gain(t, o.dot_gain)
            }
        })
        .collect();

    let taps = o.kernel.taps();
    let mut err = vec![0.0f64; cw * ch];
    let mut on = vec![false; cw * ch];
    for y in 0..ch {
        let ltr = y % 2 == 0;
        for step in 0..cw {
            let x = if ltr { step } else { cw - 1 - step };
            let i = y * cw + x;
            let want = tone.get(i).copied().unwrap_or(0.0) + err.get(i).copied().unwrap_or(0.0);
            // Solid and empty cells stay what they are; the error is for the tones between.
            let fixed = matches!(tone.get(i), Some(&t) if t <= 0.0 || t >= 1.0);
            let print = match tone.get(i).copied() {
                Some(t) if t <= 0.0 => false,
                Some(t) if t >= 1.0 => true,
                _ => want >= 0.5,
            };
            if let Some(slot) = on.get_mut(i) {
                *slot = print;
            }
            let miss = if fixed { 0.0 } else { want - f64::from(u8::from(print)) };
            for &(dx, dy, wt) in &taps {
                let dx = if ltr { dx } else { -dx };
                let (nx, ny) = (x as isize + dx, y + dy);
                if nx < 0 || nx as usize >= cw || ny >= ch {
                    continue;
                }
                if let Some(e) = err.get_mut(ny * cw + nx as usize) {
                    *e += miss * wt;
                }
            }
        }
    }
    let mut bits = Vec::with_capacity(w * h);
    for y in 0..h {
        let row = y / cell * cw;
        for x in 0..w {
            bits.push(on.get(row + x / cell).copied().unwrap_or(false));
        }
    }
    Bitmap1::from_bits(w, h, bits)
}

/// Where the hybrid screen scatters dots instead of angling them.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hybrid {
    /// Cells lighter than this tone are scattered (0..1).
    pub below: f64,
    /// Cells darker than this tone are scattered (0..1, not below `below`).
    pub above: f64,
}

/// A hybrid screen: the angled AM dots of `am` where a cell's tone is between `h.below` and
/// `h.above`, and scattered FM dots (`fm`) in the lighter and darker cells. The area printed
/// follows the tone either way, so there is no step in density where the two meet, only in the
/// texture. Highlights and shadows are where small AM dots will not hold on the screen; FM keeps
/// them as isolated whole cells of a size the screen can hold. `am`'s own limits are not used in
/// the middle (they would only matter there if `below` is under `fm.min_dot`); the dot gain of
/// `fm` is used for both.
pub fn hybrid_screen(plate: &Gray8, am: &HalftoneOptions, fm: &FmOptions, h: &Hybrid) -> Result<Bitmap1> {
    if !(0.0..=1.0).contains(&h.below) || !(0.0..=1.0).contains(&h.above) || h.below > h.above {
        return Err(param("hybrid", "need 0 <= below <= above <= 1"));
    }
    let scattered = fm_screen(plate, fm)?;
    let angled = halftone(plate, &HalftoneOptions { min_dot: 0.0, max_dot: 1.0, dot_gain: fm.dot_gain, ..*am })?;
    let (w, cell) = (plate.width(), fm.cell_px);
    let (cw, _, tone) = cell_tones(plate, cell);
    let bits = (0..plate.height())
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .map(|(x, y)| {
            let t = tone.get(y / cell * cw + x / cell).copied().unwrap_or(0.0);
            if t < h.below || t > h.above { scattered.get(x, y) } else { angled.get(x, y) }
        })
        .collect();
    Bitmap1::from_bits(w, plate.height(), bits)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(cell: usize) -> FmOptions {
        FmOptions { cell_px: cell, min_dot: 0.0, max_dot: 1.0, dot_gain: 0.0, kernel: Kernel::default() }
    }

    #[test]
    fn the_area_printed_follows_the_tone_at_any_cell_size() {
        for cell in [1, 3, 6] {
            for tone in [0.1_f64, 0.25, 0.5, 0.75, 0.9] {
                let plate = Gray8::filled(240, 240, (tone * 255.0).round() as u8).unwrap();
                let film = fm_screen(&plate, &opts(cell)).unwrap();
                assert!((film.coverage() - tone).abs() < 0.02, "cell {cell} tone {tone}: {}", film.coverage());
            }
        }
    }

    #[test]
    fn every_dot_is_a_whole_cell_and_there_is_no_repeat_to_moire_with() {
        let plate = Gray8::filled(120, 120, 60).unwrap();
        let film = fm_screen(&plate, &opts(4)).unwrap();
        for y in 0..120 {
            for x in 0..120 {
                assert_eq!(film.get(x, y), film.get(x - x % 4, y - y % 4), "({x}, {y})");
            }
        }
        // Not a regular lattice: the rows are not all copies of one another.
        let row = |y: usize| (0..30).map(|c| film.get(c * 4, y * 4)).collect::<Vec<_>>();
        let distinct: std::collections::HashSet<_> = (0..30).map(row).collect();
        assert!(distinct.len() > 10, "{} distinct rows", distinct.len());
    }

    #[test]
    fn a_ramp_prints_more_dots_towards_the_dark_end() {
        let (w, h) = (256, 64);
        let plate = Gray8::new(w, h, (0..w * h).map(|i| (i % w) as u8).collect()).unwrap();
        let film = fm_screen(&plate, &opts(2)).unwrap();
        let share = |x0: usize, x1: usize| {
            let n = (x1 - x0) * h;
            (0..h).flat_map(|y| (x0..x1).map(move |x| (x, y))).filter(|&(x, y)| film.get(x, y)).count() as f64 / n as f64
        };
        let (a, b, c) = (share(0, 64), share(96, 160), share(192, 256));
        assert!(a < b && b < c, "{a} {b} {c}");
        assert!((b - 0.5).abs() < 0.06 && (a - 0.125).abs() < 0.05 && (c - 0.875).abs() < 0.05, "{a} {b} {c}");
    }

    #[test]
    fn limits_gain_extremes_and_sizes() {
        let plate = |v| Gray8::filled(60, 60, v).unwrap();
        assert_eq!(fm_screen(&plate(0), &opts(3)).unwrap().coverage(), 0.0);
        assert_eq!(fm_screen(&plate(255), &opts(3)).unwrap().coverage(), 1.0);
        let lim = FmOptions { min_dot: 0.08, max_dot: 0.95, ..opts(3) };
        assert_eq!(fm_screen(&plate(13), &lim).unwrap().coverage(), 0.0, "5% prints nothing");
        assert_eq!(fm_screen(&plate(247), &lim).unwrap().coverage(), 1.0, "97% prints solid");
        // 20% gain: a 70% tone is a 50% film.
        let g = fm_screen(&plate(179), &FmOptions { dot_gain: 0.2, ..opts(3) }).unwrap();
        assert!((g.coverage() - 0.5).abs() < 0.03, "{}", g.coverage());
        // A size that does not divide by the cell is still covered to its edges.
        let odd = fm_screen(&Gray8::filled(61, 59, 128).unwrap(), &opts(4)).unwrap();
        assert_eq!((odd.width(), odd.height()), (61, 59));
        assert!((odd.coverage() - 0.5).abs() < 0.05);
    }

    #[test]
    fn nonsense_options_are_errors() {
        let p = Gray8::filled(8, 8, 128).unwrap();
        for bad in [
            FmOptions { cell_px: 0, ..opts(1) },
            FmOptions { cell_px: MAX_FM_CELL_PX + 1, ..opts(1) },
            FmOptions { min_dot: 0.9, max_dot: 0.5, ..opts(1) },
            FmOptions { max_dot: 1.5, ..opts(1) },
            FmOptions { min_dot: f64::NAN, ..opts(1) },
            FmOptions { dot_gain: 0.3, ..opts(1) },
            FmOptions { dot_gain: f64::NAN, ..opts(1) },
        ] {
            assert!(fm_screen(&p, &bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn every_kernel_spreads_all_of_the_error_and_keeps_the_tone() {
        for k in [Kernel::FloydSteinberg, Kernel::Jarvis, Kernel::Stucki] {
            let total: f64 = k.taps().iter().map(|t| t.2).sum();
            assert!((total - 1.0).abs() < 1e-12, "{k:?}: {total}");
            // Error only goes forward along the row or down: never back to a cell already done.
            assert!(k.taps().iter().all(|&(dx, dy, _)| dy > 0 || dx > 0), "{k:?}");
            for tone in [0.1_f64, 0.33, 0.5, 0.8] {
                let plate = Gray8::filled(240, 240, (tone * 255.0).round() as u8).unwrap();
                let film = fm_screen(&plate, &FmOptions { kernel: k, ..opts(3) }).unwrap();
                assert!((film.coverage() - tone).abs() < 0.02, "{k:?} tone {tone}: {}", film.coverage());
            }
        }
        let plate = Gray8::filled(120, 120, 100).unwrap();
        let screen = |k| fm_screen(&plate, &FmOptions { kernel: k, ..opts(2) }).unwrap();
        let (a, b, c) = (screen(Kernel::FloydSteinberg), screen(Kernel::Jarvis), screen(Kernel::Stucki));
        assert!(a != b && b != c && a != c, "the kernels make different patterns");
    }

    fn am(dpi: f64) -> HalftoneOptions {
        HalftoneOptions { dpi, lpi: 50.0, angle_deg: 22.5, shape: crate::halftone::DotShape::Round, min_dot: 0.08, max_dot: 0.95, dot_gain: 0.0 }
    }

    /// A ramp from 0 (left) to 255 (right), `w` wide.
    fn ramp(w: usize, h: usize) -> Gray8 {
        Gray8::new(w, h, (0..w * h).map(|i| ((i % w) * 255 / (w - 1)) as u8).collect()).unwrap()
    }

    #[test]
    fn the_hybrid_scatters_at_the_ends_and_angles_dots_in_the_middle() {
        let (w, h) = (300, 60);
        let plate = ramp(w, h);
        let fm = opts(3);
        let hy = Hybrid { below: 0.2, above: 0.8 };
        let film = hybrid_screen(&plate, &am(600.0), &fm, &hy).unwrap();
        let scattered = fm_screen(&plate, &fm).unwrap();
        let angled = halftone(&plate, &HalftoneOptions { min_dot: 0.0, max_dot: 1.0, ..am(600.0) }).unwrap();
        // Well inside each band the film is the one screen or the other, pixel for pixel.
        for y in 0..h {
            for x in 0..w {
                let t = x as f64 / (w - 1) as f64;
                if !(0.17..=0.83).contains(&t) {
                    assert_eq!(film.get(x, y), scattered.get(x, y), "scattered at ({x}, {y})");
                } else if (0.23..0.77).contains(&t) {
                    assert_eq!(film.get(x, y), angled.get(x, y), "angled at ({x}, {y})");
                }
            }
        }
        // The area follows the tone across the joins.
        let share = |x0: usize, x1: usize| {
            (0..h).flat_map(|y| (x0..x1).map(move |x| (x, y))).filter(|&(x, y)| film.get(x, y)).count() as f64 / ((x1 - x0) * h) as f64
        };
        for (x0, x1, want) in [(0, 30, 0.05), (45, 75, 0.25), (135, 165, 0.5), (225, 255, 0.75), (270, 300, 0.95)] {
            assert!((share(x0, x1) - want).abs() < 0.08, "{x0}..{x1}: {} vs {want}", share(x0, x1));
        }
    }

    #[test]
    fn the_hybrid_is_all_angled_or_all_scattered_at_the_extremes_of_its_range() {
        let plate = ramp(120, 40);
        let fm = opts(3);
        let all_am = hybrid_screen(&plate, &am(600.0), &fm, &Hybrid { below: 0.0, above: 1.0 }).unwrap();
        assert_eq!(all_am, halftone(&plate, &HalftoneOptions { min_dot: 0.0, max_dot: 1.0, ..am(600.0) }).unwrap());
        let none_am = hybrid_screen(&plate, &am(600.0), &fm, &Hybrid { below: 1.0, above: 1.0 }).unwrap();
        let fm_only = fm_screen(&plate, &fm).unwrap();
        // Everything lighter than solid is scattered: the only angled cells are the fully solid ones.
        for y in 0..40 {
            for x in 0..120 {
                if x < 110 {
                    assert_eq!(none_am.get(x, y), fm_only.get(x, y), "({x}, {y})");
                }
            }
        }
        for bad in [
            Hybrid { below: 0.6, above: 0.4 },
            Hybrid { below: -0.1, above: 0.5 },
            Hybrid { below: 0.2, above: 1.5 },
            Hybrid { below: f64::NAN, above: 0.5 },
        ] {
            assert!(hybrid_screen(&plate, &am(600.0), &fm, &bad).is_err(), "{bad:?}");
        }
        // The AM screen's own rules still apply: too fine a ruling for the resolution is an error.
        assert!(hybrid_screen(&plate, &HalftoneOptions { lpi: 300.0, ..am(600.0) }, &fm, &Hybrid { below: 0.1, above: 0.9 }).is_err());
    }
}
