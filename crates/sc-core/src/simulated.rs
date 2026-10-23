//! Simulated-process separation: full-colour art printed as halftone dots of a few spot inks on a
//! base (a white underbase, or the garment).
//!
//! The model is optical mixing in linear light. Side by side, dots of ink `i` covering a share
//! `c_i` of the area and the base covering the rest look like `sum(c_i * C_i) + c_0 * W`, with
//! `c_0 = 1 - sum(c_i)`. For a pixel of colour `T` the best mix is the point of the convex hull of
//! the base and the inks closest to `T`. When several mixes match `T` exactly (more than four
//! points make the hull ambiguous) the one with the least ink wins: fewer dots print cleaner.
//!
//! Dot gain (done in the halftone step), overlap of dots at different angles and ink opacity are not modelled; the defaults of
//! the halftone step are where those are tuned.

use std::collections::HashMap;

use crate::error::{Result, param};
use crate::image::{Gray8, Rgba8};
use crate::separate::ALPHA_CUTOFF;

/// Most inks a simulated-process job separates (the base is extra).
pub const MAX_INKS: usize = 8;

/// Lowest ink limit accepted.
pub const MIN_INK_LIMIT: f64 = 0.1;

/// Distinct colours solved exactly; past this, colours are rounded to 6 bits a channel first.
const EXACT_COLORS: usize = 262_144;

