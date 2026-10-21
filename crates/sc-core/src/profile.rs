//! Sanity checks of a screen setup against the mesh count. The ratios are practitioner rules of
//! thumb that vary by source (mesh / 4 to mesh / 5 is common; one guide caps lpi at mesh / 3), so
//! these produce advice for the user, never refusals.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Advice {
    /// Above a third of the mesh count: dots will not resolve.
    LpiTooHigh { max_lpi: f64 },
    /// Above a quarter of the mesh count: expect trouble in the highlights.
    LpiAboveRecommended { recommended_max: f64 },
    /// 0 or 90 degrees lines the dots up with the mesh threads, which invites moire.
    AngleAlignsWithThreads,
}

/// The lpi range usually recommended for a mesh (mesh / 5 up to mesh / 4).
pub fn recommended_lpi_range(mesh: f64) -> (f64, f64) {
    (mesh / 5.0, mesh / 4.0)
}

/// Advice for printing `lpi` at `angle_deg` through `mesh` threads per inch.
pub fn check(mesh: f64, lpi: f64, angle_deg: f64) -> Vec<Advice> {
    let mut out = Vec::new();
    if mesh.is_finite() && lpi.is_finite() && mesh > 0.0 {
        if lpi > mesh / 3.0 {
            out.push(Advice::LpiTooHigh { max_lpi: mesh / 3.0 });
        } else if lpi > mesh / 4.0 {
            out.push(Advice::LpiAboveRecommended { recommended_max: mesh / 4.0 });
        }
    }
    if angle_deg.is_finite() {
        let a = angle_deg.rem_euclid(90.0);
        if !(2.0..=88.0).contains(&a) {
            out.push(Advice::AngleAlignsWithThreads);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typical_setup_is_fine() {
        assert!(check(230.0, 45.0, 22.5).is_empty());
        let (lo, hi) = recommended_lpi_range(230.0);
        assert!((lo - 46.0).abs() < 1e-9 && (hi - 57.5).abs() < 1e-9);
    }

    #[test]
    fn flags_high_lpi_and_thread_angles() {
        assert_eq!(check(110.0, 45.0, 22.5), vec![Advice::LpiTooHigh { max_lpi: 110.0 / 3.0 }]);
        assert_eq!(check(160.0, 45.0, 22.5), vec![Advice::LpiAboveRecommended { recommended_max: 40.0 }]);
        assert_eq!(check(230.0, 45.0, 90.0), vec![Advice::AngleAlignsWithThreads]);
        assert_eq!(check(230.0, 45.0, -180.5), vec![Advice::AngleAlignsWithThreads]);
    }

    #[test]
    fn non_finite_input_gives_no_advice_and_no_panic() {
        assert!(check(f64::NAN, f64::NAN, f64::NAN).is_empty());
        assert!(check(0.0, 45.0, 22.5).is_empty());
    }
}
