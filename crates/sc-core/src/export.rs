//! Film files. A film is a 1-bit image whose physical size matters, so both writers record the
//! resolution: [`png_rgba`] for art with transparency, [`png_1bit`], [`tiff_1bit`], [`tiff_g4`] and [`pdf_g4`]. Black is ink (where the screen must open), white is
//! clear film, which is how film positives are made.

use crate::error::{Result, param};
use crate::image::{Bitmap1, Rgba8};
use crate::pdfout::{self, Pdf};

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

/// An 8-bit RGBA PNG (straight alpha) with its resolution in a `pHYs` chunk, for artwork that
/// keeps its colour and transparency (direct-to-film files).
pub fn png_rgba(img: &Rgba8, dpi: f64) -> Result<Vec<u8>> {
    check_dpi(dpi)?;
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, img.width() as u32, img.height() as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let per_meter = (dpi / 0.0254).round() as u32;
    enc.set_pixel_dims(Some(png::PixelDimensions { xppu: per_meter, yppu: per_meter, unit: png::Unit::Meter }));
    let mut w = enc.write_header().map_err(|e| param("image", e.to_string()))?;
    w.write_image_data(img.data()).map_err(|e| param("image", e.to_string()))?;
    w.finish().map_err(|e| param("image", e.to_string()))?;
    Ok(out)
}

/// A baseline, uncompressed 1-bit TIFF (little-endian) with its resolution in inches. Photometric
/// interpretation is "white is zero", so a set bit is black ink.
pub fn tiff_1bit(film: &Bitmap1, dpi: f64) -> Result<Vec<u8>> {
    check_dpi(dpi)?;
    Ok(tiff_file(film, dpi, 1, &packed_rows(film)))
}

/// A 1-bit TIFF compressed with CCITT Group 4 (T.6), the usual format for film positives: flat
/// ink areas shrink to a few hundred bytes where the raw file is megabytes. Same tags as
/// [`tiff_1bit`]. Group 4 codes widths as 16 bits, so a film wider than 65,535 pixels is refused.
pub fn tiff_g4(film: &Bitmap1, dpi: f64) -> Result<Vec<u8>> {
    check_dpi(dpi)?;
    Ok(tiff_file(film, dpi, 4, &g4_data(film)?))
}

/// The CCITT Group 4 code of `film`, ink as black, ending with the end-of-block marker.
fn g4_data(film: &Bitmap1) -> Result<Vec<u8>> {
    use fax::{Color, VecWriter, encoder::Encoder};
    let width = u16::try_from(film.width()).map_err(|_| param("film", "too wide for CCITT Group 4 (65,535 pixels at most)"))?;
    let mut enc = Encoder::new(VecWriter::new());
    for y in 0..film.height() {
        let row = (0..film.width()).map(|x| if film.get(x, y) { Color::Black } else { Color::White });
        // `VecWriter` cannot fail, so the error is `Infallible`; the `map_err` keeps it honest.
        enc.encode_line(row, width).map_err(|_| param("film", "CCITT encoding failed"))?;
    }
    Ok(enc.finish().map_err(|_| param("film", "CCITT encoding failed"))?.finish())
}

/// The longest side a PDF page may have, in points (the limit of PDF 1.4 viewers).
const PDF_MAX_POINTS: f64 = 14_400.0;