fn lin(c: u8) -> f64 {
    let c = f64::from(c) / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// Linear-light RGB of an sRGB colour.
pub fn to_linear(rgb: [u8; 3]) -> [f64; 3] {
    [lin(rgb[0]), lin(rgb[1]), lin(rgb[2])]
}

/// Solve `m x = b` for a small dense system by Gaussian elimination with partial pivoting.
/// None when the system is singular.
fn solve(mut m: Vec<Vec<f64>>, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let n = b.len();
    for col in 0..n {
        let pivot = (col..n).max_by(|&a, &c| m[a][col].abs().total_cmp(&m[c][col].abs()))?;
        if m[pivot][col].abs() < 1e-12 {
            return None;
        }
        m.swap(col, pivot);
        b.swap(col, pivot);
        for row in col + 1..n {
            let f = m[row][col] / m[col][col];
            let (upper, lower) = m.split_at_mut(row);
            for (dst, src) in lower[0].iter_mut().zip(&upper[col]).skip(col) {
                *dst -= f * src;
            }
            let v = b[col];
            b[row] -= f * v;
        }
    }
    let mut x = vec![0.0; n];
    for row in (0..n).rev() {
        let tail: f64 = (row + 1..n).map(|k| m[row][k] * x[k]).sum();
        x[row] = (b[row] - tail) / m[row][row];
    }
    Some(x)
}

/// Shares of the area for a target colour: `points[0]` is the base, the rest are the inks. The
/// result holds one share per point, all at least 0 and summing to 1.
///
/// Every subset of up to four points is tried: the closest point of a 3-D hull lies on a face of at
/// most four vertices. The best subset has the least error, then the least ink.
pub fn solve_shares(points: &[[f64; 3]], target: [f64; 3]) -> Vec<f64> {
    let n = points.len();
    let mut best: Option<(f64, f64, Vec<f64>)> = None; // error, ink, shares
    let mut consider = |subset: &[usize]| {
        let m = subset.len();
        // Minimise |A c - T|^2 with sum(c) = 1: the KKT system [2 A'A, 1; 1', 0].
        let mut mat = vec![vec![0.0; m + 1]; m + 1];
        let mut rhs = vec![0.0; m + 1];
        for (r, &i) in subset.iter().enumerate() {
            for (c, &j) in subset.iter().enumerate() {
                mat[r][c] = 2.0 * (0..3).map(|k| points[i][k] * points[j][k]).sum::<f64>();
            }
            mat[r][m] = 1.0;
            mat[m][r] = 1.0;
            rhs[r] = 2.0 * (0..3).map(|k| points[i][k] * target[k]).sum::<f64>();
        }
        rhs[m] = 1.0;
        let Some(sol) = solve(mat, rhs) else { return };
        if sol.iter().take(m).any(|&c| c < -1e-12) {
            return;
        }
        let mut shares = vec![0.0; n];
        for (r, &i) in subset.iter().enumerate() {
            shares[i] = sol[r].max(0.0);
        }
        let total: f64 = shares.iter().sum();
        if total <= 0.0 {
            return;
        }
        shares.iter_mut().for_each(|s| *s /= total);
        let err: f64 = (0..3).map(|k| (shares.iter().zip(points).map(|(s, p)| s * p[k]).sum::<f64>() - target[k]).powi(2)).sum();
        let ink: f64 = shares.iter().skip(1).sum();
        let better = match &best {
            None => true,
            Some((e, i, _)) => err < e - 1e-12 || (err <= e + 1e-12 && ink < i - 1e-12),
        };
        if better {
            best = Some((err, ink, shares));
        }
    };
    for a in 0..n {
        consider(&[a]);
        for b in a + 1..n {
            consider(&[a, b]);
            for c in b + 1..n {
                consider(&[a, b, c]);
                for d in c + 1..n {
                    consider(&[a, b, c, d]);
                }
            }
        }
    }
    best.map_or_else(
        || {
            let mut s = vec![0.0; n];
            if let Some(first) = s.first_mut() {
                *first = 1.0;
            }
            s
        },
        |(_, _, s)| s,
    )
}

/// One tone plate per ink: the share of the area each ink covers, 0 to 255, for every opaque
/// pixel (background pixels get 0 on every plate). `base` is the colour under the dots.
///
/// `ink_limit` caps the share of the area all inks together may cover (1 = no cap): the darkest
/// tones keep some base showing, which holds open on the mesh, and come out a little lighter.
pub fn simulated_plates(img: &Rgba8, inks: &[[u8; 3]], base: [u8; 3], ink_limit: f64) -> Result<Vec<Gray8>> {
    if inks.is_empty() || inks.len() > MAX_INKS {
        return Err(param("inks", format!("need 1..={MAX_INKS} inks")));
    }
    if !ink_limit.is_finite() || !(MIN_INK_LIMIT..=1.0).contains(&ink_limit) {
        return Err(param("ink_limit", format!("must be {MIN_INK_LIMIT}..=1")));
    }
    let mut points = vec![to_linear(base)];
    points.extend(inks.iter().map(|&c| to_linear(c)));
    let n = img.width() * img.height();
    let mut planes = vec![vec![0u8; n]; inks.len()];
    // Flat art has few colours: each is solved once.
    let mut cache: HashMap<u32, Vec<u8>> = HashMap::new();
    let coarse = img
        .pixels()
        .filter(|p| p[3] >= ALPHA_CUTOFF)
        .map(|p| u32::from_le_bytes([p[0], p[1], p[2], 0]))
        .collect::<std::collections::HashSet<_>>()
        .len()
        > EXACT_COLORS;
    for (i, p) in img.pixels().enumerate() {
        if p[3] < ALPHA_CUTOFF {
            continue;
        }
        let rgb = if coarse { [p[0] & 0xfc | 2, p[1] & 0xfc | 2, p[2] & 0xfc | 2] } else { [p[0], p[1], p[2]] };
        let key = u32::from_le_bytes([rgb[0], rgb[1], rgb[2], 0]);
        let shares = cache.entry(key).or_insert_with(|| {
            let mut s = solve_shares(&points, to_linear(rgb));
            let ink: f64 = s.iter().skip(1).sum();
            if ink > ink_limit {
                s.iter_mut().skip(1).for_each(|c| *c *= ink_limit / ink);
            }
            s.iter().skip(1).map(|c| (c * 255.0).round().clamp(0.0, 255.0) as u8).collect()
        });
        for (plane, &v) in planes.iter_mut().zip(shares.iter()) {
            plane[i] = v;
        }
    }
    planes.into_iter().map(|d| Gray8::new(img.width(), img.height(), d)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: [u8; 3] = [255, 255, 255];
    const RED: [u8; 3] = [200, 30, 30];
    const BLUE: [u8; 3] = [20, 40, 170];
    const YELLOW: [u8; 3] = [250, 220, 20];

    fn composite(points: &[[f64; 3]], shares: &[f64]) -> [f64; 3] {
        let mut out = [0.0; 3];
        for (s, p) in shares.iter().zip(points) {
            for (o, v) in out.iter_mut().zip(p) {
                *o += s * v;
            }
        }
        out
    }

    fn pts(inks: &[[u8; 3]]) -> Vec<[f64; 3]> {
        std::iter::once(WHITE).chain(inks.iter().copied()).map(to_linear).collect()
    }

    #[test]
    fn an_ink_colour_is_all_that_ink_and_the_base_is_no_ink() {
        let p = pts(&[RED, BLUE, YELLOW]);
        let s = solve_shares(&p, to_linear(BLUE));
        assert!((s[2] - 1.0).abs() < 1e-9 && s[0].abs() < 1e-9 && s[1].abs() < 1e-9 && s[3].abs() < 1e-9, "{s:?}");
        let s = solve_shares(&p, to_linear(WHITE));
        assert!((s[0] - 1.0).abs() < 1e-9, "{s:?}");
    }

    #[test]
    fn half_ink_half_base_takes_half_the_area() {
        let p = pts(&[RED, BLUE, YELLOW]);
        let target = composite(&p, &[0.5, 0.5, 0.0, 0.0]);
        let s = solve_shares(&p, target);
        assert!((s[1] - 0.5).abs() < 1e-9 && (s[0] - 0.5).abs() < 1e-9, "{s:?}");
    }

    #[test]
    fn two_inks_side_by_side_share_the_area() {
        let p = pts(&[RED, BLUE]);
        let target = composite(&p, &[0.0, 0.5, 0.5]);
        let s = solve_shares(&p, target);
        assert!((s[1] - 0.5).abs() < 1e-9 && (s[2] - 0.5).abs() < 1e-9 && s[0].abs() < 1e-9, "{s:?}");
    }

    #[test]
    fn colours_inside_the_hull_are_matched_exactly_with_the_least_ink() {
        // Deterministic pseudo-random mixes of the base and six inks.
        let inks = [RED, BLUE, YELLOW, [30, 140, 60], [90, 20, 110], [10, 10, 10]];
        let p = pts(&inks);
        let mut seed = 12345u64;
        let mut next = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) as f64) / f64::from(1u32 << 31)
        };
        for _ in 0..200 {
            let raw: Vec<f64> = (0..p.len()).map(|_| next().powi(3)).collect();
            let total: f64 = raw.iter().sum();
            let want: Vec<f64> = raw.iter().map(|r| r / total).collect();
            let target = composite(&p, &want);
            let got = solve_shares(&p, target);
            let back = composite(&p, &got);
            for (b, t) in back.iter().zip(&target) {
                assert!((b - t).abs() < 1e-7, "{want:?} -> {got:?}");
            }
            let ink_got: f64 = got.iter().skip(1).sum();
            let ink_want: f64 = want.iter().skip(1).sum();
            assert!(ink_got <= ink_want + 1e-9, "uses no more ink than the mix it came from: {ink_got} vs {ink_want}");
        }
    }

    #[test]
    fn colours_outside_the_hull_give_the_closest_valid_mix() {
        let p = pts(&[RED, BLUE]);
        for target in [[0.0, 0.0, 0.0], [1.0, 1.0, 1.0], [0.0, 1.0, 0.0], [5.0, -3.0, 2.0]] {
            let s = solve_shares(&p, target);
            assert!(s.iter().all(|&v| (-1e-9..=1.0 + 1e-9).contains(&v)), "{s:?}");
            assert!((s.iter().sum::<f64>() - 1.0).abs() < 1e-9, "{s:?}");
        }
    }

    #[test]
    fn plates_follow_a_gradient_and_skip_the_background() {
        // White to ink over 11 pixels, then a transparent pixel.
        let mut d = Vec::new();
        for i in 0..=10u32 {
            let t = f64::from(i) / 10.0;
            let w = to_linear(WHITE);
            let c = to_linear(BLUE);
            let lin: Vec<f64> = (0..3).map(|k| w[k] + (c[k] - w[k]) * t).collect();
            let enc = |v: f64| -> u8 {
                let s = if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
                (s * 255.0).round().clamp(0.0, 255.0) as u8
            };
            d.extend_from_slice(&[enc(lin[0]), enc(lin[1]), enc(lin[2]), 255]);
        }
        d.extend_from_slice(&[0, 0, 0, 0]);
        let img = Rgba8::new(12, 1, d).unwrap();
        let plates = simulated_plates(&img, &[BLUE, RED], WHITE, 1.0).unwrap();
        let blue = plates[0].data();
        assert!(blue[0] <= 1 && blue[10] >= 254, "{blue:?}");
        for w in blue[..11].windows(2) {
            assert!(w[1] >= w[0], "coverage never falls as the pixel gets more blue: {blue:?}");
        }
        assert!((i32::from(blue[5]) - 128).abs() <= 3, "halfway is about half coverage: {}", blue[5]);
        assert!(plates[1].data().iter().all(|&v| v <= 1), "red is not needed for blues");
        assert_eq!(blue[11], 0, "the background takes no ink");
    }

    #[test]
    fn bad_inputs_are_errors() {
        let img = Rgba8::new(1, 1, vec![0, 0, 0, 255]).unwrap();
        assert!(simulated_plates(&img, &[], WHITE, 1.0).is_err());
        assert!(simulated_plates(&img, &[RED; MAX_INKS + 1], WHITE, 1.0).is_err());
        for bad in [0.0, 0.05, 1.1, f64::NAN, f64::INFINITY] {
            assert!(simulated_plates(&img, &[RED], WHITE, bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn an_ink_limit_caps_the_total_coverage_and_leaves_light_tones_alone() {
        let d = [0, 0, 0, 255, 255, 255, 255, 255].to_vec(); // black, white
        let img = Rgba8::new(2, 1, d).unwrap();
        let free = simulated_plates(&img, &[[0, 0, 0]], WHITE, 1.0).unwrap();
        assert_eq!(free[0].data(), &[255, 0]);
        let capped = simulated_plates(&img, &[[0, 0, 0]], WHITE, 0.8).unwrap();
        assert_eq!(capped[0].data(), &[204, 0]);
        // Two inks share the cap in proportion.
        let img = Rgba8::new(1, 1, vec![0, 0, 0, 255]).unwrap();
        let p = simulated_plates(&img, &[BLUE, RED], WHITE, 0.5).unwrap();
        let total = u32::from(p[0].data()[0]) + u32::from(p[1].data()[0]);
        assert!(total <= 128, "{total}");
    }
}
