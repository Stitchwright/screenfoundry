//! Pixel containers. Constructors check `width * height` against the buffer and a size cap, so the
//! algorithms may index freely inside them.

use crate::error::{Result, ScError};

/// Largest image the pipeline accepts (100 megapixels), so untrusted input cannot exhaust memory.
pub const MAX_PIXELS: usize = 100_000_000;

fn check(width: usize, height: usize, len: usize, per_pixel: usize) -> Result<()> {
    let n = width.checked_mul(height).ok_or(ScError::TooLarge { width, height, max: MAX_PIXELS })?;
    if n == 0 {
        return Err(ScError::Empty);
    }
    if n > MAX_PIXELS {
        return Err(ScError::TooLarge { width, height, max: MAX_PIXELS });
    }
    if n.checked_mul(per_pixel) != Some(len) {
        return Err(ScError::BadBuffer { width, height, len });
    }
    Ok(())
}

/// Straight (non-premultiplied) 8-bit sRGB artwork, row-major.
#[derive(Clone, Debug, PartialEq)]
pub struct Rgba8 {
    width: usize,
    height: usize,
    data: Vec<u8>,
}

impl Rgba8 {
    pub fn new(width: usize, height: usize, data: Vec<u8>) -> Result<Self> {
        check(width, height, data.len(), 4)?;
        Ok(Self { width, height, data })
    }
    pub fn width(&self) -> usize {
        self.width
    }
    pub fn height(&self) -> usize {
        self.height
    }
    pub fn data(&self) -> &[u8] {
        &self.data
    }
    pub fn pixel(&self, x: usize, y: usize) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = (y * self.width + x) * 4;
        let p = self.data.get(i..i + 4)?;
        Some([p[0], p[1], p[2], p[3]])
    }
    /// Iterate pixels as `[r, g, b, a]`.
    pub fn pixels(&self) -> impl Iterator<Item = [u8; 4]> + '_ {
        self.data.chunks_exact(4).map(|p| [p[0], p[1], p[2], p[3]])
    }
}

/// One 8-bit channel: how much ink a plate wants at each pixel (255 = full).
#[derive(Clone, Debug, PartialEq)]
pub struct Gray8 {
    width: usize,
    height: usize,
    data: Vec<u8>,
}

impl Gray8 {
    pub fn new(width: usize, height: usize, data: Vec<u8>) -> Result<Self> {
        check(width, height, data.len(), 1)?;
        Ok(Self { width, height, data })
    }
    pub fn filled(width: usize, height: usize, value: u8) -> Result<Self> {
        check(width, height, width.saturating_mul(height), 1)?;
        Ok(Self { width, height, data: vec![value; width * height] })
    }
    pub fn width(&self) -> usize {
        self.width
    }
    pub fn height(&self) -> usize {
        self.height
    }
    pub fn data(&self) -> &[u8] {
        &self.data
    }
    pub fn get(&self, x: usize, y: usize) -> Option<u8> {
        if x >= self.width || y >= self.height {
            return None;
        }
        self.data.get(y * self.width + x).copied()
    }
    /// Fraction of the area covered, 0..=1.
    pub fn coverage(&self) -> f64 {
        let sum: u64 = self.data.iter().map(|&v| u64::from(v)).sum();
        sum as f64 / (255.0 * self.data.len() as f64)
    }
}

/// A 1-bit film positive: `true` where ink must reach the garment, which is where the film is
/// black and blocks the exposure light so the emulsion washes out.
#[derive(Clone, Debug, PartialEq)]
pub struct Bitmap1 {
    width: usize,
    height: usize,
    bits: Vec<bool>,
}

