//! Registration marks: a crosshair inside a circle at each corner, outside the artwork, so every
//! film carries the same marks and the screens can be aligned on press.
//!
//! The mark size (about 0.25 to 0.5 inch) and four-corner layout follow common shop practice
//! described in public guides; they are parameters, not a standard.

use crate::error::{Result, ScError, param};
use crate::image::Bitmap1;

/// Where the artwork sits on the film, in film pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarkSpec {
    /// Outer diameter of the circle (and length of the crosshair), in film pixels.
    pub size_px: usize,
    /// Line thickness in film pixels.
    pub line_px: usize,
    /// Gap between the artwork's corner and the nearest edge of the mark.
    pub gap_px: usize,
}

impl MarkSpec {
    /// A 0.375 inch mark with a 0.01 inch line, 0.25 inch from the artwork, at `dpi`.
    pub fn for_dpi(dpi: f64) -> Result<Self> {
        if !dpi.is_finite() || dpi <= 0.0 || dpi > 20_000.0 {
            return Err(param("dpi", "must be 0 < dpi <= 20000"));
        }
        Ok(Self { size_px: (0.375 * dpi).round() as usize, line_px: ((0.01 * dpi).round() as usize).max(1), gap_px: (0.25 * dpi).round() as usize })
    }
}

/// Centres of the four marks, clockwise from the top left, or an error if any would leave the film.
pub fn corner_centers(film_w: usize, film_h: usize, art: Rect, m: &MarkSpec) -> Result<[(usize, usize); 4]> {
    let off = m.gap_px + m.size_px / 2;
    let (x1, y1) = (art.x.saturating_add(art.width), art.y.saturating_add(art.height));
    let half = m.size_px / 2;
    if art.x < off + half || art.y < off + half || x1 + off + half > film_w || y1 + off + half > film_h {
        return Err(ScError::MarksDontFit);
    }
    Ok([(art.x - off, art.y - off), (x1 + off, art.y - off), (x1 + off, y1 + off), (art.x - off, y1 + off)])
}

/// Draw a crosshair-in-circle centred at `(cx, cy)`.
pub fn draw_mark(film: &mut Bitmap1, cx: usize, cy: usize, m: &MarkSpec) {
    let r = (m.size_px / 2) as i64;
    let t = m.line_px as i64;
    let (cx, cy) = (cx as i64, cy as i64);
    for dy in -r..=r {
        for dx in -r..=r {
            let d2 = dx * dx + dy * dy;
            let ring = d2 <= r * r && d2 >= (r - t).max(0).pow(2);
            let cross = dx.abs() * 2 < t + 1 || dy.abs() * 2 < t + 1;
            if ring || cross {
                film.set(cx + dx, cy + dy, true);
            }
        }
    }
}

/// Stamp the four corner marks onto a film.
pub fn add_registration_marks(film: &mut Bitmap1, art: Rect, m: &MarkSpec) -> Result<()> {
    for (cx, cy) in corner_centers(film.width(), film.height(), art, m)? {
        draw_mark(film, cx, cy, m);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_land_at_each_corner_and_are_identical() {
        let m = MarkSpec { size_px: 20, line_px: 2, gap_px: 10 };
        let art = Rect { x: 60, y: 60, width: 100, height: 80 };
        let mut film = Bitmap1::new(220, 200).unwrap();
        add_registration_marks(&mut film, art, &m).unwrap();
        let cs = corner_centers(220, 200, art, &m).unwrap();
        assert_eq!(cs[0], (40, 40));
        assert_eq!(cs[2], (180, 160));
        for (cx, cy) in cs {
            assert!(film.get(cx, cy), "centre of mark at {cx},{cy}");
            assert!(film.get(cx + 10, cy) || film.get(cx + 9, cy), "ring");
        }
        // Nothing inked inside the artwork area.
        for y in 60..140 {
            for x in 60..160 {
                assert!(!film.get(x, y));
            }
        }
    }

    #[test]
    fn marks_that_do_not_fit_are_an_error_not_a_panic() {
        let m = MarkSpec { size_px: 20, line_px: 2, gap_px: 10 };
        let art = Rect { x: 5, y: 5, width: 50, height: 50 };
        let mut film = Bitmap1::new(60, 60).unwrap();
        assert_eq!(add_registration_marks(&mut film, art, &m), Err(ScError::MarksDontFit));
        let huge = Rect { x: usize::MAX, y: 0, width: usize::MAX, height: 1 };
        assert!(corner_centers(10, 10, huge, &m).is_err());
    }

    #[test]
    fn spec_from_dpi() {
        let m = MarkSpec::for_dpi(1200.0).unwrap();
        assert_eq!((m.size_px, m.line_px, m.gap_px), (450, 12, 300));
        assert!(MarkSpec::for_dpi(0.0).is_err());
        assert!(MarkSpec::for_dpi(f64::NAN).is_err());
    }
}
