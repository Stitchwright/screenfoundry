//! Amplitude-modulated screening: a rotated grid of dots whose size follows the plate's coverage.
//!
//! Each film pixel is mapped into the rotated cell grid, and a spot function gives its position
//! in the order dots fill (centre first). The spot function's values are then converted to ranks,
//! so a plate at 50% inks 50% of the area for any dot shape.

use crate::error::{Result, param};
use crate::image::{Bitmap1, Gray8};
use serde::{Deserialize, Serialize};
use std::f64::consts::TAU;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DotShape {
    /// Round dots that merge into a checkerboard at 50% (general work).
    Round,
    /// Chain dots that join along one axis near 40% and 60% (smoother tonal transitions).
    Ellipse,
    /// Parallel lines.
    Line,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct HalftoneOptions {
    /// Film resolution in pixels per inch.
    pub dpi: f64,
    /// Screen ruling in lines (dot rows) per inch.
    pub lpi: f64,
    /// Screen angle in degrees.
    pub angle_deg: f64,
    pub shape: DotShape,
    /// Coverage below this prints nothing (tiny dots will not hold on the screen). 0..1.
    pub min_dot: f64,
    /// Coverage above this prints solid (highlights between dots fill in). 0..1.
    pub max_dot: f64,
}

impl Default for HalftoneOptions {
    fn default() -> Self {
        // 22.5 degrees is the commonly cited screen-printing angle; 8% / 95% are editable
        // limits, not facts (see the research notes).
        Self { dpi: 1200.0, lpi: 45.0, angle_deg: 22.5, shape: DotShape::Round, min_dot: 0.08, max_dot: 0.95 }
    }
}

const RANK_GRID: usize = 128;

fn spot(shape: DotShape, fu: f64, fv: f64) -> f64 {
    let (cu, cv) = ((TAU * fu).cos(), (TAU * fv).cos());
    match shape {
        DotShape::Round => 0.5 - 0.25 * (cu + cv),
        DotShape::Ellipse => 0.5 - 0.5 * (0.6 * cu + 0.4 * cv),
        DotShape::Line => 0.5 - 0.5 * cu,
    }
}

/// Sorted spot values over one cell: the rank of a value is its share of the cell that fills first.
fn ranks(shape: DotShape) -> Vec<f64> {
    let mut v = Vec::with_capacity(RANK_GRID * RANK_GRID);
    for j in 0..RANK_GRID {
        for i in 0..RANK_GRID {
            let fu = (i as f64 + 0.5) / RANK_GRID as f64 - 0.5;
            let fv = (j as f64 + 0.5) / RANK_GRID as f64 - 0.5;
            v.push(spot(shape, fu, fv));
        }
    }
    v.sort_by(f64::total_cmp);
    v
}

fn validate(o: &HalftoneOptions) -> Result<()> {
    if !o.dpi.is_finite() || o.dpi <= 0.0 || o.dpi > 20_000.0 {
        return Err(param("dpi", "must be 0 < dpi <= 20000"));
    }
    if !o.lpi.is_finite() || o.lpi <= 0.0 {
        return Err(param("lpi", "must be positive"));
    }
    if o.dpi / o.lpi < 4.0 {
        return Err(param("lpi", "needs at least 4 film pixels per dot cell (dpi / lpi >= 4)"));
    }
    if !o.angle_deg.is_finite() {
        return Err(param("angle_deg", "must be finite"));
    }
    if !(0.0..=1.0).contains(&o.min_dot) || !(0.0..=1.0).contains(&o.max_dot) || o.min_dot > o.max_dot {
        return Err(param("min_dot/max_dot", "need 0 <= min_dot <= max_dot <= 1"));
    }
    Ok(())
}

/// Screen a tone plate into 1-bit film.
pub fn halftone(plate: &Gray8, o: &HalftoneOptions) -> Result<Bitmap1> {
    validate(o)?;
    let cell = o.dpi / o.lpi;
    let (s, c) = o.angle_deg.to_radians().sin_cos();
    let sorted = ranks(o.shape);
    let n = sorted.len() as f64;
    let (w, h) = (plate.width(), plate.height());
    let mut bits = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let tone = f64::from(plate.data().get(y * w + x).copied().unwrap_or(0)) / 255.0;
            let on = if tone < o.min_dot.max(1.0 / 255.0) {
                false
            } else if tone > o.max_dot {
                true
            } else {
                let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                let (u, v) = ((px * c + py * s) / cell, (-px * s + py * c) / cell);
                let (fu, fv) = (u - u.floor() - 0.5, v - v.floor() - 0.5);
                let rank = sorted.partition_point(|&q| q < spot(o.shape, fu, fv)) as f64 / n;
                rank < tone
            };
            bits.push(on);
        }
    }
    Bitmap1::from_bits(w, h, bits)
}