impl Bitmap1 {
    pub fn new(width: usize, height: usize) -> Result<Self> {
        check(width, height, width.saturating_mul(height), 1)?;
        Ok(Self { width, height, bits: vec![false; width * height] })
    }
    pub fn from_bits(width: usize, height: usize, bits: Vec<bool>) -> Result<Self> {
        check(width, height, bits.len(), 1)?;
        Ok(Self { width, height, bits })
    }
    pub fn width(&self) -> usize {
        self.width
    }
    pub fn height(&self) -> usize {
        self.height
    }
    pub fn bits(&self) -> &[bool] {
        &self.bits
    }
    /// The film flipped left to right, as for printing emulsion side down.
    pub fn mirrored(&self) -> Self {
        let mut bits = Vec::with_capacity(self.bits.len());
        for row in self.bits.chunks_exact(self.width.max(1)) {
            bits.extend(row.iter().rev());
        }
        Self { width: self.width, height: self.height, bits }
    }
    pub fn get(&self, x: usize, y: usize) -> bool {
        x < self.width && y < self.height && self.bits.get(y * self.width + x).copied().unwrap_or(false)
    }
    /// Set a pixel; writes outside the film are ignored (marks may graze the edge).
    pub fn set(&mut self, x: i64, y: i64, value: bool) {
        if x < 0 || y < 0 {
            return;
        }
        let (x, y) = (x as usize, y as usize);
        if x < self.width
            && y < self.height
            && let Some(b) = self.bits.get_mut(y * self.width + x)
        {
            *b = value;
        }
    }
    pub fn coverage(&self) -> f64 {
        self.bits.iter().filter(|&&b| b).count() as f64 / self.bits.len() as f64
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn rejects_empty_dimensions() {
        assert!(matches!(Rgba8::new(0, 5, vec![]), Err(ScError::Empty)));
        assert!(matches!(Gray8::new(5, 0, vec![]), Err(ScError::Empty)));
        assert!(matches!(Gray8::filled(0, 0, 7), Err(ScError::Empty)));
        assert!(matches!(Bitmap1::new(0, 3), Err(ScError::Empty)));
        assert!(matches!(Bitmap1::from_bits(3, 0, vec![]), Err(ScError::Empty)));
    }

    #[test]
    fn rejects_wrong_buffer_length() {
        assert!(matches!(Rgba8::new(2, 2, vec![0; 15]), Err(ScError::BadBuffer { .. })));
        assert!(matches!(Rgba8::new(2, 2, vec![0; 17]), Err(ScError::BadBuffer { .. })));
        assert!(matches!(Gray8::new(3, 3, vec![0; 8]), Err(ScError::BadBuffer { .. })));
        assert!(matches!(Bitmap1::from_bits(3, 3, vec![false; 10]), Err(ScError::BadBuffer { .. })));
    }

    #[test]
    fn rejects_oversized_and_overflowing_dimensions() {
        // Over the cap, without allocating a matching buffer.
        assert!(matches!(Gray8::filled(MAX_PIXELS + 1, 1, 0), Err(ScError::TooLarge { .. })));
        assert!(matches!(Bitmap1::new(20_000, 20_000), Err(ScError::TooLarge { .. })));
        // width * height overflows usize.
        assert!(matches!(Gray8::filled(usize::MAX, 2, 0), Err(ScError::TooLarge { .. })));
        assert!(matches!(Rgba8::new(usize::MAX, usize::MAX, vec![]), Err(ScError::TooLarge { .. })));
        assert!(matches!(Bitmap1::new(usize::MAX, usize::MAX), Err(ScError::TooLarge { .. })));
    }

    #[test]
    fn rgba_pixel_access_and_bounds() {
        let img = Rgba8::new(2, 1, vec![1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        assert_eq!(img.pixel(0, 0), Some([1, 2, 3, 4]));
        assert_eq!(img.pixel(1, 0), Some([5, 6, 7, 8]));
        assert_eq!(img.pixel(2, 0), None);
        assert_eq!(img.pixel(0, 1), None);
        assert_eq!(img.pixel(usize::MAX, usize::MAX), None);
        assert_eq!(img.pixels().count(), 2);
    }

    #[test]
    fn gray_get_coverage() {
        let g = Gray8::new(2, 2, vec![0, 255, 255, 0]).unwrap();
        assert_eq!(g.get(1, 0), Some(255));
        assert_eq!(g.get(2, 0), None);
        assert_eq!(g.get(0, 2), None);
        assert!((g.coverage() - 0.5).abs() < 1e-12);
        assert_eq!(Gray8::filled(3, 3, 0).unwrap().coverage(), 0.0);
        assert_eq!(Gray8::filled(3, 3, 255).unwrap().coverage(), 1.0);
    }

    #[test]
    fn bitmap_set_ignores_out_of_range_writes() {
        let mut b = Bitmap1::new(3, 2).unwrap();
        b.set(-1, 0, true);
        b.set(0, -1, true);
        b.set(3, 0, true);
        b.set(0, 2, true);
        b.set(i64::MAX, i64::MAX, true);
        b.set(i64::MIN, i64::MIN, true);
        assert_eq!(b.coverage(), 0.0);
        b.set(2, 1, true);
        assert!(b.get(2, 1));
        assert!(!b.get(3, 1));
        assert!(!b.get(usize::MAX, usize::MAX));
        assert!((b.coverage() - 1.0 / 6.0).abs() < 1e-12);
    }

    #[test]
    fn bitmap_mirror_flips_rows_and_round_trips() {
        let b = Bitmap1::from_bits(3, 2, vec![true, false, false, false, true, true]).unwrap();
        let m = b.mirrored();
        assert_eq!(m.bits(), &[false, false, true, true, true, false]);
        assert_eq!((m.width(), m.height()), (3, 2));
        assert_eq!(m.mirrored(), b);
    }

    #[test]
    fn single_pixel_edge_cases() {
        let b = Bitmap1::from_bits(1, 1, vec![true]).unwrap();
        assert_eq!(b.mirrored(), b);
        assert_eq!(b.coverage(), 1.0);
    }
}
