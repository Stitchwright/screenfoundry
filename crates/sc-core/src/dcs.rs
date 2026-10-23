//! DCS 2.0 (Desktop Color Separation) files: one "main" EPS holding a composite of the job and a
//! pointer to a separation file for every plate, each plate an EPS of its own.
//!
//! Written from the public specification, *Desktop Color Separation Specification 2.0* (June 1993,
//! revised May 1995): the main file is an EPS with a `%%PlateFile` comment for every plate before
//! `%%EndComments`, then a composite. Two variants:
//!
//! * **single file**: `%%PlateFile: (name) EPS #offset size`, every plate's EPS following the
//!   main part in the same file, `offset` and `size` in decimal bytes;
//! * **multiple files**: `%%PlateFile: (name) EPS Local filename`, the plates in files beside it.
//!
//! Every plate here is a spot colour (a film is one ink), drawn as a 1-bit grayscale image whose
//! black is where that ink prints. The composite is the plates painted in order in their own
//! colours on white, at a resolution the caller chooses ([`DEFAULT_COMPOSITE_DPI`] unless asked;
//! the film's own for a full-resolution composite, which makes a large main file). A
//! plate file begins `%!PS-Adobe` and ends `%%Trailer` and carries no `%%PlateFile` comment, as the
//! specification asks.
//!
//! No PostScript interpreter or DCS consumer was available to check the files against: the tests
//! read them back and check the structure the specification describes. Test with your own RIP.

use crate::error::{Result, param};
use crate::image::Bitmap1;

/// Default resolution of the composite in the main file. The specification prefers a full
/// resolution composite for proofing; this keeps the file a fraction of the size of the plates.
pub const DEFAULT_COMPOSITE_DPI: f64 = 150.0;

/// Largest composite, in bytes of RGB (50 megapixels), so that a full-resolution composite of a
/// huge film cannot exhaust memory.
const MAX_COMPOSITE_BYTES: usize = 150_000_000;

/// Most plates one file points at.
pub const MAX_PLATES: usize = 32;

/// Width a `%%PlateFile` line is padded to, so that the offsets it holds can change length
/// without moving anything after it.
const PLATE_LINE_WIDTH: usize = 120;

/// Longest comment line (the specification asks for 255 at most; longer ones continue with `%%+`).
const MAX_LINE: usize = 255;

/// The first line of every EPS file, DCS main and plate files included: the signature the
/// PostScript document structure conventions require.
const EPS_SIGNATURE: &str = "%!PS-Adobe-2.0 EPSF-1.2"; // brand-ok: the file signature that format defines

/// One plate: the ink and its film, black (`true`) where the ink prints.
#[derive(Clone, Copy, Debug)]
pub struct Plate<'a> {
    pub name: &'a str,
    /// The ink's colour on screen, for the composite.
    pub rgb: [u8; 3],
    /// The ink's CMYK (0 to 1) for `%%CMYKCustomColor`, if the caller can convert it with colour
    /// management; otherwise a plain conversion of `rgb`.
    pub cmyk: Option<[f64; 4]>,
    pub film: &'a Bitmap1,
}

/// A name as a DSC colour name: parentheses and backslashes would end or escape the string.
fn dsc_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c.is_ascii_graphic() || c == ' ' { c } else { '-' })
        .map(|c| if matches!(c, '(' | ')' | '\\') { '-' } else { c })
        .collect();
    let s = s.trim().to_string();
    if s.is_empty() { "Ink".into() } else { s.chars().take(60).collect() }
}

/// Plain CMYK of an sRGB colour (full black generation), for `%%CMYKCustomColor` when the caller
/// gave none.
fn cmyk(rgb: [u8; 3]) -> [f64; 4] {
    let [r, g, b] = rgb.map(|c| f64::from(c) / 255.0);
    let k = 1.0 - r.max(g).max(b);
    if k >= 1.0 {
        return [0.0, 0.0, 0.0, 1.0];
    }
    [(1.0 - r - k) / (1.0 - k), (1.0 - g - k) / (1.0 - k), (1.0 - b - k) / (1.0 - k), k]
}

