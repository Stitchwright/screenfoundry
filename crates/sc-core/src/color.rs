//! sRGB to CIE Lab (D65), so ink matching follows perceived difference rather than RGB distance.

fn lin(c: u8) -> f64 {
    let c = f64::from(c) / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn f(t: f64) -> f64 {
    const D: f64 = 6.0 / 29.0;
    if t > D * D * D { t.cbrt() } else { t / (3.0 * D * D) + 4.0 / 29.0 }
}

/// CIE Lab of an sRGB colour, D65 white.
pub fn srgb_to_lab(rgb: [u8; 3]) -> [f64; 3] {
    let (r, g, b) = (lin(rgb[0]), lin(rgb[1]), lin(rgb[2]));
    let x = (0.4124564 * r + 0.3575761 * g + 0.1804375 * b) / 0.95047;
    let y = 0.2126729 * r + 0.7151522 * g + 0.0721750 * b;
    let z = (0.0193339 * r + 0.1191920 * g + 0.9503041 * b) / 1.08883;
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// Squared CIE76 colour difference.
pub fn delta_e_sq(a: [f64; 3], b: [f64; 3]) -> f64 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}

/// `#rgb` or `#rrggbb` (the `#` is optional) to sRGB.
pub fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let h = s.trim().trim_start_matches('#');
    if !h.is_ascii() {
        return None;
    }
    let v = |i: usize, n: usize| u8::from_str_radix(h.get(i..i + n)?, 16).ok();
    match h.len() {
        3 => Some([v(0, 1)? * 17, v(1, 1)? * 17, v(2, 1)? * 17]),
        6 => Some([v(0, 2)?, v(2, 2)?, v(4, 2)?]),
        _ => None,
    }
}

/// `#rrggbb`, lowercase.
pub fn to_hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips_and_rejects_junk() {
        assert_eq!(parse_hex("#ff8000"), Some([255, 128, 0]));
        assert_eq!(parse_hex("f80"), Some([255, 136, 0]));
        assert_eq!(to_hex([255, 128, 0]), "#ff8000");
        for bad in ["", "#", "#12", "#12345", "#gg0000", "#1234567", "é1234", "#ff80 0"] {
            assert_eq!(parse_hex(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn white_and_black() {
        let w = srgb_to_lab([255, 255, 255]);
        assert!((w[0] - 100.0).abs() < 0.01 && w[1].abs() < 0.01 && w[2].abs() < 0.01);
        assert!(srgb_to_lab([0, 0, 0])[0].abs() < 1e-9);
    }

    #[test]
    fn red_is_positive_a() {
        let r = srgb_to_lab([255, 0, 0]);
        assert!((r[0] - 53.24).abs() < 0.1 && r[1] > 70.0);
    }
}
