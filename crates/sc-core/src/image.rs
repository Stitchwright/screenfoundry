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