fn check(plates: &[Plate], dpi: f64) -> Result<(usize, usize)> {
    if plates.is_empty() || plates.len() > MAX_PLATES {
        return Err(param("plates", format!("DCS needs 1 to {MAX_PLATES} plates")));
    }
    if !dpi.is_finite() || dpi <= 0.0 || dpi > 20_000.0 {
        return Err(param("dpi", "must be above 0 and at most 20000"));
    }
    let first = plates.first().ok_or_else(|| param("plates", "no plates"))?;
    let size = (first.film.width(), first.film.height());
    if plates.iter().any(|p| (p.film.width(), p.film.height()) != size) {
        return Err(param("plates", "every plate must be the same size"));
    }
    Ok(size)
}

/// A comment `key` with `items` after it, continued on `%%+` lines so that none passes 255 bytes.
fn list_comment(key: &str, items: &[String]) -> String {
    let mut out = String::new();
    let mut line = key.to_string();
    for item in items {
        if line.len() + 1 + item.len() > MAX_LINE && line.len() > key.len() {
            out.push_str(&line);
            out.push('\n');
            line = "%%+".to_string();
        }
        line.push(' ');
        line.push_str(item);
    }
    out.push_str(&line);
    out.push('\n');
    out
}

/// A plate's own EPS: the film as a 1-bit grayscale image over the bounding box, ink black.
fn plate_eps(p: &Plate, bbox: (u32, u32), pt: (f64, f64), title: &str) -> Vec<u8> {
    let (w, h) = (p.film.width(), p.film.height());
    let stride = w.div_ceil(8);
    let mut data = vec![0xffu8; stride * h];
    for y in 0..h {
        for x in 0..w {
            if p.film.get(x, y)
                && let Some(b) = data.get_mut(y * stride + x / 8)
            {
                *b &= !(0x80 >> (x % 8));
            }
        }
    }
    let mut out = format!(
        "{EPS_SIGNATURE}\n%%Creator: ScreenFoundry\n%%Title: {title} ({})\n%%BoundingBox: 0 0 {} {}\n%%DocumentData: Binary\n%%LanguageLevel: 1\n%%EndComments\n\
         gsave\n0 0 translate {:.4} {:.4} scale\n/pic {stride} string def\n{w} {h} 1 [{w} 0 0 -{h} 0 {h}] {{currentfile pic readstring pop}} image\n",
        dsc_name(p.name),
        bbox.0,
        bbox.1,
        pt.0,
        pt.1
    )
    .into_bytes();
    out.extend_from_slice(&data);
    out.extend_from_slice(b"\ngrestore\nshowpage\n%%Trailer\n");
    out
}

/// The composite: the plates painted in order in their colours on white, every `step` pixels.
fn composite(plates: &[Plate], (w, h): (usize, usize), step: usize) -> (usize, usize, Vec<u8>) {
    let (cw, ch) = (w.div_ceil(step), h.div_ceil(step));
    let mut rgb = vec![255u8; cw * ch * 3];
    for p in plates {
        for cy in 0..ch {
            for cx in 0..cw {
                if p.film.get(cx * step, cy * step)
                    && let Some(px) = rgb.get_mut((cy * cw + cx) * 3..(cy * cw + cx) * 3 + 3)
                {
                    px.copy_from_slice(&p.rgb);
                }
            }
        }
    }
    (cw, ch, rgb)
}

/// The main file up to the plate pointers: everything before `%%PlateFile` lines are filled in is
/// returned in two parts, the comments before them and the body (`%%EndComments` onward).
struct Main {
    head: String,
    body: Vec<u8>,
}

/// The composite's subsampling step: every `step`-th film pixel, so that it comes out at about
/// `composite_dpi`. A composite dpi at or above the film's is every pixel.
fn composite_step(dpi: f64, composite_dpi: f64, size: (usize, usize)) -> Result<usize> {
    if !composite_dpi.is_finite() || composite_dpi <= 0.0 {
        return Err(param("composite_dpi", "must be above 0"));
    }
    let step = ((dpi / composite_dpi).ceil() as usize).max(1);
    let bytes = size.0.div_ceil(step).saturating_mul(size.1.div_ceil(step)).saturating_mul(3);
    if bytes > MAX_COMPOSITE_BYTES {
        return Err(param("composite_dpi", "the composite would be too large: lower the composite resolution"));
    }
    Ok(step)
}

