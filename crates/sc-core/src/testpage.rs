//! A print-test page: films to expose and print once to learn what a shop's screens, ink and press
//! will hold, so the defaults (smallest dot, finest line, trap) can be set from a result instead of
//! a rule of thumb.
//!
//! The page is 220 × 90 mm, in three parts, each its own screen:
//!
//! * **tones** (one film): three rows of 20 patches, each patch a flat tone from 0% to 100%
//!   screened as angled dots (row 1), scattered dots (row 2) and a hybrid (row 3). The patches that
//!   come out and the ones that fill in or drop out say the smallest dot and the largest open area
//!   the screen holds, for each kind of screening.
//! * **lines** (one film): eight widths of line, each as a clear line across a solid block (a
//!   negative line) and as a free-standing ink line (a positive line), horizontal and vertical.
//!   The thinnest that prints cleanly says the finest detail.
//! * **trap** (two films, light and dark): pairs of squares that overlap by 0 to 0.5 mm. Print the
//!   light film, then the dark one over it: the smallest overlap that shows no gap at the seam is
//!   the trap the press needs.
//!
//! Every patch and line has its value printed under it as a number, using a 3 × 5 pixel digit font:
//! tones in percent, line widths and traps in hundredths of a millimetre. Tones are screened
//! without the dot limits the jobs use (`min_dot` 0, `max_dot` 1): the page is for finding those.

use crate::error::{Result, param};
use crate::fm::{FmOptions, Hybrid, fm_screen, hybrid_screen};
use crate::halftone::{HalftoneOptions, halftone};
use crate::image::{Bitmap1, Gray8};

/// The page, in millimetres.
pub const PAGE_MM: (f64, f64) = (220.0, 90.0);

/// The tone patches, in percent, left to right.
pub const TONES: [u32; 20] = [0, 2, 3, 5, 8, 10, 15, 20, 30, 40, 50, 60, 70, 80, 90, 92, 95, 97, 98, 100];

/// The line widths, in hundredths of a millimetre.
pub const LINES: [u32; 8] = [5, 8, 10, 15, 20, 30, 50, 100];

/// The trap overlaps, in hundredths of a millimetre.
pub const TRAPS: [u32; 9] = [0, 5, 10, 15, 20, 25, 30, 40, 50];

/// The row of the tones part each screening is on, top to bottom.
pub const TONE_ROWS: [&str; 3] = ["AM", "FM", "Hybrid"];

/// Which parts to make.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parts {
    pub tones: bool,
    pub lines: bool,
    pub trap: bool,
}

/// What to make.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TestPage {
    pub dpi: f64,
    pub parts: Parts,
    /// The angled screen of the first tone row. Its dot limits are not used.
    pub am: HalftoneOptions,
    /// The scattered screen of the second row and the ends of the third. Its limits are not used.
    pub fm: FmOptions,
    /// Where the third row scatters.
    pub hybrid: Hybrid,
}

/// One film of the page.
#[derive(Clone, Debug, PartialEq)]
pub struct TestFilm {
    pub name: &'static str,
    /// A colour to show the film in (it prints in the ink the shop chooses).
    pub rgb: [u8; 3],
    pub bitmap: Bitmap1,
}

/// 3 × 5 pixel digits, one row of three bits (left bit first) per line.
const DIGITS: [[u8; 5]; 10] = [
    [0b111, 0b101, 0b101, 0b101, 0b111],
    [0b010, 0b110, 0b010, 0b010, 0b111],
    [0b111, 0b001, 0b111, 0b100, 0b111],
    [0b111, 0b001, 0b111, 0b001, 0b111],
    [0b101, 0b101, 0b111, 0b001, 0b001],
    [0b111, 0b100, 0b111, 0b001, 0b111],
    [0b111, 0b100, 0b111, 0b101, 0b111],
    [0b111, 0b001, 0b001, 0b001, 0b001],
    [0b111, 0b101, 0b111, 0b101, 0b111],
    [0b111, 0b101, 0b111, 0b001, 0b111],
];

/// A drawing surface in millimetres over a film.
struct Canvas {
    bits: Bitmap1,
    px_per_mm: f64,
    /// Pixels to a pixel of the digit font.
    font_px: usize,
}

impl Canvas {
    fn px(&self, mm: f64) -> usize {
        (mm * self.px_per_mm).round().max(0.0) as usize
    }

    /// Ink the box from (`x0`, `y0`) to (`x1`, `y1`) mm.
    fn fill(&mut self, x0: f64, y0: f64, x1: f64, y1: f64) {
        let (a, b, c, d) = (self.px(x0), self.px(y0), self.px(x1), self.px(y1));
        for y in b..d {
            for x in a..c {
                self.bits.set(x as i64, y as i64, true);
            }
        }
    }

