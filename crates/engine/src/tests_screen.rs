//! Screen printing: `screen.*` commands (SC-1).

use serde_json::{Value, json};

use super::*;

const NAVY: &str = "#0a1458";
const YELLOW: &str = "#fadc14";

fn rect(s: &mut Session, x: f64, w: f64, fill: &str) {
    let r = s.execute("shape.rectangle", &json!({"x": x, "y": 0, "width": w, "height": 100})).unwrap();
    s.execute("select.set", &json!({"ids": [r["id"].as_u64().unwrap()]})).unwrap();
    s.execute("paint.setFill", &json!({"color": fill})).unwrap();
}

/// A 200 × 100 pt document: yellow on the left half, navy on the right.
fn session() -> Session {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 200, "height": 100})).unwrap();
    rect(&mut s, 0.0, 100.0, YELLOW);
    rect(&mut s, 100.0, 100.0, NAVY);
    s
}

fn inks() -> Value {
    json!([{"name": "Navy", "color": NAVY, "mesh": 156}, {"name": "Yellow", "color": YELLOW}])
}

fn film_image(f: &Value) -> image::GrayImage {
    let png = vectorcraft_format::base64_decode(f["dataBase64"].as_str().unwrap()).unwrap();
    image::load_from_memory(&png).unwrap().to_luma8()
}

fn inked(img: &image::GrayImage, x: u32, y: u32) -> bool {
    img.get_pixel(x, y).0[0] == 0
}

#[test]
fn separate_makes_one_film_a_screen_underbase_first_then_light_to_dark() {
    let mut s = session();
    let r = s.execute("screen.separate", &json!({"dpi": 100, "inks": inks(), "underbase": true, "trapMm": 1.0})).unwrap();
    let films = r["films"].as_array().unwrap();
    let names: Vec<_> = films.iter().map(|f| f["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Underbase", "Yellow", "Navy"]);
    assert_eq!(films[0]["role"], "underbase");
    assert_eq!(films[1]["color"], YELLOW);
    assert_eq!(films[2]["mesh"], 156.0);
    assert_eq!(films[0]["mesh"], 110.0, "the underbase's default mesh");
    // 100 dpi: a 200 × 100 pt artboard is 278 × 139 px.
    let (w, h) = (r["width"].as_u64().unwrap() as u32, r["height"].as_u64().unwrap() as u32);
    assert!((277..=279).contains(&w) && (138..=140).contains(&h), "{w}x{h}");
    let (ub, yellow, navy) = (film_image(&films[0]), film_image(&films[1]), film_image(&films[2]));
    let mid = w / 2;
    // Yellow's left half is inked and navy's is not; yellow is trapped (spread) under navy by
    // 1 mm (about 4 px), navy is not spread.
    assert!(inked(&yellow, 20, 70) && !inked(&navy, 20, 70));
    assert!(inked(&navy, w - 20, 70) && !inked(&yellow, w - 20, 70));
    // The colours meet at 100 pt = pixel 138.9 here, so navy starts at 138 and yellow ends at 137.
    assert!(inked(&yellow, mid + 1, 70) && inked(&navy, mid + 1, 70), "yellow tucks under navy");
    assert!(!inked(&yellow, mid + 5, 70), "the trap stops about 4 px in");
    assert!(!inked(&navy, mid - 4, 70), "navy is not spread");
    // The underbase fills the art but stays in from its edges.
    assert!(inked(&ub, mid, 70) && !inked(&ub, 0, 70) && !inked(&ub, mid, 0));
}

#[test]
fn separate_without_underbase_and_with_colors_found_in_the_art() {
    let mut s = session();
    let r = s.execute("screen.separate", &json!({"dpi": 72, "colors": 2})).unwrap();
    let films = r["films"].as_array().unwrap();
    assert_eq!(films.len(), 2, "two colours found, no underbase");
    assert!(films.iter().all(|f| f["role"] == "color"));
    let cov: f64 = films.iter().map(|f| f["coverage"].as_f64().unwrap()).sum();
    assert!((cov - 1.0).abs() < 0.02, "the plates partition the art: {cov}");
}