/// A one-page PDF holding the film as a Group 4 image at its physical size (the page is
/// `pixels × 72 / dpi` points), ink black on white. The image is drawn edge to edge, so a viewer's
/// "actual size" is the film's size. A film longer than 200 inches is refused.
pub fn pdf_g4(film: &Bitmap1, dpi: f64) -> Result<Vec<u8>> {
    check_dpi(dpi)?;
    let data = g4_data(film)?;
    let (w, h) = (film.width(), film.height());
    let pts = |px: usize| px as f64 * 72.0 / dpi;
    let (pw, ph) = (pts(w), pts(h));
    if pw > PDF_MAX_POINTS || ph > PDF_MAX_POINTS {
        return Err(param("film", "larger than a PDF page can be (200 inches)"));
    }
    let mut pdf = Pdf::new();
    let (catalog, pages, page, contents, image) = (pdf.reserve(), pdf.reserve(), pdf.reserve(), pdf.reserve(), pdf.reserve());
    pdf.set(catalog, format!("<< /Type /Catalog /Pages {pages} 0 R >>").as_bytes());
    pdf.set(pages, format!("<< /Type /Pages /Kids [{page} 0 R] /Count 1 >>").as_bytes());
    pdf.set(
        page,
        format!(
            "<< /Type /Page /Parent {pages} 0 R /MediaBox [0 0 {pw:.4} {ph:.4}] /Resources << /XObject << /Im0 {image} 0 R >> >> /Contents {contents} 0 R >>"
        )
        .as_bytes(),
    );
    pdf.set(contents, &pdfout::stream("", format!("q {pw:.4} 0 0 {ph:.4} 0 0 cm /Im0 Do Q").as_bytes()));
    let dict = format!(
        "/Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceGray /BitsPerComponent 1 \
         /Filter /CCITTFaxDecode /DecodeParms << /K -1 /Columns {w} /Rows {h} >>"
    );
    pdf.set(image, &pdfout::stream(&dict, &data));
    pdf.finish(catalog)
}