/// Threshold a solid plate at 50% into film (line art and spot colours need no screening).
pub fn solid(plate: &Gray8) -> Result<Bitmap1> {
    Bitmap1::from_bits(plate.width(), plate.height(), plate.data().iter().map(|&v| v >= 128).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(shape: DotShape, angle: f64) -> HalftoneOptions {
        HalftoneOptions { dpi: 600.0, lpi: 50.0, angle_deg: angle, shape, min_dot: 0.0, max_dot: 1.0 }
    }

    #[test]
    fn area_coverage_tracks_tone_for_every_shape() {
        for shape in [DotShape::Round, DotShape::Ellipse, DotShape::Line] {
            for tone in [0.1_f64, 0.25, 0.5, 0.75, 0.9] {
                let plate = Gray8::filled(240, 240, (tone * 255.0).round() as u8).unwrap();
                let film = halftone(&plate, &opts(shape, 22.5)).unwrap();
                assert!((film.coverage() - tone).abs() < 0.03, "{shape:?} tone {tone} gave {}", film.coverage());
            }
        }
    }

    #[test]
    fn extremes_and_limits() {
        let zero = Gray8::filled(50, 50, 0).unwrap();
        assert_eq!(halftone(&zero, &HalftoneOptions::default()).unwrap().coverage(), 0.0);
        let full = Gray8::filled(50, 50, 255).unwrap();
        assert_eq!(halftone(&full, &HalftoneOptions::default()).unwrap().coverage(), 1.0);
        // 5% is below the default 8% floor: prints nothing. 97% is over the cap: prints solid.
        let low = Gray8::filled(50, 50, 13).unwrap();
        assert_eq!(halftone(&low, &HalftoneOptions::default()).unwrap().coverage(), 0.0);
        let high = Gray8::filled(50, 50, 247).unwrap();
        assert_eq!(halftone(&high, &HalftoneOptions::default()).unwrap().coverage(), 1.0);
    }

    #[test]
    fn dots_repeat_at_the_screen_period() {
        // Angle 0, 600 dpi / 50 lpi = 12 px cells: the pattern repeats every 12 px.
        let plate = Gray8::filled(48, 48, 100).unwrap();
        let film = halftone(&plate, &opts(DotShape::Round, 0.0)).unwrap();
        for y in 0..36 {
            for x in 0..36 {
                assert_eq!(film.get(x, y), film.get(x + 12, y), "x={x} y={y}");
                assert_eq!(film.get(x, y), film.get(x, y + 12));
            }
        }
    }

    #[test]
    fn different_angles_make_different_patterns() {
        let plate = Gray8::filled(96, 96, 128).unwrap();
        let a = halftone(&plate, &opts(DotShape::Round, 22.5)).unwrap();
        let b = halftone(&plate, &opts(DotShape::Round, 67.5)).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn rejects_nonsense_options() {
        let p = Gray8::filled(8, 8, 128).unwrap();
        let bad = |f: fn(&mut HalftoneOptions)| {
            let mut o = HalftoneOptions::default();
            f(&mut o);
            assert!(halftone(&p, &o).is_err());
        };
        bad(|o| o.dpi = f64::NAN);
        bad(|o| o.dpi = 0.0);
        bad(|o| o.lpi = -5.0);
        bad(|o| o.lpi = o.dpi); // one pixel per cell
        bad(|o| o.angle_deg = f64::INFINITY);
        bad(|o| o.min_dot = 0.99); // above the default max_dot
        bad(|o| o.max_dot = 1.5);
    }

    #[test]
    fn solid_thresholds_at_half() {
        let p = Gray8::new(3, 1, vec![0, 127, 128]).unwrap();
        assert_eq!(solid(&p).unwrap().bits(), &[false, false, true]);
    }
}