fn main_parts(plates: &[Plate], dpi: f64, title: &str, size: (usize, usize), step: usize) -> (Main, (u32, u32), (f64, f64)) {
    let pt = (size.0 as f64 * 72.0 / dpi, size.1 as f64 * 72.0 / dpi);
    let bbox = (pt.0.ceil() as u32, pt.1.ceil() as u32);
    let names: Vec<String> = plates.iter().map(|p| format!("({})", dsc_name(p.name))).collect();
    let mut head = format!("{EPS_SIGNATURE}\n%%Creator: ScreenFoundry\n");
    head.push_str(&format!("%%Title: {}\n", dsc_name(title)));
    head.push_str(&list_comment("%%DocumentCustomColors:", &names));
    for (p, n) in plates.iter().zip(&names) {
        let [c, m, y, k] = p.cmyk.map_or_else(|| cmyk(p.rgb), |v| v.map(|c| if c.is_finite() { c.clamp(0.0, 1.0) } else { 0.0 }));
        head.push_str(&format!("%%CMYKCustomColor: {c:.3} {m:.3} {y:.3} {k:.3} {n}\n"));
    }
    head.push_str(&format!("%%DocumentData: Binary\n%%LanguageLevel: 2\n%%BoundingBox: 0 0 {} {}\n", bbox.0, bbox.1));

    let (cw, ch, rgb) = composite(plates, size, step);
    let mut body = format!(
        "%%EndComments\ngsave\n0 0 translate {:.4} {:.4} scale\n/pic {} string def\n{cw} {ch} 8 [{cw} 0 0 -{ch} 0 {ch}] {{currentfile pic readstring pop}} false 3 colorimage\n",
        pt.0,
        pt.1,
        cw * 3
    )
    .into_bytes();
    body.extend_from_slice(&rgb);
    body.extend_from_slice(b"\ngrestore\nshowpage\n%%Trailer\n");
    (Main { head, body }, bbox, pt)
}

/// A `%%PlateFile` line padded to a fixed width, so its numbers can change without moving the rest.
fn padded(line: String) -> String {
    format!("{line:<width$}\n", width = PLATE_LINE_WIDTH)
}

/// A single-file DCS 2.0: the main part, then every plate's EPS, in one file.
pub fn single_file(plates: &[Plate], dpi: f64, title: &str, composite_dpi: f64) -> Result<Vec<u8>> {
    let size = check(plates, dpi)?;
    let step = composite_step(dpi, composite_dpi, size)?;
    let (main, bbox, pt) = main_parts(plates, dpi, title, size, step);
    let eps: Vec<Vec<u8>> = plates.iter().map(|p| plate_eps(p, bbox, pt, title)).collect();
    // Every plate line has the same length whatever its numbers are, so the main part's length
    // is known before the numbers are.
    let line = |name: &str, offset: usize, len: usize| padded(format!("%%PlateFile: ({}) EPS #{offset} {len}", dsc_name(name)));
    let main_len = main.head.len() + plates.len() * PLATE_LINE_WIDTH + plates.len() + main.body.len();
    let mut out = main.head.into_bytes();
    let mut offset = main_len;
    for (p, e) in plates.iter().zip(&eps) {
        out.extend_from_slice(line(p.name, offset, e.len()).as_bytes());
        offset += e.len();
    }
    out.extend_from_slice(&main.body);
    if out.len() != main_len {
        return Err(param("plates", "internal: the DCS header changed size"));
    }
    for e in &eps {
        out.extend_from_slice(e);
    }
    Ok(out)
}