#[test]
fn films_are_written_to_a_folder_with_registration_marks() {
    let mut s = session();
    let dir = vectorcraft_testkit::temp_dir("screen");
    let sub = dir.join("films");
    std::fs::create_dir_all(&sub).unwrap();
    let plain = s.execute("screen.separate", &json!({"dpi": 100, "inks": inks()})).unwrap();
    let marked = s.execute("screen.separate", &json!({"dpi": 100, "inks": inks(), "marks": true, "dir": sub.to_string_lossy()})).unwrap();
    let films = marked["films"].as_array().unwrap();
    assert!(marked["width"].as_u64() > plain["width"].as_u64(), "marks add a margin");
    let widths: std::collections::HashSet<_> = films.iter().map(|f| (f["width"].as_u64(), f["height"].as_u64())).collect();
    assert_eq!(widths.len(), 1, "every film is the same size so the marks line up");
    for f in films {
        let path = f["path"].as_str().unwrap();
        assert!(path.contains("-color-"), "{path}");
        let img = image::open(path).unwrap().to_luma8();
        assert_eq!(u64::from(img.width()), f["width"].as_u64().unwrap());
        // The corner marks are drawn on every film (top-left mark's centre).
        let m = sc_core::marks::MarkSpec::for_dpi(100.0).unwrap();
        let margin = (m.gap_px + m.size_px + 1) as u32;
        let (cx, cy) = (margin - (m.gap_px + m.size_px / 2) as u32, margin - (m.gap_px + m.size_px / 2) as u32);
        assert!(inked(&img, cx, cy), "mark centre at {cx},{cy} in {path}");
        assert!(f.get("dataBase64").is_none());
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn artwork_can_come_from_image_bytes_without_a_document() {
    let mut s = Session::new();
    let mut png = Vec::new();
    let img = image::RgbaImage::from_fn(40, 20, |x, _| if x < 20 { image::Rgba([250, 220, 20, 255]) } else { image::Rgba([10, 20, 88, 255]) });
    image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let src = json!({"dataBase64": vectorcraft_format::base64_encode(&png)});
    let pal = s.execute("screen.palette", &json!({"source": src, "colors": 3})).unwrap();
    let colors: Vec<_> = pal["colors"].as_array().unwrap().iter().map(|c| c.as_str().unwrap()).collect();
    assert_eq!(colors.len(), 2);
    assert!(colors.contains(&YELLOW) && colors.contains(&NAVY), "{colors:?}");
    assert_eq!((pal["width"].as_u64(), pal["height"].as_u64()), (Some(40), Some(20)));
    let r = s.execute("screen.separate", &json!({"source": src, "colors": 2, "dpi": 300})).unwrap();
    assert_eq!(r["films"].as_array().unwrap().len(), 2);
}

#[test]
fn halftone_screens_a_gray_ramp_and_reports_mesh_advice() {
    let mut s = Session::new();
    let mut png = Vec::new();
    let img = image::RgbaImage::from_pixel(240, 240, image::Rgba([128, 128, 128, 255]));
    image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let src = json!({"dataBase64": vectorcraft_format::base64_encode(&png)});
    let r = s.execute("screen.halftone", &json!({"source": src, "dpi": 600, "lpi": 50, "mesh": 110, "angle": 0, "minDot": 0, "maxDot": 1})).unwrap();
    let f = &r["films"][0];
    assert_eq!(f["name"], "Halftone");
    // Mid grey needs about half the ink (the grey is not exactly 50% in Lab).
    let c = f["coverage"].as_f64().unwrap();
    assert!((0.35..0.65).contains(&c), "{c}");
    let codes: Vec<_> = r["advice"].as_array().unwrap().iter().map(|a| a["code"].as_str().unwrap()).collect();
    assert_eq!(codes, ["lpiTooHigh", "angleAlignsWithThreads"], "50 lpi on 110 mesh at 0°");
    let ok = s.execute("screen.halftone", &json!({"source": src, "dpi": 600, "lpi": 45, "mesh": 230})).unwrap();
    assert!(ok["advice"].as_array().unwrap().is_empty());
    for shape in ["round", "ellipse", "line"] {
        s.execute("screen.halftone", &json!({"source": src, "dpi": 600, "shape": shape})).unwrap();
    }
}

#[test]
fn advice_command() {
    let mut s = Session::new();
    let r = s.execute("screen.advice", &json!({"mesh": 230, "lpi": 45})).unwrap();
    assert_eq!(r["recommendedLpi"], json!({"min": 46.0, "max": 57.5}));
    assert!(r["advice"].as_array().unwrap().is_empty());
    let r = s.execute("screen.advice", &json!({"mesh": 160, "lpi": 45, "angle": 90})).unwrap();
    assert_eq!(r["advice"].as_array().unwrap().len(), 2);
    assert!(s.execute("screen.advice", &json!({"lpi": 45})).is_err(), "mesh is required");
}

#[test]
fn bad_input_is_an_error_and_changes_nothing() {
    let mut s = Session::new();
    // No document and no other source.
    assert!(s.execute("screen.separate", &json!({})).is_err());
    let mut s = session();
    let before = s.doc().unwrap().history.undo.len();
    for bad in [
        json!({"dpi": 0}),
        json!({"dpi": 1e9}),
        json!({"dpi": "high"}),
        json!({"inks": []}),
        json!({"inks": "red"}),
        json!({"inks": [{"color": "red"}]}),
        json!({"inks": inks(), "trapMm": -1}),
        json!({"inks": inks(), "underbase": {"color": "nope"}}),
        json!({"colors": 99}),
        json!({"source": 5}),
        json!({"source": {"path": "/definitely/not/here.png"}}),
        json!({"source": {"dataBase64": "!!"}}),
        json!({"source": {"dataBase64": "AAAA"}}),
        json!({"source": {}}),
    ] {
        assert!(s.execute("screen.separate", &bad).is_err(), "{bad}");
    }
    for bad in [json!({"shape": "star"}), json!({"lpi": 0}), json!({"lpi": 600, "dpi": 300}), json!({"minDot": 2}), json!({"ink": "#ffffff"})] {
        assert!(s.execute("screen.halftone", &bad).is_err(), "{bad}");
    }
    assert_eq!(s.doc().unwrap().history.undo.len(), before);
}

#[test]
fn films_come_as_one_bit_png_or_tiff_with_their_resolution() {
    let mut s = session();
    let dir = vectorcraft_testkit::temp_dir("screen-formats");
    let png = s.execute("screen.separate", &json!({"dpi": 100, "inks": inks()})).unwrap();
    let f = &png["films"][0];
    assert_eq!(f["format"], "png");
    let bytes = vectorcraft_format::base64_decode(f["dataBase64"].as_str().unwrap()).unwrap();
    assert!(bytes.starts_with(b"\x89PNG"));
    assert!(bytes.windows(4).any(|w| w == b"pHYs"), "the PNG records its resolution");
    // 1-bit grayscale: bit depth 1, colour type 0 (the IHDR starts at byte 16).
    assert_eq!((bytes[24], bytes[25]), (1, 0));

    let tiff = s.execute("screen.separate", &json!({"dpi": 100, "inks": inks(), "format": "tiff", "dir": dir.to_string_lossy()})).unwrap();
    for f in tiff["films"].as_array().unwrap() {
        assert_eq!(f["format"], "tiff");
        let path = f["path"].as_str().unwrap();
        assert!(path.ends_with(".tif"), "{path}");
        let bytes = std::fs::read(path).unwrap();
        assert_eq!(&bytes[..4], b"II*\0");
        let mut d = tiff::decoder::Decoder::new(std::io::Cursor::new(bytes)).unwrap();
        assert_eq!(d.colortype().unwrap(), tiff::ColorType::Gray(1));
        assert_eq!(u64::from(d.dimensions().unwrap().0), f["width"].as_u64().unwrap());
    }
    // Group 4 files decode to the same pixels as the uncompressed ones.
    let g4 = s.execute("screen.separate", &json!({"dpi": 100, "inks": inks(), "format": "tiff-g4", "dir": dir.to_string_lossy()})).unwrap();
    assert_eq!(g4["films"].as_array().unwrap().len(), tiff["films"].as_array().unwrap().len());
    for f in g4["films"].as_array().unwrap() {
        assert_eq!(f["format"], "tiff-g4");
        let mut d = tiff::decoder::Decoder::new(std::io::Cursor::new(std::fs::read(f["path"].as_str().unwrap()).unwrap())).unwrap();
        assert_eq!(d.get_tag_u64(tiff::tags::Tag::Compression).unwrap(), 4);
        assert_eq!(u64::from(d.dimensions().unwrap().0), f["width"].as_u64().unwrap());
        d.read_image().unwrap();
    }
    // PDF films are one page each, at the film's physical size, and the app's own importer opens them.
    let pdf = s.execute("screen.separate", &json!({"dpi": 100, "inks": inks(), "format": "pdf", "dir": dir.to_string_lossy()})).unwrap();
    for f in pdf["films"].as_array().unwrap() {
        assert_eq!(f["format"], "pdf");
        let path = f["path"].as_str().unwrap();
        assert!(path.ends_with(".pdf"), "{path}");
        let bytes = std::fs::read(path).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
        let text = String::from_utf8_lossy(&bytes);
        let (w, h) = (f["width"].as_f64().unwrap() * 0.72, f["height"].as_f64().unwrap() * 0.72);
        assert!(text.contains(&format!("/MediaBox [0 0 {w:.4} {h:.4}]")), "{path}");
        let mut opened = session();
        opened.execute("document.open", &json!({"name": "film.pdf", "dataBase64": vectorcraft_format::base64_encode(&bytes)})).unwrap();
    }
    // The halftone takes the option too.
    let mut png = Vec::new();
    let img = image::RgbaImage::from_pixel(60, 60, image::Rgba([128, 128, 128, 255]));
    image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let src = json!({"dataBase64": vectorcraft_format::base64_encode(&png)});
    let h = s.execute("screen.halftone", &json!({"source": src, "dpi": 600, "format": "tiff"})).unwrap();
    assert_eq!(h["films"][0]["format"], "tiff");
    assert!(s.execute("screen.separate", &json!({"inks": inks(), "format": "gif"})).is_err());
    assert!(s.execute("screen.halftone", &json!({"source": src, "format": 3})).is_err());
    let _ = std::fs::remove_dir_all(dir);
}

/// sRGB bytes of a linear-light mix `t` of the way from white to `ink`.
fn mix_with_white(ink: [u8; 3], t: f64) -> [u8; 3] {
    let lin = |c: u8| {
        let c = f64::from(c) / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    let enc = |v: f64| {
        let s = if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
        (s * 255.0).round().clamp(0.0, 255.0) as u8
    };
    [0, 1, 2].map(|k| enc(1.0 + (lin(ink[k]) - 1.0) * t))
}

fn png_source(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Value {
    let img = image::RgbaImage::from_fn(w, h, |x, y| image::Rgba(f(x, y)));
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    json!({"dataBase64": vectorcraft_format::base64_encode(&png)})
}

/// The share of inked pixels of a PNG film in the columns `x0..x1`.
fn inked_share(f: &Value, x0: u32, x1: u32) -> f64 {
    let img = film_image(f);
    let mut n = 0u32;
    let mut inked_px = 0u32;
    for y in 0..img.height() {
        for x in x0..x1 {
            n += 1;
            inked_px += u32::from(inked(&img, x, y));
        }
    }
    f64::from(inked_px) / f64::from(n)
}

#[test]
fn simulate_prints_a_tone_as_dots_of_the_ink_over_the_base() {
    let mut s = Session::new();
    let navy = [10, 20, 90];
    // A white-to-navy ramp, 240 pixels wide: denser dots to the right.
    let src = png_source(240, 60, |x, _| {
        let c = mix_with_white(navy, f64::from(x) / 239.0);
        [c[0], c[1], c[2], 255]
    });
    let inks = json!([{"name": "Navy", "color": "#0a1458"}]);
    let r = s.execute("screen.simulate", &json!({"source": src, "dpi": 600, "lpi": 50, "inks": inks, "minDot": 0, "maxDot": 1})).unwrap();
    let films = r["films"].as_array().unwrap();
    assert_eq!(films.len(), 1);
    let f = &films[0];
    let (left, mid, right) = (inked_share(f, 0, 60), inked_share(f, 90, 150), inked_share(f, 180, 240));
    assert!(left < mid && mid < right, "{left} < {mid} < {right}");
    assert!((left - 0.125).abs() < 0.06 && (mid - 0.5).abs() < 0.06 && (right - 0.875).abs() < 0.06, "{left} {mid} {right}");
}

#[test]
fn simulate_mixes_inks_has_an_underbase_and_checks_mesh_and_angle() {
    let mut s = Session::new();
    // Half navy and half red by area (in linear light) over white: each ink covers a bit under half.
    let (a, b) = ([10u8, 20, 90], [200u8, 30, 30]);
    let lin = |c: u8| {
        let c = f64::from(c) / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    let enc = |v: f64| {
        let s = if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
        (s * 255.0).round().clamp(0.0, 255.0) as u8
    };
    let px: Vec<u8> = (0..3).map(|k| enc(0.5 * lin(a[k]) + 0.5 * lin(b[k]))).collect();
    let src = png_source(120, 120, |_, _| [px[0], px[1], px[2], 255]);
    let inks = json!([{"name": "Navy", "color": "#0a1458", "mesh": 110}, {"name": "Red", "color": "#c81e1e", "mesh": 230}]);
    let r = s
        .execute(
            "screen.simulate",
            &json!({"source": src, "dpi": 600, "lpi": 50, "inks": inks, "underbase": true, "angles": [0, 67.5], "minDot": 0, "maxDot": 1, "marks": true}),
        )
        .unwrap();
    let names: Vec<_> = r["films"].as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Underbase", "Red", "Navy"]);
    let widths: std::collections::HashSet<_> = r["films"].as_array().unwrap().iter().map(|f| f["width"].as_u64()).collect();
    assert_eq!(widths.len(), 1, "marks give every film the same size");
    for f in &r["films"].as_array().unwrap()[1..] {
        let c = f["coverage"].as_f64().unwrap();
        assert!((c - 0.5).abs() < 0.05, "{}: {c}", f["name"]);
    }
    // Navy on 110 mesh at 50 lpi is too fine, and its 0° angle lines up with the threads.
    let advice: Vec<_> = r["advice"].as_array().unwrap().iter().map(|a| (a["ink"].as_str().unwrap(), a["code"].as_str().unwrap())).collect();
    assert_eq!(advice, [("Navy", "lpiTooHigh"), ("Navy", "angleAlignsWithThreads")]);
}

#[test]
fn simulate_rejects_bad_parameters() {
    let mut s = Session::new();
    let src = png_source(8, 8, |_, _| [200, 30, 30, 255]);
    let inks = json!([{"color": "#c81e1e"}]);
    for bad in [
        json!({"source": src, "inks": inks, "angles": []}),
        json!({"source": src, "inks": inks, "angles": "22.5"}),
        json!({"source": src, "inks": inks, "angles": [1e9]}),
        json!({"source": src, "inks": inks, "base": "white"}),
        json!({"source": src, "inks": inks, "shape": "star"}),
        json!({"source": src, "inks": inks, "lpi": 0}),
        json!({"source": src, "inks": inks, "lpi": 600, "dpi": 300}),
        json!({"source": src, "colors": 9}),
        json!({"source": src, "inks": vec![json!({"color": "#c81e1e"}); 9]}),
    ] {
        assert!(s.execute("screen.simulate", &bad).is_err(), "{bad}");
    }
}

#[test]
fn a_job_sheet_lists_the_screens_and_the_advice() {
    let mut s = session();
    let dir = vectorcraft_testkit::temp_dir("screen-sheet");
    let none = s.execute("screen.separate", &json!({"dpi": 100, "inks": inks()})).unwrap();
    assert!(none.get("jobSheet").is_none(), "the sheet is asked for");

    let r = s
        .execute(
            "screen.separate",
            &json!({"dpi": 100, "inks": inks(), "format": "tiff-g4", "dir": dir.to_string_lossy(), "jobSheet": {"title": "Tour (front)", "notes": "Cure 160 C"}}),
        )
        .unwrap();
    let path = r["jobSheet"]["path"].as_str().unwrap();
    assert!(path.ends_with("/job-sheet.pdf"), "{path}");
    let text = String::from_utf8_lossy(&std::fs::read(path).unwrap()).into_owned();
    assert!(text.contains("(Tour \\(front\\))") && text.contains("(Cure 160 C)"), "{text}");
    for (i, f) in r["films"].as_array().unwrap().iter().enumerate() {
        let file = f["path"].as_str().unwrap().rsplit('/').next().unwrap();
        assert!(text.contains(&format!("({file})")), "{file}");
        assert!(text.contains(&format!("({}. {})", i + 1, f["name"].as_str().unwrap())));
    }
    let mut opened = session();
    opened.execute("document.open", &json!({"name": "sheet.pdf", "dataBase64": vectorcraft_format::base64_encode(text.as_bytes())})).ok();

    // Without a folder the sheet comes back as base64; the halftone's advice reaches it.
    let mut png = Vec::new();
    let img = image::RgbaImage::from_pixel(60, 60, image::Rgba([128, 128, 128, 255]));
    image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let src = json!({"dataBase64": vectorcraft_format::base64_encode(&png)});
    let h = s.execute("screen.halftone", &json!({"source": src, "dpi": 600, "mesh": 60, "lpi": 55, "jobSheet": true})).unwrap();
    assert!(!h["advice"].as_array().unwrap().is_empty());
    let bytes = vectorcraft_format::base64_decode(h["jobSheet"]["dataBase64"].as_str().unwrap()).unwrap();
    let sheet = String::from_utf8_lossy(&bytes).into_owned();
    assert!(sheet.starts_with("%PDF-1.4") && sheet.contains("(Advice)") && sheet.contains("(ScreenFoundry job)"), "{sheet}");

    assert!(s.execute("screen.halftone", &json!({"source": src, "jobSheet": 3})).is_err());
    let _ = std::fs::remove_dir_all(dir);
}