/// The TIFF container shared by the writers: one strip, white is zero, `compression` is the
/// TIFF code (1 none, 4 CCITT Group 4).
fn tiff_file(film: &Bitmap1, dpi: f64, compression: u16, data: &[u8]) -> Vec<u8> {
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
    entry(259, SHORT, 1, u32::from(compression)); // Compression
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
    out.extend_from_slice(data);
    out
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

    /// What a TIFF decodes to: size, resolution tag and the packed pixels.
    fn decode_tiff(bytes: Vec<u8>) -> ((u32, u32), tiff::decoder::ifd::Value, Vec<u8>) {
        use tiff::decoder::{Decoder, DecodingResult};
        let mut d = Decoder::new(Cursor::new(bytes)).unwrap();
        let size = d.dimensions().unwrap();
        let res = d.get_tag(tiff::tags::Tag::XResolution).unwrap();
        let DecodingResult::U8(px) = d.read_image().unwrap() else { panic!("not 8-bit samples") };
        (size, res, px)
    }

    #[test]
    fn group4_decodes_to_the_same_pixels_as_the_raw_tiff() {
        // Widths either side of byte and 64-pixel boundaries, with runs long enough to need make-up
        // codes, lone pixels, solid rows, empty rows and noise.
        for (w, h) in [(1, 1), (7, 5), (13, 3), (64, 9), (65, 9), (200, 40), (2600, 6)] {
            let mut b = Bitmap1::new(w, h).unwrap();
            let mut seed = 12345u32;
            for y in 0..h {
                for x in 0..w {
                    seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    let ink = match y % 4 {
                        0 => false,
                        1 => true,
                        2 => (seed >> 24) < 40,
                        _ => (x / 3 + y) % 2 == 0,
                    };
                    b.set(x as i64, y as i64, ink);
                }
            }
            let raw = decode_tiff(tiff_1bit(&b, 600.0).unwrap());
            let g4 = decode_tiff(tiff_g4(&b, 600.0).unwrap());
            assert_eq!(raw.0, g4.0, "{w}x{h}");
            assert_eq!(raw.2, g4.2, "{w}x{h} pixels");
            assert_eq!(format!("{:?}", raw.1), format!("{:?}", g4.1), "{w}x{h} resolution");
        }
    }

    #[test]
    fn group4_shrinks_flat_film_and_states_its_compression() {
        use tiff::decoder::Decoder;
        let mut b = Bitmap1::new(3000, 2000).unwrap();
        for y in 500..1500 {
            for x in 800..2200 {
                b.set(x, y, true);
            }
        }
        let g4 = tiff_g4(&b, 1200.0).unwrap();
        assert!(g4.len() < 2_000, "{} bytes", g4.len());
        assert!(g4.len() * 100 < tiff_1bit(&b, 1200.0).unwrap().len());
        let mut d = Decoder::new(Cursor::new(g4)).unwrap();
        assert_eq!(d.get_tag_u64(tiff::tags::Tag::Compression).unwrap(), 4);
        assert_eq!(d.get_tag_u64(tiff::tags::Tag::ResolutionUnit).unwrap(), 2);
    }

    #[test]
    fn pdf_has_a_valid_xref_the_film_size_and_decodable_pixels() {
        let mut b = Bitmap1::new(300, 150).unwrap();
        for y in 20..90 {
            for x in 40..250 {
                b.set(x, y, (x + y) % 7 != 0);
            }
        }
        let pdf = pdf_g4(&b, 300.0).unwrap();
        assert!(pdf.starts_with(b"%PDF-1.4"));
        let text = String::from_utf8_lossy(&pdf);
        // 300 × 150 px at 300 dpi is 72 × 36 points.
        assert!(text.contains("/MediaBox [0 0 72.0000 36.0000]"), "{text}");
        // Each xref entry points at its "N 0 obj".
        let xref = text.rfind("startxref\n").unwrap();
        let start: usize = text[xref + 10..].lines().next().unwrap().parse().unwrap();
        assert!(pdf[start..].starts_with(b"xref\n0 6\n"));
        let table = String::from_utf8_lossy(&pdf[start..]).into_owned();
        for n in 1..=5usize {
            let line = table.lines().nth(n + 2).unwrap();
            let off: usize = line[..10].parse().unwrap();
            assert!(pdf[off..].starts_with(format!("{n} 0 obj").as_bytes()), "object {n}");
        }
        // The image stream is the Group 4 code of the film: decode it and compare every pixel.
        let key = b"stream\n";
        let img_obj = pdf.windows(15).position(|w| w == b"/Subtype /Image").unwrap();
        let rest = String::from_utf8_lossy(&pdf[img_obj..]).into_owned();
        let from = img_obj + rest.find("stream\n").unwrap() + key.len();
        let len: usize = rest.split("/Length ").nth(1).unwrap().split_whitespace().next().unwrap().parse().unwrap();
        let mut y = 0;
        fax::decoder::decode_g4(pdf[from..from + len].iter().copied(), 300, Some(150), |line| {
            for (x, c) in fax::decoder::pels(line, 300).enumerate() {
                assert_eq!(c == fax::Color::Black, b.get(x, y), "({x}, {y})");
            }
            y += 1;
        })
        .unwrap();
        assert_eq!(y, 150);
    }

    #[test]
    fn pdf_refuses_pages_past_the_pdf_limit() {
        let long = Bitmap1::new(10, 4000).unwrap();
        assert!(pdf_g4(&long, 1.0).is_err()); // 4000 inches
        assert!(pdf_g4(&long, 600.0).is_ok());
        assert!(pdf_g4(&long, f64::NAN).is_err());
    }

    #[test]
    fn group4_refuses_what_it_cannot_code() {
        let wide = Bitmap1::new(65_536, 1).unwrap();
        assert!(tiff_g4(&wide, 300.0).is_err());
        assert!(tiff_g4(&Bitmap1::new(65_535, 1).unwrap(), 300.0).is_ok());
    }

    #[test]
    fn rgba_png_keeps_colour_alpha_and_resolution() {
        let img = Rgba8::new(2, 1, vec![200, 30, 30, 255, 1, 2, 3, 0]).unwrap();
        let bytes = png_rgba(&img, 300.0).unwrap();
        let mut reader = png::Decoder::new(Cursor::new(bytes)).read_info().unwrap();
        assert_eq!((reader.info().color_type, reader.info().bit_depth), (png::ColorType::Rgba, png::BitDepth::Eight));
        let dims = reader.info().pixel_dims.unwrap();
        assert!((f64::from(dims.xppu) * 0.0254 - 300.0).abs() < 0.5);
        let mut buf = vec![0u8; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut buf).unwrap();
        assert_eq!(&buf[..8], img.data());
        assert!(png_rgba(&img, 0.0).is_err() && png_rgba(&img, f64::NAN).is_err());
    }

    #[test]
    fn bad_resolutions_are_errors() {
        for dpi in [0.0, -1.0, f64::NAN, f64::INFINITY, 1e9] {
            assert!(png_1bit(&sample(), dpi).is_err(), "{dpi}");
            assert!(tiff_1bit(&sample(), dpi).is_err(), "{dpi}");
            assert!(tiff_g4(&sample(), dpi).is_err(), "{dpi}");
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
