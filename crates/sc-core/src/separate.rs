//! Turning artwork into plates.
//!
//! * [`extract_palette`] finds the dominant colours (k-means in Lab, deterministic).
//! * [`separate_spot`] assigns each opaque pixel to its nearest ink: one solid plate per ink.
//! * [`tone_plate`] gives one ink's continuous coverage (for halftoning a single-ink job).
//!
//! Simulated-process separation (overlapping halftone channels) is the next stage; see the spec.

use crate::color::{delta_e_sq, srgb_to_lab};
use crate::error::{Result, param};
use crate::image::{Gray8, Rgba8};

/// Pixels below this alpha are background: they take no ink.
pub const ALPHA_CUTOFF: u8 = 128;

const SAMPLE_CAP: usize = 65_536;
const ITERATIONS: usize = 12;

/// Dominant colours of the opaque pixels, most frequent first. May return fewer than `k` when the
/// artwork has fewer distinct colours.
pub fn extract_palette(img: &Rgba8, k: usize) -> Result<Vec<[u8; 3]>> {
    if k == 0 || k > 64 {
        return Err(param("k", "palette size must be 1..=64"));
    }
    let total = img.width() * img.height();
    let stride = (total / SAMPLE_CAP).max(1);
    let samples: Vec<[u8; 3]> = img.pixels().step_by(stride).filter(|p| p[3] >= ALPHA_CUTOFF).map(|p| [p[0], p[1], p[2]]).collect();
    if samples.is_empty() {
        return Ok(Vec::new());
    }
    let labs: Vec<[f64; 3]> = samples.iter().map(|&c| srgb_to_lab(c)).collect();

    // Farthest-point initialisation: deterministic, and it keeps small accent colours alive.
    let mut centers: Vec<[f64; 3]> = vec![labs[0]];
    while centers.len() < k {
        let mut best = (0.0, None);
        for (i, l) in labs.iter().enumerate() {
            let d = centers.iter().map(|c| delta_e_sq(*c, *l)).fold(f64::MAX, f64::min);
            if d > best.0 {
                best = (d, Some(i));
            }
        }
        match best.1 {
            Some(i) if best.0 > 1.0 => centers.push(labs[i]),
            _ => break, // every remaining sample is within 1 dE of a centre
        }
    }

    let mut assign = vec![0usize; labs.len()];
    for _ in 0..ITERATIONS {
        for (a, l) in assign.iter_mut().zip(&labs) {
            *a = nearest(&centers, *l);
        }
        let mut sum = vec![[0.0f64; 3]; centers.len()];
        let mut n = vec![0usize; centers.len()];
        for (a, l) in assign.iter().zip(&labs) {
            for c in 0..3 {
                sum[*a][c] += l[c];
            }
            n[*a] += 1;
        }
        for (i, c) in centers.iter_mut().enumerate() {
            if n[i] > 0 {
                *c = [sum[i][0] / n[i] as f64, sum[i][1] / n[i] as f64, sum[i][2] / n[i] as f64];
            }
        }
    }

    // Report the mean sRGB of each cluster's members, ordered by size.
    let mut acc = vec![([0u64; 3], 0u64); centers.len()];
    for (a, s) in assign.iter().zip(&samples) {
        for (sum, &v) in acc[*a].0.iter_mut().zip(s) {
            *sum += u64::from(v);
        }
        acc[*a].1 += 1;
    }
    let mut out: Vec<(u64, [u8; 3])> =
        acc.into_iter().filter(|(_, n)| *n > 0).map(|(s, n)| (n, [(s[0] / n) as u8, (s[1] / n) as u8, (s[2] / n) as u8])).collect();
    out.sort_by_key(|b| std::cmp::Reverse(b.0));
    Ok(out.into_iter().map(|(_, c)| c).collect())
}

fn nearest(centers: &[[f64; 3]], l: [f64; 3]) -> usize {
    let mut best = (f64::MAX, 0);
    for (i, c) in centers.iter().enumerate() {
        let d = delta_e_sq(*c, l);
        if d < best.0 {
            best = (d, i);
        }
    }
    best.1
}

