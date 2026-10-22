//! Direct-to-film (DTF) files: the colour art mirrored for the film, and a white layer under it.
//!
//! A DTF printer prints the colours onto film, then white over them, and the print is transferred
//! to the garment, so the art goes on the film as a mirror image and the white is printed last
//! over the colours. Shops feed the printer's RIP a transparent PNG; whether the RIP makes the
//! white itself or takes it as a second file varies, so both files are made. The white layer is
//! the art's footprint pulled in a little (the choke), so no white shows past the colour edge.
//!
//! The research behind this is thin (one vendor page: transparent PNG or TIFF at 300 dpi, the RIP
//! mirroring and adding white). The choke default and the white as a plain white-on-transparent
//! PNG are choices, not a standard: they are options, and a shop's RIP decides what it needs.

use crate::error::Result;
use crate::image::{Gray8, Rgba8};
use crate::morph::choke;
use crate::separate::ALPHA_CUTOFF;

/// What to make.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DtfOptions {
    /// Flip the art left to right, as it must go on the film.
    pub mirror: bool,
    /// Make the white layer.
    pub white: bool,
    /// How far the white is pulled in from the colour's edges, in pixels.
    pub white_choke_px: f64,
}

/// The files of a DTF job: both the same size.
#[derive(Clone, Debug, PartialEq)]
pub struct DtfFiles {
    /// The colour art, mirrored when asked, transparent where there is none.
    pub color: Rgba8,
    /// White where white ink prints (opaque white, transparent elsewhere), when asked.
    pub white: Option<Rgba8>,
}

fn mirrored(img: &Rgba8) -> Result<Rgba8> {
    let (w, h) = (img.width(), img.height());
    let mut out = Vec::with_capacity(img.data().len());
    for row in img.data().chunks_exact(w * 4) {
        for px in row.chunks_exact(4).rev() {
            out.extend_from_slice(px);
        }
    }
    Rgba8::new(w, h, out)
}

/// The colour file and white layer for `img`.
pub fn prepare(img: &Rgba8, o: &DtfOptions) -> Result<DtfFiles> {
    let color = if o.mirror { mirrored(img)? } else { img.clone() };
    let white = if o.white {
        let footprint = Gray8::new(img.width(), img.height(), img.pixels().map(|p| if p[3] >= ALPHA_CUTOFF { 255 } else { 0 }).collect())?;
        let plate = choke(&footprint, o.white_choke_px)?;
        let mut px: Vec<u8> = plate.data().iter().flat_map(|&v| if v >= 128 { [255, 255, 255, 255] } else { [255, 255, 255, 0] }).collect();
        let layer = Rgba8::new(img.width(), img.height(), std::mem::take(&mut px))?;
        Some(if o.mirror { mirrored(&layer)? } else { layer })
    } else {
        None
    };
    Ok(DtfFiles { color, white })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 12 × 6: red in the left half, blue in the right, a transparent column at the far right.
    fn art() -> Rgba8 {
        let mut d = Vec::new();
        for _ in 0..6 {
            for x in 0..12 {
                d.extend_from_slice(&match x {
                    0..=5 => [200, 30, 30, 255],
                    6..=10 => [20, 40, 170, 255],
                    _ => [0, 0, 0, 0],
                });
            }
        }
        Rgba8::new(12, 6, d).unwrap()
    }

    fn opts(mirror: bool, choke: f64) -> DtfOptions {
        DtfOptions { mirror, white: true, white_choke_px: choke }
    }

    #[test]
    fn the_colour_file_is_the_art_flipped_left_to_right() {
        let f = prepare(&art(), &opts(true, 0.0)).unwrap();
        assert_eq!(f.color.pixel(0, 0), Some([0, 0, 0, 0]), "the transparent column is now at the left");
        assert_eq!(f.color.pixel(1, 3), Some([20, 40, 170, 255]));
        assert_eq!(f.color.pixel(11, 3), Some([200, 30, 30, 255]));
        let flat = prepare(&art(), &opts(false, 0.0)).unwrap();
        assert_eq!(flat.color, art());
        // Mirroring twice gives the art back.
        assert_eq!(mirrored(&f.color).unwrap(), art());
    }

    #[test]
    fn the_white_layer_is_the_footprint_pulled_in_and_mirrored_with_the_colour() {
        let f = prepare(&art(), &opts(false, 0.0)).unwrap();
        let w = f.white.unwrap();
        assert_eq!(w.pixel(3, 2), Some([255, 255, 255, 255]));
        assert_eq!(w.pixel(11, 2), Some([255, 255, 255, 0]), "no white where there is no art");
        // A 1.5 px choke takes the white in from the art's edge (and the canvas edge counts as empty).
        let c = prepare(&art(), &opts(false, 1.5)).unwrap().white.unwrap();
        assert_eq!(c.pixel(0, 3).map(|p| p[3]), Some(0));
        assert_eq!(c.pixel(10, 3).map(|p| p[3]), Some(0), "pulled in from the transparent column");
        assert_eq!(c.pixel(5, 3).map(|p| p[3]), Some(255), "inside the art");
        // Mirrored together: white stays under its colour.
        let m = prepare(&art(), &opts(true, 1.5)).unwrap();
        let (col, wh) = (m.color, m.white.unwrap());
        for y in 0..6 {
            for x in 0..12 {
                if wh.pixel(x, y).unwrap()[3] == 255 {
                    assert_eq!(col.pixel(x, y).unwrap()[3], 255, "white without colour at ({x}, {y})");
                }
            }
        }
        assert!(wh.pixel(6, 3).unwrap()[3] == 255);
    }

    #[test]
    fn no_white_when_not_asked() {
        let f = prepare(&art(), &DtfOptions { mirror: false, white: false, white_choke_px: 1.0 }).unwrap();
        assert!(f.white.is_none());
    }
}
