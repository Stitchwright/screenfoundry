//! Choke and spread by an exact Euclidean distance, in linear time (Felzenszwalb & Huttenlocher,
//! 2012, "Distance transforms of sampled functions"). Plates are thresholded at 50% first, so
//! this is for solid (line-art / spot) plates, not for halftone tone.

use crate::error::{Result, param};
use crate::image::Gray8;

const INF: f64 = 1e20;
/// Largest choke/spread in pixels. A 1200 dpi film at 0.5 mm is about 24 px.
pub const MAX_RADIUS: f64 = 256.0;

fn edt_1d(f: &[f64], d: &mut [f64], v: &mut [usize], z: &mut [f64]) {
    let n = f.len();
    if n == 0 {
        return;
    }
    let mut k = 0usize;
    v[0] = 0;
    z[0] = -INF;
    z[1] = INF;
    for q in 1..n {
        let mut s;
        loop {
            let p = v[k];
            let (qf, pf) = (q as f64, p as f64);
            s = ((f[q] + qf * qf) - (f[p] + pf * pf)) / (2.0 * qf - 2.0 * pf);
            if s <= z[k] && k > 0 {
                k -= 1;
            } else {
                break;
            }
        }
        k += 1;
        v[k] = q;
        z[k] = s;
        z[k + 1] = INF;
    }
    k = 0;
    for (q, out) in d.iter_mut().enumerate() {
        while z[k + 1] < q as f64 {
            k += 1;
        }
        let dq = q as f64 - v[k] as f64;
        *out = dq * dq + f[v[k]];
    }
}

/// Squared distance from every pixel to the nearest pixel where `seed` is true.
fn dist_sq(seed: &[bool], w: usize, h: usize) -> Vec<f64> {
    let mut g: Vec<f64> = seed.iter().map(|&s| if s { 0.0 } else { INF }).collect();
    let m = w.max(h);
    let (mut v, mut z) = (vec![0usize; m], vec![0.0f64; m + 1]);
    let (mut col, mut out) = (vec![0.0f64; h], vec![0.0f64; h]);
    for x in 0..w {
        for y in 0..h {
            col[y] = g[y * w + x];
        }
        edt_1d(&col, &mut out, &mut v, &mut z);
        for y in 0..h {
            g[y * w + x] = out[y];
        }
    }
    let mut row_out = vec![0.0f64; w];
    for y in 0..h {
        let row: Vec<f64> = g[y * w..(y + 1) * w].to_vec();
        edt_1d(&row, &mut row_out, &mut v, &mut z);
        g[y * w..(y + 1) * w].copy_from_slice(&row_out);
    }
    g
}

fn check(radius_px: f64) -> Result<()> {
    if !radius_px.is_finite() || !(0.0..=MAX_RADIUS).contains(&radius_px) {
        return Err(param("radius_px", format!("must be 0..={MAX_RADIUS}")));
    }
    Ok(())
}

fn mask(plate: &Gray8) -> Vec<bool> {
    plate.data().iter().map(|&v| v >= 128).collect()
}

fn out(plate: &Gray8, on: impl Iterator<Item = bool>) -> Result<Gray8> {
    Gray8::new(plate.width(), plate.height(), on.map(|b| if b { 255 } else { 0 }).collect())
}

/// Grow the inked area by `radius_px` (a disc), thresholding the plate at 50% first.
pub fn spread(plate: &Gray8, radius_px: f64) -> Result<Gray8> {
    check(radius_px)?;
    let m = mask(plate);
    if radius_px == 0.0 {
        return out(plate, m.into_iter());
    }
    let d = dist_sq(&m, plate.width(), plate.height());
    let r2 = radius_px * radius_px;
    out(plate, d.into_iter().map(|v| v <= r2))
}

