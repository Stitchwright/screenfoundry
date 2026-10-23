//! Stochastic (frequency-modulated, FM) screening: the tone of a plate as a scatter of equal dots
//! whose number, not whose size, follows the tone. There is no screen angle and no ruling, so
//! nothing for two screens to moiré against.
//!
//! The method is Floyd–Steinberg error diffusion (1976), serpentine, run on cells of
//! [`FmOptions::cell_px`] film pixels a side, the smallest dot the screen has to hold: every dot
//! is one whole cell or a run of touching cells, never smaller. Each cell takes the mean tone of
//! the plate under it, so the area printed follows the tone and the error left at a cell goes to
//! its unprinted neighbours. It is not the void-and-cluster method (a patented method of the
//! 1990s); error diffusion is older and public.
//!
//! Tones lighter than `min_dot` print nothing and tones darker than `max_dot` print solid, as for
//! the AM screens; dot gain is compensated the same way ([`crate::halftone::compensate_gain`]).

use crate::error::{Result, param};
use crate::halftone::{MAX_DOT_GAIN, compensate_gain};
use crate::image::{Bitmap1, Gray8};

/// Largest cell, in film pixels.
pub const MAX_FM_CELL_PX: usize = 64;

/// Error diffusion weights (Floyd and Steinberg): right, below-left, below, below-right.
const WEIGHTS: [(isize, usize, f64); 4] = [(1, 0, 7.0 / 16.0), (-1, 1, 3.0 / 16.0), (0, 1, 5.0 / 16.0), (1, 1, 1.0 / 16.0)];

/// How to screen.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FmOptions {
    /// Side of one dot, in film pixels (1 to [`MAX_FM_CELL_PX`]).
    pub cell_px: usize,
    /// Coverage below this prints nothing. 0..1.
    pub min_dot: f64,
    /// Coverage above this prints solid. 0..1.
    pub max_dot: f64,
    /// Dot gain to compensate (0 to [`MAX_DOT_GAIN`]), as in [`crate::halftone::HalftoneOptions`].
    pub dot_gain: f64,
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

/// Screen a tone plate into 1-bit film.
pub fn fm_screen(plate: &Gray8, o: &FmOptions) -> Result<Bitmap1> {
    validate(o)?;
    let (w, h, cell) = (plate.width(), plate.height(), o.cell_px);
    let (cw, ch) = (w.div_ceil(cell), h.div_ceil(cell));
    // The mean tone of the plate under each cell.
    let mut sum = vec![0u64; cw * ch];
    let mut area = vec![0u64; cw * ch];
    for (i, &v) in plate.data().iter().enumerate() {
        let c = (i / w) / cell * cw + (i % w) / cell;
        if let (Some(s), Some(a)) = (sum.get_mut(c), area.get_mut(c)) {
            *s += u64::from(v);
            *a += 1;
        }
    }
    let tone: Vec<f64> = sum
        .iter()
        .zip(&area)
        .map(|(&s, &a)| {
            let t = s as f64 / (255.0 * a.max(1) as f64);
            if t < o.min_dot.max(1.0 / 255.0) {
                0.0
            } else if t > o.max_dot {
                1.0
            } else {
                compensate_gain(t, o.dot_gain)
            }
        })
        .collect();

    let mut err = vec![0.0f64; cw * ch];
    let mut on = vec![false; cw * ch];
    for y in 0..ch {
        let ltr = y % 2 == 0;
        for step in 0..cw {
            let x = if ltr { step } else { cw - 1 - step };
            let i = y * cw + x;
            let want = tone.get(i).copied().unwrap_or(0.0) + err.get(i).copied().unwrap_or(0.0);
            // Solid and empty cells stay what they are; the error is for the tones between.
            let print = match tone.get(i).copied() {
                Some(t) if t <= 0.0 => false,
                Some(t) if t >= 1.0 => true,
                _ => want >= 0.5,
            };
            if let Some(slot) = on.get_mut(i) {
                *slot = print;
            }
            let miss = if matches!(tone.get(i), Some(&t) if t <= 0.0 || t >= 1.0) { 0.0 } else { want - f64::from(u8::from(print)) };
            for &(dx, dy, wt) in &WEIGHTS {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(cell: usize) -> FmOptions {
        FmOptions { cell_px: cell, min_dot: 0.0, max_dot: 1.0, dot_gain: 0.0 }
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
}