/// A multiple-file DCS 2.0: the main file, and the EPS of every plate to write under `file_names`
/// (in plate order; each a plain file name, no folder).
pub fn multiple_files(plates: &[Plate], dpi: f64, title: &str, file_names: &[String], composite_dpi: f64) -> Result<(Vec<u8>, Vec<Vec<u8>>)> {
    let size = check(plates, dpi)?;
    let step = composite_step(dpi, composite_dpi, size)?;
    if file_names.len() != plates.len() || file_names.iter().any(|n| n.is_empty() || n.contains(['/', '\\', ' ', '\n'])) {
        return Err(param("file_names", "one plain file name without spaces for each plate"));
    }
    let (main, bbox, pt) = main_parts(plates, dpi, title, size, step);
    let mut out = main.head;
    for (p, f) in plates.iter().zip(file_names) {
        out.push_str(&padded(format!("%%PlateFile: ({}) EPS Local {f}", dsc_name(p.name))));
    }
    let mut out = out.into_bytes();
    out.extend_from_slice(&main.body);
    let eps = plates.iter().map(|p| plate_eps(p, bbox, pt, title)).collect();
    Ok((out, eps))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn film(w: usize, h: usize, f: impl Fn(usize, usize) -> bool) -> Bitmap1 {
        Bitmap1::from_bits(w, h, (0..w * h).map(|i| f(i % w, i / w)).collect()).unwrap()
    }

    /// The `%%PlateFile` entries of a main file: (name, location, size).
    fn plate_lines(main: &[u8]) -> Vec<(String, String, Option<usize>)> {
        let end = main.windows(14).position(|w| w == b"%%EndComments\n").expect("%%EndComments");
        String::from_utf8_lossy(&main[..end])
            .lines()
            .filter_map(|l| l.strip_prefix("%%PlateFile: "))
            .map(|l| {
                let (name, rest) = l.split_once(") ").unwrap();
                let mut f = rest.split_whitespace();
                assert_eq!(f.next(), Some("EPS"));
                (name.trim_start_matches('(').to_string(), f.next().unwrap().to_string(), f.next().and_then(|s| s.parse().ok()))
            })
            .collect()
    }

    /// The bytes of the 1-bit image in a plate EPS: after the `image` line, before `grestore`.
    fn plate_pixels(eps: &[u8]) -> &[u8] {
        let start = eps.windows(6).position(|w| w == b"image\n").unwrap() + 6;
        let end = eps.windows(10).rposition(|w| w == b"\ngrestore\n").unwrap();
        &eps[start..end]
    }

    fn two_plates() -> (Bitmap1, Bitmap1) {
        (film(19, 6, |x, y| x < 9 && y < 4), film(19, 6, |x, _| x >= 12))
    }

    #[test]
    fn single_file_points_at_complete_plate_files_inside_itself() {
        let (a, b) = two_plates();
        let plates = [
            Plate { name: "Yellow (PMS 109)", rgb: [250, 220, 20], cmyk: None, film: &a },
            Plate { name: "Navy", rgb: [10, 20, 90], cmyk: None, film: &b },
        ];
        let dcs = single_file(&plates, 300.0, "Job", DEFAULT_COMPOSITE_DPI).unwrap();
        assert!(dcs.starts_with(b"%!PS-Adobe-2.0 EPSF-1.2\n"));
        let text = String::from_utf8_lossy(&dcs[..600]).into_owned();
        assert!(text.contains("%%DocumentCustomColors: (Yellow -PMS 109-) (Navy)"), "{text}");
        assert!(text.contains("%%CMYKCustomColor: 0.000 0.120 0.920 0.020 (Yellow -PMS 109-)"), "{text}");
        assert!(text.contains("%%BoundingBox: 0 0 5 2"), "19 × 6 px at 300 dpi is 4.56 × 1.44 pt: {text}");
        let lines = plate_lines(&dcs);
        assert_eq!(lines.len(), 2);
        let mut next = None;
        for ((name, loc, size), p) in lines.iter().zip(&plates) {
            assert_eq!(name, &dsc_name(p.name));
            let (off, size) = (loc.strip_prefix('#').expect("single-file offsets start with #").parse::<usize>().unwrap(), size.unwrap());
            let part = &dcs[off..off + size];
            assert!(part.starts_with(b"%!PS-Adobe-2.0 EPSF-1.2\n") && part.ends_with(b"%%Trailer\n"), "{name}");
            assert!(!String::from_utf8_lossy(part).contains("%%PlateFile"), "plate files carry no PlateFile comment");
            // The plate is its film: ink is a 0 bit, rows are byte-aligned (19 px = 3 bytes).
            let px = plate_pixels(part);
            assert_eq!(px.len(), 3 * 6);
            for y in 0..6 {
                for x in 0..19 {
                    let bit = px[y * 3 + x / 8] & (0x80 >> (x % 8)) != 0;
                    assert_eq!(!bit, p.film.get(x, y), "{name} ({x}, {y})");
                }
            }
            if let Some(n) = next {
                assert_eq!(off, n, "plates follow one another");
            }
            next = Some(off + size);
        }
        assert_eq!(next, Some(dcs.len()), "nothing after the last plate");
        // The main part ends where the first plate begins.
        let first: usize = lines[0].1[1..].parse().unwrap();
        assert!(dcs[..first].ends_with(b"%%Trailer\n"));
    }

    #[test]
    fn the_composite_paints_each_plate_in_its_colour_on_white() {
        let (a, b) = two_plates();
        let plates = [Plate { name: "A", rgb: [250, 220, 20], cmyk: None, film: &a }, Plate { name: "B", rgb: [10, 20, 90], cmyk: None, film: &b }];
        let (cw, ch, rgb) = composite(&plates, (19, 6), 1);
        assert_eq!((cw, ch), (19, 6));
        let at = |x: usize, y: usize| [rgb[(y * cw + x) * 3], rgb[(y * cw + x) * 3 + 1], rgb[(y * cw + x) * 3 + 2]];
        assert_eq!(at(2, 1), [250, 220, 20]);
        assert_eq!(at(15, 5), [10, 20, 90]);
        assert_eq!(at(10, 5), [255, 255, 255]);
        // Subsampled: every second pixel, rounding the size up.
        assert_eq!(composite(&plates, (19, 6), 2).0, 10);
    }

    #[test]
    fn multiple_files_name_their_plates_and_have_no_offsets() {
        let (a, b) = two_plates();
        let plates = [Plate { name: "A", rgb: [1, 2, 3], cmyk: None, film: &a }, Plate { name: "B", rgb: [4, 5, 6], cmyk: None, film: &b }];
        let (main, files) = multiple_files(&plates, 300.0, "Job", &["job-a.eps".into(), "job-b.eps".into()], DEFAULT_COMPOSITE_DPI).unwrap();
        let lines = plate_lines(&main);
        assert_eq!(lines.iter().map(|l| (l.0.as_str(), l.1.as_str(), l.2)).collect::<Vec<_>>(), [("A", "Local", None), ("B", "Local", None)]);
        // The comments only: the composite after them is binary and may hold any byte.
        let text = String::from_utf8_lossy(&main[..main.windows(14).position(|w| w == b"%%EndComments\n").unwrap()]).into_owned();
        assert!(text.contains("%%PlateFile: (A) EPS Local job-a.eps") && text.contains("job-b.eps"));
        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|f| f.starts_with(b"%!PS-Adobe") && f.ends_with(b"%%Trailer\n")));
        assert!(main.ends_with(b"%%Trailer\n"));
        assert!(!text.contains('#'), "multiple-file plates have no #offset");
        assert!(multiple_files(&plates, 300.0, "Job", &["only-one.eps".into()], DEFAULT_COMPOSITE_DPI).is_err());
        assert!(multiple_files(&plates, 300.0, "Job", &["a/b.eps".into(), "c.eps".into()], DEFAULT_COMPOSITE_DPI).is_err());
    }

    #[test]
    fn long_colour_lists_continue_on_plus_lines_and_no_header_line_passes_255() {
        let film = film(8, 8, |_, _| false);
        let names: Vec<String> = (0..12).map(|i| format!("A rather long ink name number {i} for a test")).collect();
        let plates: Vec<Plate> = names.iter().map(|n| Plate { name: n, rgb: [n.len() as u8; 3], cmyk: None, film: &film }).collect();
        let dcs = single_file(&plates, 600.0, "Job", DEFAULT_COMPOSITE_DPI).unwrap();
        let head = String::from_utf8_lossy(&dcs[..dcs.windows(14).position(|w| w == b"%%EndComments\n").unwrap()]).into_owned();
        assert!(head.contains("\n%%+ ("), "{head}");
        assert!(head.lines().all(|l| l.len() <= 255), "lines are at most 255 bytes");
        assert_eq!(plate_lines(&dcs).len(), 12);
    }

    fn p(f: &Bitmap1) -> Plate<'_> {
        Plate { name: "A", rgb: [0; 3], cmyk: None, film: f }
    }

    #[test]
    fn plates_must_agree_and_be_countable() {
        let (a, _) = two_plates();
        let other = film(8, 8, |_, _| true);
        assert!(single_file(&[], 300.0, "J", 150.0).is_err());
        assert!(single_file(&[p(&a), p(&other)], 300.0, "J", 150.0).is_err());
        assert!(single_file(&[p(&a)], f64::NAN, "J", 150.0).is_err());
        assert!(single_file(&[p(&a)], 0.0, "J", 150.0).is_err());
        let many: Vec<Plate> = (0..=MAX_PLATES).map(|_| p(&a)).collect();
        assert!(single_file(&many, 300.0, "J", 150.0).is_err());
        assert_eq!(dsc_name("a(b)\\c\n"), "a-b--c-");
        assert_eq!(dsc_name("   "), "Ink");
    }

    #[test]
    fn the_composite_resolution_is_chosen_and_full_resolution_is_bigger() {
        let (a, b) = two_plates();
        let plates = [Plate { name: "A", rgb: [1, 2, 3], cmyk: None, film: &a }, Plate { name: "B", rgb: [4, 5, 6], cmyk: None, film: &b }];
        // 19 × 6 px at 300 dpi: 150 dpi takes every second pixel (10 × 3), the film's own all of them.
        let half = single_file(&plates, 300.0, "Job", 150.0).unwrap();
        let full = single_file(&plates, 300.0, "Job", 300.0).unwrap();
        let above = single_file(&plates, 300.0, "Job", 2400.0).unwrap();
        assert_eq!(full, above, "a composite resolution above the film's is the film's");
        let comp = |f: &[u8]| {
            let end = (f.windows(14).position(|w| w == b"%%EndComments\n").unwrap() + 1500).min(f.len());
            let t = String::from_utf8_lossy(&f[..end]).into_owned();
            t.lines().find(|l| l.ends_with("false 3 colorimage")).map(str::to_string).unwrap()
        };
        assert!(comp(&half).starts_with("10 3 8 [10 0 0 -3 0 3]"), "{}", comp(&half));
        assert!(comp(&full).starts_with("19 6 8 [19 0 0 -6 0 6]"), "{}", comp(&full));
        assert_eq!(full.len() - half.len(), (19 * 6 - 10 * 3) * 3, "only the composite differs in size");
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY * 0.0] {
            assert!(single_file(&plates, 300.0, "Job", bad).is_err(), "{bad}");
        }
        // A film too big for a full-resolution composite is refused, not allocated.
        let huge = Bitmap1::new(8_000, 8_000).unwrap(); // 192 MB of composite at full resolution
        assert!(single_file(&[Plate { name: "H", rgb: [0; 3], cmyk: None, film: &huge }], 600.0, "J", 600.0).is_err());
        assert!(single_file(&[Plate { name: "H", rgb: [0; 3], cmyk: None, film: &huge }], 600.0, "J", 150.0).is_ok());
    }

    #[test]
    fn a_given_cmyk_is_written_instead_of_the_plain_conversion() {
        let (a, _) = two_plates();
        let plate = |cmyk| Plate { name: "A", rgb: [250, 220, 20], cmyk, film: &a };
        let text = |p: Plate| {
            let f = single_file(&[p], 300.0, "J", 150.0).unwrap();
            String::from_utf8_lossy(&f[..f.windows(14).position(|w| w == b"%%EndComments\n").unwrap()]).into_owned()
        };
        assert!(text(plate(None)).contains("%%CMYKCustomColor: 0.000 0.120 0.920 0.020 (A)"));
        assert!(text(plate(Some([0.02, 0.1, 0.95, 0.0]))).contains("%%CMYKCustomColor: 0.020 0.100 0.950 0.000 (A)"));
        // Out of range and non-finite values are held to 0..1.
        assert!(text(plate(Some([1.7, -0.3, f64::NAN, 0.5]))).contains("%%CMYKCustomColor: 1.000 0.000 0.000 0.500 (A)"));
    }
}