/// Shrink the inked area by `radius_px` (a disc), thresholding the plate at 50% first. The
/// plate's border counts as empty, so ink touching the border is choked there too.
pub fn choke(plate: &Gray8, radius_px: f64) -> Result<Gray8> {
    check(radius_px)?;
    let m = mask(plate);
    if radius_px == 0.0 {
        return out(plate, m.into_iter());
    }
    // The plate's border counts as background: art that meets the canvas edge is still choked
    // there, because the film ends where the art ends. Pad by one background pixel to say so.
    let (w, h) = (plate.width(), plate.height());
    let (pw, ph) = (w + 2, h + 2);
    let mut bg = vec![true; pw * ph];
    for y in 0..h {
        for x in 0..w {
            bg[(y + 1) * pw + x + 1] = !m[y * w + x];
        }
    }
    let d = dist_sq(&bg, pw, ph);
    let r2 = radius_px * radius_px;
    // A pixel survives only if the nearest background is farther than the radius.
    let keep = (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).map(|(x, y)| d[(y + 1) * pw + x + 1] > r2);
    out(plate, keep)
}

/// Union of plates (any plate inked): the footprint an underbase has to cover.
pub fn union(plates: &[&Gray8]) -> Result<Gray8> {
    let first = plates.first().ok_or_else(|| param("plates", "need at least one plate"))?;
    let mut acc = vec![0u8; first.data().len()];
    for p in plates {
        if p.width() != first.width() || p.height() != first.height() {
            return Err(crate::ScError::SizeMismatch);
        }
        for (a, &v) in acc.iter_mut().zip(p.data()) {
            *a = (*a).max(v);
        }
    }
    Gray8::new(first.width(), first.height(), acc)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(w: usize, h: usize, x0: usize, x1: usize, y0: usize, y1: usize) -> Gray8 {
        let mut d = vec![0u8; w * h];
        for y in y0..y1 {
            for x in x0..x1 {
                d[y * w + x] = 255;
            }
        }
        Gray8::new(w, h, d).unwrap()
    }

    #[test]
    fn choke_shrinks_a_square_by_the_radius_on_each_side() {
        let g = square(30, 30, 5, 25, 5, 25);
        let c = choke(&g, 3.0).unwrap();
        assert_eq!(c.get(7, 15), Some(0));
        assert_eq!(c.get(8, 15), Some(255));
        assert_eq!(c.get(16, 8), Some(255));
        assert_eq!(c.get(16, 7), Some(0));
    }

    #[test]
    fn spread_grows_a_square_by_the_radius_on_each_side() {
        let g = square(30, 30, 10, 20, 10, 20);
        let s = spread(&g, 3.0).unwrap();
        assert_eq!(s.get(7, 15), Some(255));
        assert_eq!(s.get(6, 15), Some(0));
        // A disc, not a square: the corner of the grown box stays empty.
        assert_eq!(s.get(7, 7), Some(0));
    }

    #[test]
    fn zero_radius_only_thresholds() {
        let g = Gray8::new(2, 1, vec![100, 200]).unwrap();
        assert_eq!(choke(&g, 0.0).unwrap().data(), &[0, 255]);
        assert_eq!(spread(&g, 0.0).unwrap().data(), &[0, 255]);
    }

    #[test]
    fn empty_and_full_plates_are_stable() {
        let empty = Gray8::filled(8, 8, 0).unwrap();
        assert_eq!(spread(&empty, 5.0).unwrap(), empty);
        // Full ink survives in the interior only; the border is choked like any other edge.
        let full = Gray8::filled(20, 20, 255).unwrap();
        let c = choke(&full, 3.0).unwrap();
        assert_eq!(c.get(10, 10), Some(255));
        assert_eq!(c.get(0, 10), Some(0));
        assert_eq!(c.get(2, 10), Some(0));
        assert_eq!(c.get(3, 10), Some(255));
    }

    #[test]
    fn rejects_bad_radius() {
        let g = Gray8::filled(4, 4, 0).unwrap();
        assert!(spread(&g, f64::NAN).is_err());
        assert!(choke(&g, -1.0).is_err());
        assert!(spread(&g, 1e9).is_err());
    }

    #[test]
    fn union_takes_the_max() {
        let a = square(6, 1, 0, 2, 0, 1);
        let b = square(6, 1, 4, 6, 0, 1);
        assert_eq!(union(&[&a, &b]).unwrap().data(), &[255, 255, 0, 0, 255, 255]);
        assert!(union(&[]).is_err());
        let c = Gray8::filled(5, 1, 0).unwrap();
        assert!(union(&[&a, &c]).is_err());
    }
}
