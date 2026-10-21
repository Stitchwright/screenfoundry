//! Film files. A film is a 1-bit image whose physical size matters, so both writers record the
//! resolution: [`png_1bit`] and [`tiff_1bit`]. Black is ink (where the screen must open), white is
//! clear film, which is how film positives are made.

use crate::error::{Result, param};
use crate::image::Bitmap1;

/// Highest resolution a film file may declare. A 2,400 dpi imagesetter is the practical top.
pub const MAX_DPI: f64 = 20_000.0;

fn check_dpi(dpi: f64) -> Result<()> {
    if !dpi.is_finite() || dpi <= 0.0 || dpi > MAX_DPI {
        return Err(param("dpi", format!("must be above 0 and at most {MAX_DPI}")));
    }
    Ok(())
}

/// One row of the film as bytes, most significant bit first, with `1` meaning black.
fn packed_rows(film: &Bitmap1) -> Vec<u8> {
    let (w, h) = (film.width(), film.height());
    let stride = w.div_ceil(8);
    let mut out = vec![0u8; stride * h];
    for (y, row) in out.chunks_exact_mut(stride).enumerate() {
        for x in 0..w {
            if film.get(x, y)
                && let Some(b) = row.get_mut(x / 8)
            {
                *b |= 0x80 >> (x % 8);
            }
        }
    }
    out
}

/// A 1-bit grayscale PNG with its resolution in a `pHYs` chunk. In a 1-bit grayscale PNG a set
/// bit is white, so ink is written as `0`.
pub fn png_1bit(film: &Bitmap1, dpi: f64) -> Result<Vec<u8>> {
    check_dpi(dpi)?;
    let mut data = packed_rows(film);
    for b in &mut data {
        *b = !*b;
    }
    // Padding bits at the end of each row stay white; they are never shown.
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, film.width() as u32, film.height() as u32);
    enc.set_color(png::ColorType::Grayscale);
    enc.set_depth(png::BitDepth::One);
    let per_meter = (dpi / 0.0254).round() as u32;
    enc.set_pixel_dims(Some(png::PixelDimensions { xppu: per_meter, yppu: per_meter, unit: png::Unit::Meter }));
    let mut w = enc.write_header().map_err(|e| param("film", e.to_string()))?;
    w.write_image_data(&data).map_err(|e| param("film", e.to_string()))?;
    w.finish().map_err(|e| param("film", e.to_string()))?;
    Ok(out)
}