    /// A line `w_mm` wide (at least one pixel) centred on `c` along the axis, from `a` to `b`.
    fn line(&mut self, horizontal: bool, c: f64, a: f64, b: f64, w_mm: f64, ink: bool) {
        let w = self.px(w_mm).max(1);
        let c0 = self.px(c).saturating_sub(w / 2);
        let (a, b) = (self.px(a), self.px(b));
        for along in a..b {
            for across in c0..c0 + w {
                let (x, y) = if horizontal { (along, across) } else { (across, along) };
                self.bits.set(x as i64, y as i64, ink);
            }
        }
    }

    /// `n` as digits with the top left at (`x`, `y`) mm.
    fn number(&mut self, n: u32, x: f64, y: f64) {
        let (k, mut at) = (self.font_px, self.px(x));
        let top = self.px(y);
        for ch in n.to_string().bytes() {
            let Some(glyph) = DIGITS.get(usize::from(ch - b'0')) else { continue };
            for (row, bitsrow) in glyph.iter().enumerate() {
                for col in 0..3 {
                    if bitsrow & (0b100 >> col) != 0 {
                        for dy in 0..k {
                            for dx in 0..k {
                                self.bits.set((at + col * k + dx) as i64, (top + row * k + dy) as i64, true);
                            }
                        }
                    }
                }
            }
            at += 4 * k;
        }
    }
}

fn canvas(dpi: f64) -> Result<Canvas> {
    let px_per_mm = dpi / 25.4;
    let size = |mm: f64| (mm * px_per_mm).round() as usize;
    let bits = Bitmap1::new(size(PAGE_MM.0), size(PAGE_MM.1))?;
    Ok(Canvas { bits, px_per_mm, font_px: ((0.6 * px_per_mm).round() as usize).max(1) })
}

/// The tones part: patches screened row by row, with their numbers.
fn tones(o: &TestPage) -> Result<TestFilm> {
    let mut c = canvas(o.dpi)?;
    let w = c.bits.width();
    let am = HalftoneOptions { min_dot: 0.0, max_dot: 1.0, ..o.am };
    let fm = FmOptions { min_dot: 0.0, max_dot: 1.0, ..o.fm };
    for (row, name) in TONE_ROWS.iter().enumerate() {
        let y0 = 5.0 + row as f64 * 14.0;
        // The patches of this row as a plate, screened on its own.
        let (top, bottom) = (c.px(y0), c.px(y0 + 8.0));
        let mut data = vec![0u8; w * (bottom - top)];
        for (i, tone) in TONES.iter().enumerate() {
            let x0 = 5.0 + i as f64 * 10.5;
            for y in 0..bottom - top {
                for x in c.px(x0)..c.px(x0 + 8.0) {
                    if let Some(v) = data.get_mut(y * w + x) {
                        *v = (f64::from(*tone) * 2.55).round() as u8;
                    }
                }
            }
        }
        let plate = Gray8::new(w, bottom - top, data)?;
        let film = match *name {
            "AM" => halftone(&plate, &am)?,
            "FM" => fm_screen(&plate, &fm)?,
            _ => hybrid_screen(&plate, &am, &fm, &o.hybrid)?,
        };
        for y in 0..film.height() {
            for x in 0..film.width() {
                if film.get(x, y) {
                    c.bits.set(x as i64, (top + y) as i64, true);
                }
            }
        }
        // The row's number at the left, and each patch's tone under it.
        c.number(row as u32 + 1, 1.0, y0 + 1.0);
        for (i, tone) in TONES.iter().enumerate() {
            c.number(*tone, 5.0 + i as f64 * 10.5, y0 + 9.0);
        }
    }
    Ok(TestFilm { name: "Test tones", rgb: [0, 0, 0], bitmap: c.bits })
}

/// The lines part: negative and positive lines of every width.
fn lines(o: &TestPage) -> Result<TestFilm> {
    let mut c = canvas(o.dpi)?;
    for (i, hundredths) in LINES.iter().enumerate() {
        let x0 = 5.0 + i as f64 * 26.0;
        let w_mm = f64::from(*hundredths) / 100.0;
        // A solid block with a clear line across it each way.
        c.fill(x0, 50.0, x0 + 10.0, 60.0);
        c.line(true, 55.0, x0, x0 + 10.0, w_mm, false);
        c.line(false, x0 + 5.0, 50.0, 60.0, w_mm, false);
        // A free-standing ink line each way.
        c.line(true, 52.5, x0 + 12.0, x0 + 22.0, w_mm, true);
        c.line(false, x0 + 17.0, 55.0, 60.0, w_mm, true);
        c.number(*hundredths, x0, 61.5);
    }
    Ok(TestFilm { name: "Test lines", rgb: [0, 0, 0], bitmap: c.bits })
}