/// One solid plate per ink (255 where the pixel is closest to that ink, else 0). Background
/// pixels (alpha below [`ALPHA_CUTOFF`]) appear on no plate. Plates never overlap.
pub fn separate_spot(img: &Rgba8, inks: &[[u8; 3]]) -> Result<Vec<Gray8>> {
    if inks.is_empty() || inks.len() > 64 {
        return Err(param("inks", "need 1..=64 inks"));
    }
    let labs: Vec<[f64; 3]> = inks.iter().map(|&c| srgb_to_lab(c)).collect();
    let n = img.width() * img.height();
    let mut planes = vec![vec![0u8; n]; inks.len()];
    // Artwork is mostly flat colour: remember the last pixel's answer.
    let mut last: Option<([u8; 3], usize)> = None;
    for (i, p) in img.pixels().enumerate() {
        if p[3] < ALPHA_CUTOFF {
            continue;
        }
        let rgb = [p[0], p[1], p[2]];
        let k = match last {
            Some((c, k)) if c == rgb => k,
            _ => nearest(&labs, srgb_to_lab(rgb)),
        };
        last = Some((rgb, k));
        planes[k][i] = 255;
    }
    planes.into_iter().map(|d| Gray8::new(img.width(), img.height(), d)).collect()
}

/// Coverage of one ink needed to reproduce each pixel on a white substrate: the pixel's position
/// on the line from white (0) to the ink (255). Background pixels get 0.
pub fn tone_plate(img: &Rgba8, ink: [u8; 3]) -> Result<Gray8> {
    let ink_lab = srgb_to_lab(ink);
    let white = srgb_to_lab([255, 255, 255]);
    let axis = [ink_lab[0] - white[0], ink_lab[1] - white[1], ink_lab[2] - white[2]];
    let len_sq = axis.iter().map(|v| v * v).sum::<f64>();
    if len_sq < 1.0 {
        return Err(param("ink", "ink is indistinguishable from white"));
    }
    let data = img
        .pixels()
        .map(|p| {
            if p[3] < ALPHA_CUTOFF {
                return 0;
            }
            let l = srgb_to_lab([p[0], p[1], p[2]]);
            let t = ((l[0] - white[0]) * axis[0] + (l[1] - white[1]) * axis[1] + (l[2] - white[2]) * axis[2]) / len_sq;
            (t.clamp(0.0, 1.0) * 255.0).round() as u8
        })
        .collect();
    Gray8::new(img.width(), img.height(), data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(w: usize, h: usize, left: [u8; 3], right: [u8; 3]) -> Rgba8 {
        let mut d = Vec::new();
        for _ in 0..h {
            for x in 0..w {
                let c = if x < w / 2 { left } else { right };
                d.extend_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        Rgba8::new(w, h, d).unwrap()
    }

    #[test]
    fn palette_finds_two_colours() {
        let img = flat(20, 10, [200, 30, 30], [20, 20, 160]);
        let p = extract_palette(&img, 4).unwrap();
        assert_eq!(p.len(), 2);
        assert!(p.contains(&[200, 30, 30]) && p.contains(&[20, 20, 160]));
    }

    #[test]
    fn spot_plates_partition_the_artwork() {
        let img = flat(20, 10, [210, 40, 40], [30, 30, 150]);
        let plates = separate_spot(&img, &[[220, 30, 30], [20, 20, 170]]).unwrap();
        assert_eq!(plates.len(), 2);
        assert!((plates[0].coverage() - 0.5).abs() < 1e-9);
        for (a, b) in plates[0].data().iter().zip(plates[1].data()) {
            assert_eq!(u16::from(*a) + u16::from(*b), 255);
        }
    }

    #[test]
    fn transparent_pixels_take_no_ink() {
        let img = Rgba8::new(2, 1, vec![255, 0, 0, 0, 255, 0, 0, 255]).unwrap();
        let plates = separate_spot(&img, &[[255, 0, 0]]).unwrap();
        assert_eq!(plates[0].data(), &[0, 255]);
    }

    #[test]
    fn tone_plate_runs_from_white_to_ink() {
        let img = Rgba8::new(3, 1, vec![255, 255, 255, 255, 128, 128, 128, 255, 0, 0, 0, 255]).unwrap();
        let t = tone_plate(&img, [0, 0, 0]).unwrap();
        assert_eq!(t.data()[0], 0);
        assert_eq!(t.data()[2], 255);
        assert!(t.data()[1] > 100 && t.data()[1] < 200);
    }

    #[test]
    fn bad_parameters_are_errors() {
        let img = flat(4, 4, [0, 0, 0], [255, 255, 255]);
        assert!(extract_palette(&img, 0).is_err());
        assert!(separate_spot(&img, &[]).is_err());
        assert!(tone_plate(&img, [255, 255, 255]).is_err());
    }
}