/// A baseline, uncompressed 1-bit TIFF (little-endian) with its resolution in inches. Photometric
/// interpretation is "white is zero", so a set bit is black ink.
pub fn tiff_1bit(film: &Bitmap1, dpi: f64) -> Result<Vec<u8>> {
    check_dpi(dpi)?;
    let data = packed_rows(film);
    let (w, h) = (film.width() as u32, film.height() as u32);
    const ENTRIES: u16 = 12;
    let ifd_len = 2 + usize::from(ENTRIES) * 12 + 4;
    let rationals = 8 + ifd_len; // two RATIONALs (8 bytes each) follow the IFD
    let strip = rationals + 16;
    // Resolution as a rational: dpi in thousandths.
    let den = 1000u32;
    let num = (dpi * f64::from(den)).round() as u32;

    let mut out = Vec::with_capacity(strip + data.len());
    out.extend_from_slice(b"II*\0");
    out.extend_from_slice(&8u32.to_le_bytes());
    out.extend_from_slice(&ENTRIES.to_le_bytes());
    let mut entry = |tag: u16, ty: u16, count: u32, value: u32| {
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&ty.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&value.to_le_bytes());
    };
    const SHORT: u16 = 3;
    const LONG: u16 = 4;
    const RATIONAL: u16 = 5;
    entry(256, LONG, 1, w); // ImageWidth
    entry(257, LONG, 1, h); // ImageLength
    entry(258, SHORT, 1, 1); // BitsPerSample
    entry(259, SHORT, 1, 1); // Compression: none
    entry(262, SHORT, 1, 0); // PhotometricInterpretation: WhiteIsZero
    entry(273, LONG, 1, strip as u32); // StripOffsets
    entry(277, SHORT, 1, 1); // SamplesPerPixel
    entry(278, LONG, 1, h); // RowsPerStrip
    entry(279, LONG, 1, data.len() as u32); // StripByteCounts
    entry(282, RATIONAL, 1, rationals as u32); // XResolution
    entry(283, RATIONAL, 1, rationals as u32 + 8); // YResolution
    entry(296, SHORT, 1, 2); // ResolutionUnit: inch
    out.extend_from_slice(&0u32.to_le_bytes()); // no next IFD
    for _ in 0..2 {
        out.extend_from_slice(&num.to_le_bytes());
        out.extend_from_slice(&den.to_le_bytes());
    }
    out.extend_from_slice(&data);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// A 13 × 3 film (not a multiple of 8 wide) with a diagonal of ink.
    fn sample() -> Bitmap1 {
        let mut b = Bitmap1::new(13, 3).unwrap();
        for i in 0..3_i64 {
            b.set(i * 5, i, true);
        }
        b
    }

    #[test]
    fn png_is_one_bit_with_the_resolution_and_the_ink_black() {
        let bytes = png_1bit(&sample(), 600.0).unwrap();
        let mut reader = png::Decoder::new(Cursor::new(bytes)).read_info().unwrap();
        let info = reader.info();
        assert_eq!((info.width, info.height), (13, 3));
        assert_eq!(info.bit_depth, png::BitDepth::One);
        assert_eq!(info.color_type, png::ColorType::Grayscale);
        let dims = info.pixel_dims.unwrap();
        assert_eq!(dims.unit, png::Unit::Meter);
        let dpi = f64::from(dims.xppu) * 0.0254;
        assert!((dpi - 600.0).abs() < 0.5, "{dpi}");
        let mut buf = vec![0u8; reader.output_buffer_size().unwrap()];
        let frame = reader.next_frame(&mut buf).unwrap();
        // Row 0: ink at x = 0 is a 0 bit, everything else 1.
        assert_eq!(frame.line_size, 2);
        assert_eq!(buf[0], 0b0111_1111);
        // Row 1: ink at x = 5.
        assert_eq!(buf[2], 0b1111_1011);
    }

    #[test]
    fn tiff_decodes_as_one_bit_with_the_resolution() {
        use tiff::decoder::{Decoder, DecodingResult};
        use tiff::tags::Tag;
        let bytes = tiff_1bit(&sample(), 1200.0).unwrap();
        let mut d = Decoder::new(Cursor::new(bytes)).unwrap();
        assert_eq!(d.dimensions().unwrap(), (13, 3));
        assert_eq!(d.colortype().unwrap(), tiff::ColorType::Gray(1));
        for tag in [Tag::XResolution, Tag::YResolution] {
            let tiff::decoder::ifd::Value::Rational(n, den) = d.get_tag(tag).unwrap() else { panic!("{tag:?} is not a rational") };
            assert!((f64::from(n) / f64::from(den) - 1200.0).abs() < 1e-6, "{tag:?}: {n}/{den}");
        }
        assert_eq!(d.get_tag_u64(Tag::ResolutionUnit).unwrap(), 2);
        // The file says white-is-zero, so a set bit in the file is ink...
        assert_eq!(d.get_tag_u64(Tag::PhotometricInterpretation).unwrap(), 0);
        // ...and the decoder hands back black as 0, as for any other 1-bit image: ink at x = 0 in
        // row 0 is a clear bit, the other twelve pixels of the row are set.
        let DecodingResult::U8(px) = d.read_image().unwrap() else { panic!("not 8-bit samples") };
        assert_eq!(px[0], 0b0111_1111);
        assert_eq!(px[1] & 0b1111_1000, 0b1111_1000);
    }

    #[test]
    fn bad_resolutions_are_errors() {
        for dpi in [0.0, -1.0, f64::NAN, f64::INFINITY, 1e9] {
            assert!(png_1bit(&sample(), dpi).is_err(), "{dpi}");
            assert!(tiff_1bit(&sample(), dpi).is_err(), "{dpi}");
        }
    }

    #[test]
    fn a_large_film_round_trips() {
        let mut b = Bitmap1::new(4097, 33).unwrap();
        for y in 0..33 {
            b.set(4096, y, true);
        }
        let png_len = png_1bit(&b, 300.0).unwrap().len();
        let tiff = tiff_1bit(&b, 300.0).unwrap();
        assert!(png_len > 0);
        assert_eq!(tiff.len(), 8 + 2 + 12 * 12 + 4 + 16 + (4097usize.div_ceil(8)) * 33);
    }
}