/// The trap part: the light film and the dark film.
fn trap(o: &TestPage) -> Result<[TestFilm; 2]> {
    let (mut light, mut dark) = (canvas(o.dpi)?, canvas(o.dpi)?);
    for (j, hundredths) in TRAPS.iter().enumerate() {
        let x0 = 5.0 + j as f64 * 23.0;
        // Both meet at x0 + 10; the light one runs on under the dark one by the trap.
        light.fill(x0, 70.0, x0 + 10.0 + f64::from(*hundredths) / 100.0, 80.0);
        dark.fill(x0 + 10.0, 70.0, x0 + 20.0, 80.0);
        dark.number(*hundredths, x0 + 7.0, 81.5);
    }
    Ok([
        TestFilm { name: "Test trap (light)", rgb: [250, 220, 20], bitmap: light.bits },
        TestFilm { name: "Test trap (dark)", rgb: [10, 20, 90], bitmap: dark.bits },
    ])
}

/// The films of the page asked for, all the same size.
pub fn test_page(o: &TestPage) -> Result<Vec<TestFilm>> {
    if !o.dpi.is_finite() || !(100.0..=1200.0).contains(&o.dpi) {
        return Err(param("dpi", "the test page is made at 100 to 1200 dpi"));
    }
    if !(o.parts.tones || o.parts.lines || o.parts.trap) {
        return Err(param("parts", "choose at least one part of the page"));
    }
    let mut films = Vec::new();
    if o.parts.tones {
        films.push(tones(o)?);
    }
    if o.parts.lines {
        films.push(lines(o)?);
    }
    if o.parts.trap {
        films.extend(trap(o)?);
    }
    Ok(films)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::halftone::DotShape;

    fn page(dpi: f64, parts: Parts) -> TestPage {
        TestPage {
            dpi,
            parts,
            am: HalftoneOptions { dpi, lpi: 45.0, angle_deg: 22.5, shape: DotShape::Round, min_dot: 0.08, max_dot: 0.95, dot_gain: 0.0 },
            fm: FmOptions { cell_px: 5, min_dot: 0.08, max_dot: 0.95, dot_gain: 0.0, kernel: Default::default() },
            hybrid: Hybrid { below: 0.15, above: 0.85 },
        }
    }

    const ALL: Parts = Parts { tones: true, lines: true, trap: true };

    /// The share of ink in a box (mm) of a film at `dpi`.
    fn ink(f: &Bitmap1, dpi: f64, x0: f64, y0: f64, x1: f64, y1: f64) -> f64 {
        let px = |mm: f64| (mm * dpi / 25.4).round() as usize;
        let (mut on, mut n) = (0usize, 0usize);
        for y in px(y0)..px(y1) {
            for x in px(x0)..px(x1) {
                n += 1;
                on += usize::from(f.get(x, y));
            }
        }
        on as f64 / n as f64
    }

    #[test]
    fn every_film_is_the_same_size_and_the_parts_can_be_chosen() {
        let films = test_page(&page(300.0, ALL)).unwrap();
        let names: Vec<_> = films.iter().map(|f| f.name).collect();
        assert_eq!(names, ["Test tones", "Test lines", "Test trap (light)", "Test trap (dark)"]);
        let size = (films[0].bitmap.width(), films[0].bitmap.height());
        assert_eq!(size, (2598, 1063), "220 × 90 mm at 300 dpi");
        assert!(films.iter().all(|f| (f.bitmap.width(), f.bitmap.height()) == size));
        let only = |parts| test_page(&page(300.0, parts)).unwrap().len();
        assert_eq!(only(Parts { tones: true, lines: false, trap: false }), 1);
        assert_eq!(only(Parts { tones: false, lines: false, trap: true }), 2);
        assert!(test_page(&page(300.0, Parts { tones: false, lines: false, trap: false })).is_err());
        for dpi in [50.0, 5000.0, f64::NAN] {
            assert!(test_page(&page(dpi, ALL)).is_err(), "{dpi}");
        }
    }

    #[test]
    fn each_tone_patch_prints_its_tone_in_every_row() {
        let dpi = 600.0;
        let f = &test_page(&page(dpi, Parts { tones: true, lines: false, trap: false })).unwrap()[0].bitmap;
        for (row, _) in TONE_ROWS.iter().enumerate() {
            let y0 = 5.0 + row as f64 * 14.0;
            for (i, tone) in TONES.iter().enumerate() {
                let x0 = 5.0 + i as f64 * 10.5;
                let share = ink(f, dpi, x0 + 1.0, y0 + 1.0, x0 + 7.0, y0 + 7.0);
                let want = f64::from(*tone) / 100.0;
                // 6 mm of patch holds only a few dots, so allow a few percent either way.
                assert!((share - want).abs() < 0.07, "row {row} tone {tone}: {share}");
            }
        }
        // Between the rows, below the labels and above the next patches, nothing prints.
        assert_eq!(ink(f, dpi, 5.0, 17.5, 210.0, 18.5), 0.0);
        // And the labels are there: the 100% label (three digits) is ink under the last patch.
        assert!(ink(f, dpi, 5.0 + 19.0 * 10.5, 14.0, 5.0 + 19.0 * 10.5 + 7.0, 17.0) > 0.2);
    }

    #[test]
    fn lines_are_negative_in_the_block_and_positive_beside_it() {
        let dpi = 600.0;
        let f = &test_page(&page(dpi, Parts { tones: false, lines: true, trap: false })).unwrap()[0].bitmap;
        let px = |mm: f64| (mm * dpi / 25.4).round() as usize;
        for (i, hundredths) in LINES.iter().enumerate() {
            let x0 = 5.0 + i as f64 * 26.0;
            let w = px(f64::from(*hundredths) / 100.0).max(1);
            // The block is solid away from its lines and clear along them.
            assert!(f.get(px(x0 + 2.0), px(52.0)), "block {hundredths}");
            let mid = px(55.0);
            assert!((0..w).all(|d| !f.get(px(x0 + 2.0), mid - w / 2 + d)), "a clear line {hundredths} a whole {w} px wide");
            // The free-standing line is ink and is clear around it.
            let top = px(52.5) - w / 2;
            assert!((0..w).all(|d| f.get(px(x0 + 17.0), top + d)), "positive line {hundredths}");
            assert!(!f.get(px(x0 + 17.0), top.saturating_sub(2)) && !f.get(px(x0 + 17.0), top + w + 2), "{hundredths}");
        }
        // 0.05 mm is a pixel at 600 dpi (a width never goes under one pixel).
        assert!(px(0.05).max(1) >= 1);
    }

    #[test]
    fn the_trap_films_overlap_by_the_trap_and_the_dark_one_is_numbered() {
        let dpi = 600.0;
        let films = test_page(&page(dpi, Parts { tones: false, lines: false, trap: true })).unwrap();
        let (light, dark) = (&films[0].bitmap, &films[1].bitmap);
        let px = |mm: f64| (mm * dpi / 25.4).round() as usize;
        let y = px(75.0);
        for (j, hundredths) in TRAPS.iter().enumerate() {
            let seam = px(5.0 + j as f64 * 23.0 + 10.0);
            let reach = px(f64::from(*hundredths) / 100.0);
            // The light film is ink up to the seam plus the trap; the dark one starts at the seam.
            assert!(light.get(seam - 2, y) && !dark.get(seam - 2, y), "{hundredths}");
            assert!(dark.get(seam + 2, y) && !light.get(seam + 2 + reach, y), "{hundredths}");
            if reach > 2 {
                assert!(light.get(seam + reach - 2, y), "overlap {hundredths}");
            }
        }
        // The numbers are on the dark film only.
        assert_eq!(ink(light, dpi, 5.0, 81.0, 215.0, 88.0), 0.0);
        assert!(ink(dark, dpi, 5.0, 81.0, 215.0, 88.0) > 0.0);
    }

    #[test]
    fn digits_are_drawn_whole() {
        let mut c = canvas(300.0).unwrap();
        c.number(8, 1.0, 1.0);
        let k = c.font_px;
        let (x0, y0) = (c.px(1.0), c.px(1.0));
        // An 8 is solid on every row's edges and open in the two spots of its middle column.
        for row in 0..5 {
            assert!(c.bits.get(x0, y0 + row * k) && c.bits.get(x0 + 2 * k, y0 + row * k), "row {row}");
        }
        assert!(!c.bits.get(x0 + k, y0 + k) && !c.bits.get(x0 + k, y0 + 3 * k));
        assert!(c.bits.get(x0 + k, y0) && c.bits.get(x0 + k, y0 + 2 * k) && c.bits.get(x0 + k, y0 + 4 * k));
    }
}
