//! A measured dot-gain curve: what each film tone really printed as.
//!
//! The test page (`testpage`) shows, for film tones from 0 to 100 percent, the tone that comes
//! out on the press. Entering those pairs gives a curve the separation inverts: to print a tone
//! `p`, the film carries the tone `f` whose printed tone is `p`. Between the entered points the
//! curve is straight lines; the ends (0 prints as 0, 100 as 100) are implied.

use serde::{Deserialize, Serialize};

use crate::error::{Result, param};
use crate::image::Gray8;

/// Most points one curve holds.
pub const MAX_CURVE_POINTS: usize = 32;

/// (film tone, printed tone) pairs in 0..=1, with film strictly increasing and printed never
/// falling.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ToneCurve {
    points: Vec<(f64, f64)>,
}

impl ToneCurve {
    /// A curve through `points`; the ends (0,0) and (1,1) are added when missing.
    pub fn new(points: &[(f64, f64)]) -> Result<Self> {
        if points.len() > MAX_CURVE_POINTS {
            return Err(param("dot_curve", format!("at most {MAX_CURVE_POINTS} points")));
        }
        if points.iter().any(|&(f, p)| !f.is_finite() || !p.is_finite() || !(0.0..=1.0).contains(&f) || !(0.0..=1.0).contains(&p)) {
            return Err(param("dot_curve", "tones are between 0 and 1"));
        }
        let mut all: Vec<(f64, f64)> = vec![(0.0, 0.0)];
        all.extend(points.iter().copied().filter(|&(f, _)| f > 0.0 && f < 1.0));
        all.push((1.0, 1.0));
        if all.windows(2).any(|w| w[1].0 <= w[0].0 || w[1].1 < w[0].1) {
            return Err(param("dot_curve", "film tones must rise and printed tones must not fall"));
        }
        Ok(Self { points: all })
    }

    /// The tone that prints from `film`.
    pub fn printed(&self, film: f64) -> f64 {
        let film = film.clamp(0.0, 1.0);
        for w in self.points.windows(2) {
            let ((f0, p0), (f1, p1)) = (w[0], w[1]);
            if film <= f1 {
                return p0 + (p1 - p0) * ((film - f0) / (f1 - f0).max(f64::MIN_POSITIVE));
            }
        }
        film
    }

    /// The film tone that prints as `printed` (the lowest one where the curve is flat).
    pub fn film_for(&self, printed: f64) -> f64 {
        let printed = printed.clamp(0.0, 1.0);
        for w in self.points.windows(2) {
            let ((f0, p0), (f1, p1)) = (w[0], w[1]);
            if printed <= p1 {
                if p1 - p0 <= f64::EPSILON {
                    return f0;
                }
                return f0 + (f1 - f0) * ((printed - p0) / (p1 - p0)).clamp(0.0, 1.0);
            }
        }
        printed
    }

    /// `plate`, whose values are the tones wanted on the print, as the tones the film must carry.
    pub fn compensate(&self, plate: &Gray8) -> Result<Gray8> {
        let lut: Vec<u8> = (0..=255u32).map(|v| (self.film_for(f64::from(v) / 255.0) * 255.0).round().clamp(0.0, 255.0) as u8).collect();
        let data = plate.data().iter().map(|&v| lut.get(usize::from(v)).copied().unwrap_or(v)).collect();
        Gray8::new(plate.width(), plate.height(), data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_inverse_undoes_the_curve() {
        let c = ToneCurve::new(&[(0.25, 0.35), (0.5, 0.7), (0.75, 0.88)]).unwrap();
        for f in [0.0, 0.1, 0.25, 0.4, 0.5, 0.6, 0.75, 0.9, 1.0] {
            assert!((c.film_for(c.printed(f)) - f).abs() < 1e-9, "{f}");
        }
        assert!((c.film_for(0.7) - 0.5).abs() < 1e-9 && c.film_for(0.0) == 0.0 && c.film_for(1.0) == 1.0);
    }

    #[test]
    fn a_gaining_press_gets_a_lighter_film() {
        let c = ToneCurve::new(&[(0.5, 0.7)]).unwrap();
        let plate = Gray8::new(3, 1, vec![0, 178, 255]).unwrap(); // 0%, 70%, 100% wanted
        let film = c.compensate(&plate).unwrap();
        assert_eq!(film.data()[0], 0);
        assert!((i32::from(film.data()[1]) - 128).abs() <= 1, "{:?}", film.data());
        assert_eq!(film.data()[2], 255);
    }

    #[test]
    fn flat_stretches_and_bad_input_are_handled() {
        // Everything between 0.4 and 0.6 on film prints as 0.5: asking for 0.5 takes the lowest film tone.
        let c = ToneCurve::new(&[(0.4, 0.5), (0.6, 0.5)]).unwrap();
        assert!((c.film_for(0.5) - 0.4).abs() < 1e-9);
        assert!(ToneCurve::new(&[(0.5, 0.7), (0.4, 0.8)]).is_err(), "film must rise");
        assert!(ToneCurve::new(&[(0.4, 0.7), (0.5, 0.6)]).is_err(), "printed must not fall");
        assert!(ToneCurve::new(&[(1.5, 0.5)]).is_err() && ToneCurve::new(&[(f64::NAN, 0.5)]).is_err());
        assert!(ToneCurve::new(&vec![(0.5, 0.5); MAX_CURVE_POINTS + 1]).is_err());
        // The ends given explicitly or not make no difference.
        assert_eq!(ToneCurve::new(&[(0.0, 0.0), (0.5, 0.6), (1.0, 1.0)]).unwrap(), ToneCurve::new(&[(0.5, 0.6)]).unwrap());
        // A default curve (from a bad file) leaves tones alone.
        assert_eq!(ToneCurve::default().film_for(0.3), 0.3);
    }
}
