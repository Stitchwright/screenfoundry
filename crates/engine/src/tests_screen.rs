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

#[test]
fn the_ink_library_lists_inks_and_matches_colours_to_them() {
    let mut s = session();
    let lib = s.execute("screen.inkLibrary", &json!({})).unwrap();
    let inks = lib["inks"].as_array().unwrap();
    assert!(inks.len() >= 30 && inks.iter().all(|i| i["color"].as_str().is_some_and(|c| c.len() == 7) && i["family"].is_string()));

    let r = s.execute("screen.matchInks", &json!({"colors": ["#d21f26", "#cd2328", "#00ff00"], "maxDeltaE": 15})).unwrap();
    let m = r["matches"].as_array().unwrap();
    assert_eq!((m[0]["name"].as_str(), m[0]["deltaE"].as_f64()), (Some("Red"), Some(0.0)));
    assert_eq!(m[1]["name"], "Red");
    assert!(m[1]["deltaE"].as_f64().unwrap() > 0.0);
    assert!(m[2].is_null(), "nothing is within 15 of pure green");

    // The shop's own ink is tried first.
    let own = s.execute("screen.matchInks", &json!({"colors": ["#d21f26"], "library": [{"name": "Shop Red", "color": "#d21f26"}]})).unwrap();
    assert_eq!(own["matches"][0]["name"], "Shop Red");

    for bad in [
        json!({}),
        json!({"colors": "#fff"}),
        json!({"colors": ["nope"]}),
        json!({"colors": ["#fff"], "maxDeltaE": -1}),
        json!({"colors": ["#fff"], "library": [{"color": 1}]}),
    ] {
        assert!(s.execute("screen.matchInks", &bad).is_err(), "{bad}");
    }
}

#[test]
fn separating_with_snap_uses_library_inks_and_keeps_given_names() {
    let mut s = session();
    let inks = json!([{"color": "#cd2328", "name": "Front Red"}, {"color": "#1d4fa6"}, {"color": "#d0222a"}]);
    let r = s.execute("screen.separate", &json!({"dpi": 100, "inks": inks, "snap": true})).unwrap();
    let films = r["films"].as_array().unwrap();
    let find = |color: &str| films.iter().find(|f| f["color"] == color).map(|f| f["name"].as_str().unwrap().to_string());
    // The given name stays; the blue takes the library's name; the second near-red is not folded
    // into the same ink, so it keeps its own colour and no screen comes out the same as another.
    assert_eq!(films.len(), 3);
    assert_eq!(find("#d21f26").as_deref(), Some("Front Red"));
    assert_eq!(find("#1f4fa8").as_deref(), Some("Royal Blue"));
    assert_eq!(find("#d0222a").as_deref(), Some("Ink 3"));
    // Without it the colours are as given; with a tight limit nothing snaps.
    let plain = s.execute("screen.separate", &json!({"dpi": 100, "inks": inks})).unwrap();
    let colors = |r: &Value| r["films"].as_array().unwrap().iter().map(|f| f["color"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    assert!(colors(&plain).contains(&"#cd2328".to_string()));
    let tight = s.execute("screen.separate", &json!({"dpi": 100, "inks": inks, "snap": {"maxDeltaE": 0}})).unwrap();
    assert!(colors(&tight).contains(&"#1d4fa6".to_string()) && !colors(&tight).contains(&"#1f4fa8".to_string()));
    let sim =
        s.execute("screen.simulate", &json!({"dpi": 300, "inks": inks, "snap": {"library": [{"name": "Shop Blue", "color": "#1d4fa6"}]}})).unwrap();
    assert!(sim["films"].as_array().unwrap().iter().any(|f| f["name"] == "Shop Blue"));
    assert!(s.execute("screen.separate", &json!({"inks": inks, "snap": 3})).is_err());
}

#[test]
fn dot_gain_lightens_the_film_so_the_print_comes_out_right() {
    let mut s = session();
    let mut png = Vec::new();
    let img = image::RgbaImage::from_pixel(120, 120, image::Rgba([77, 77, 77, 255])); // 70% black
    image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let src = json!({"dataBase64": vectorcraft_format::base64_encode(&png)});
    let mut cov = |p: Value| s.execute("screen.halftone", &p).unwrap()["films"][0]["coverage"].as_f64().unwrap();
    let plain = cov(json!({"source": src, "dpi": 600, "minDot": 0, "maxDot": 1}));
    let gain = cov(json!({"source": src, "dpi": 600, "minDot": 0, "maxDot": 1, "dotGain": 0.2}));
    assert!((plain - 0.70).abs() < 0.05, "{plain}");
    assert!((gain - 0.50).abs() < 0.05, "a 70% tone with 20% gain is a 50% film dot: {gain}");
    for bad in [json!(-0.1), json!(0.3), json!("x")] {
        assert!(s.execute("screen.halftone", &json!({"source": src, "dotGain": bad})).is_err(), "{bad}");
    }
}

#[test]
fn index_prints_each_pixel_in_one_ink_and_the_films_never_overlap() {
    let mut s = Session::new();
    // A white-to-navy ramp over black and red inks: tone comes from how many pixels are inked.
    let navy = [10, 20, 90];
    let src = png_source(240, 60, |x, _| {
        let c = mix_with_white(navy, f64::from(x) / 239.0);
        [c[0], c[1], c[2], 255]
    });
    let inks = json!([{"name": "Black", "color": "#000000"}, {"name": "Red", "color": "#c81e1e"}]);
    let r = s.execute("screen.index", &json!({"source": src, "dpi": 600, "cellMm": 0.2, "inks": inks})).unwrap();
    let films = r["films"].as_array().unwrap();
    let names: Vec<_> = films.iter().map(|f| f["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Red", "Black"], "light to dark");
    let cell = r["cellPx"].as_u64().unwrap() as u32;
    assert_eq!(cell, 5, "0.2 mm at 600 dpi");
    let (red, black) = (film_image(&films[0]), film_image(&films[1]));
    for y in 0..red.height() {
        for x in 0..red.width() {
            assert!(!(inked(&red, x, y) && inked(&black, x, y)), "x={x} y={y}");
            // Pixels come in whole cells.
            assert_eq!(inked(&black, x, y), inked(&black, x - x % cell, y - y % cell), "x={x} y={y}");
        }
    }
    let (left, right) = (inked_share(&films[1], 0, 60), inked_share(&films[1], 180, 240));
    assert!(left < 0.2 && right > 0.5, "denser to the right: {left} {right}");
}

#[test]
fn index_without_dither_is_flat_and_can_find_its_own_inks() {
    let mut s = Session::new();
    // Left half red, right half navy: two flat colours, found from the art, no underbase unless asked.
    let src = png_source(100, 40, |x, _| if x < 50 { [200, 30, 30, 255] } else { [10, 20, 90, 255] });
    let r = s.execute("screen.index", &json!({"source": src, "dpi": 300, "colors": 2, "dither": 0, "underbase": true})).unwrap();
    let films = r["films"].as_array().unwrap();
    assert_eq!(films.len(), 3, "underbase and two inks");
    assert_eq!(films[0]["role"], "underbase");
    for f in &films[1..] {
        let c = f["coverage"].as_f64().unwrap();
        assert!((c - 0.5).abs() < 0.03, "{}: {c}", f["name"]);
    }
}

#[test]
fn index_rejects_bad_parameters() {
    let mut s = Session::new();
    let src = png_source(8, 8, |_, _| [200, 30, 30, 255]);
    let inks = json!([{"color": "#c81e1e"}]);
    for bad in [
        json!({"source": src, "inks": inks, "dither": 1.5}),
        json!({"source": src, "inks": inks, "dither": "full"}),
        json!({"source": src, "inks": inks, "cellMm": 0}),
        json!({"source": src, "inks": inks, "cellMm": 9}),
        json!({"source": src, "inks": inks, "base": "white"}),
        json!({"source": src, "colors": 13}),
        json!({"source": src, "inks": vec![json!({"color": "#c81e1e"}); 13]}),
        json!({"source": src, "inks": []}),
    ] {
        assert!(s.execute("screen.index", &bad).is_err(), "{bad}");
    }
}

/// A rectangle on the page with a fill; its id.
fn shape(s: &mut Session, (x, y, w, h): (f64, f64, f64, f64), fill: &str) -> u64 {
    let r = s.execute("shape.rectangle", &json!({"x": x, "y": y, "width": w, "height": h})).unwrap();
    let id = r["id"].as_u64().unwrap();
    s.execute("paint.setFill", &json!({"ids": [id], "color": fill})).unwrap();
    s.execute("paint.setStroke", &json!({"ids": [id], "none": true})).unwrap();
    id
}

fn film<'a>(r: &'a Value, name: &str) -> &'a Value {
    r["films"].as_array().unwrap().iter().find(|f| f["name"] == name).unwrap_or_else(|| panic!("no film {name}: {r}"))
}

#[test]
fn vector_separation_knocks_out_traps_and_has_an_underbase() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 200, "height": 100})).unwrap();
    // Yellow runs under the navy: yellow 0..140 pt, navy 100..200 pt on top.
    shape(&mut s, (0.0, 0.0, 140.0, 100.0), YELLOW);
    shape(&mut s, (100.0, 0.0, 100.0, 100.0), NAVY);
    let r = s.execute("screen.separateVector", &json!({"dpi": 100, "inks": inks(), "underbase": true, "trapMm": 1.0})).unwrap();
    let names: Vec<_> = r["films"].as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Underbase", "Yellow", "Navy"]);
    assert_eq!(r["skippedTotal"], 0);
    let (ub, yellow, navy) = (film_image(film(&r, "Underbase")), film_image(film(&r, "Yellow")), film_image(film(&r, "Navy")));
    // 100 dpi: 100 pt is pixel 138.9. Yellow stops at the navy's edge plus the 1 mm (about 4 px) trap,
    // although the art runs yellow to 140 pt: the navy knocked it out.
    assert!(inked(&yellow, 20, 70) && !inked(&navy, 20, 70));
    assert!(inked(&yellow, 141, 70), "trapped under the navy");
    assert!(!inked(&yellow, 150, 70) && !inked(&yellow, 180, 70), "knocked out under the navy");
    assert!(!inked(&navy, 130, 70) && inked(&navy, 145, 70) && inked(&navy, 270, 70));
    assert!(!inked(&yellow, 250, 70), "no yellow where the art has none");
    // The underbase fills the art and stays in from the edges.
    assert!(inked(&ub, 138, 70) && !inked(&ub, 0, 70) && !inked(&ub, 138, 0));
    // The films come out with edges at the art's own: no smoothing grey, sharp 1-bit.
    assert_eq!(film(&r, "Navy")["format"], "png");
}

#[test]
fn vector_traps_can_be_set_for_one_pair() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 200, "height": 100})).unwrap();
    // Yellow beside navy, side by side at 100 pt; a third, red, beyond the navy.
    shape(&mut s, (0.0, 0.0, 100.0, 100.0), YELLOW);
    shape(&mut s, (100.0, 0.0, 50.0, 100.0), NAVY);
    let spread = |s: &mut Session, extra: Value| {
        let mut p = json!({"dpi": 100, "inks": inks(), "trapMm": 0.25});
        for (k, v) in extra.as_object().unwrap() {
            p[k] = v.clone();
        }
        let r = s.execute("screen.separateVector", &p).unwrap();
        film_image(film(&r, "Yellow"))
    };
    // 100 dpi: the edge is at pixel 138.9. The default 0.25 mm (about 1 px) trap reaches 140 but not 146.
    let plain = spread(&mut s, json!({}));
    assert!(inked(&plain, 139, 70) && !inked(&plain, 146, 70));
    let wide = spread(&mut s, json!({"pairs": [{"spread": "yellow", "under": "Navy", "mm": 2.0}]}));
    assert!(inked(&wide, 146, 70), "2 mm is about 8 px");
    let none = spread(&mut s, json!({"pairs": [{"spread": "Yellow", "under": "Navy", "mm": 0}]}));
    assert!(!inked(&none, 140, 70));
    // The other direction (navy under yellow) is not a pair that traps: yellow prints first.
    let other = spread(&mut s, json!({"pairs": [{"spread": "Navy", "under": "Yellow", "mm": 2.0}]}));
    assert_eq!(other.as_raw(), plain.as_raw());
    for bad in [json!({"pairs": [{"spread": "Nope", "under": "Navy"}]}), json!({"pairs": [{"spread": "Yellow"}]}), json!({"pairs": 3})] {
        let mut p = json!({"dpi": 100, "inks": inks()});
        p["pairs"] = bad["pairs"].clone();
        assert!(s.execute("screen.separateVector", &p).is_err(), "{bad}");
    }
}

#[test]
fn vector_overprint_prints_over_the_ink_below() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 200, "height": 100})).unwrap();
    shape(&mut s, (0.0, 0.0, 200.0, 100.0), YELLOW);
    let top = shape(&mut s, (60.0, 20.0, 80.0, 60.0), NAVY);
    let p = json!({"dpi": 100, "inks": inks(), "trapMm": 0});
    let knocked = s.execute("screen.separateVector", &p).unwrap();
    let (a, b) = (film_image(film(&knocked, "Yellow")), film_image(film(&knocked, "Navy")));
    assert!(!inked(&a, 139, 70) && inked(&b, 139, 70), "knocked out");
    s.execute("object.setOverprint", &json!({"ids": [top], "fill": true})).unwrap();
    let over = s.execute("screen.separateVector", &p).unwrap();
    let (a, b) = (film_image(film(&over, "Yellow")), film_image(film(&over, "Navy")));
    assert!(inked(&a, 139, 70) && inked(&b, 139, 70), "yellow stays under the overprinting navy");
}

#[test]
fn vector_strokes_become_outlines_and_spot_swatches_name_their_inks() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 200, "height": 100})).unwrap();
    s.execute("swatch.new", &json!({"name": "PMS Navy", "color": NAVY, "spot": true})).unwrap();
    // A frame: no fill, a 10 pt stroke in the spot colour.
    let r = s.execute("shape.rectangle", &json!({"x": 20, "y": 20, "width": 160, "height": 60})).unwrap();
    let id = r["id"].as_u64().unwrap();
    s.execute("paint.setFill", &json!({"ids": [id], "none": true})).unwrap();
    s.execute("paint.setStroke", &json!({"ids": [id], "swatch": "PMS Navy"})).unwrap();
    s.execute("stroke.set", &json!({"ids": [id], "weight": 10})).unwrap();
    let out = s.execute("screen.separateVector", &json!({"dpi": 100})).unwrap();
    let inks = out["inks"].as_array().unwrap();
    assert_eq!(inks.len(), 1);
    assert_eq!(inks[0]["name"], "PMS Navy", "the spot swatch names the ink");
    let f = film(&out, "PMS Navy");
    let img = film_image(f);
    // The ring is inked and the middle is not (100 dpi: 1 pt = 1.39 px).
    assert!(inked(&img, 28, 70) || inked(&img, 30, 70), "the stroke");
    assert!(!inked(&img, 139, 70), "the hollow middle");
    assert!(f["coverage"].as_f64().unwrap() > 0.05 && f["coverage"].as_f64().unwrap() < 0.4);
}

#[test]
fn vector_separation_lists_what_it_cannot_separate_and_checks_input() {
    let mut s = Session::new();
    // No document.
    assert!(s.execute("screen.separateVector", &json!({})).is_err());
    s.execute("file.new", &json!({"width": 200, "height": 100})).unwrap();
    // An empty page has nothing to separate.
    assert!(s.execute("screen.separateVector", &json!({})).is_err());
    let g = shape(&mut s, (0.0, 0.0, 100.0, 100.0), YELLOW);
    s.execute("paint.setFill", &json!({"ids": [g], "gradient": {"kind": "linear"}})).unwrap();
    shape(&mut s, (100.0, 0.0, 100.0, 100.0), NAVY);
    let r = s.execute("screen.separateVector", &json!({"dpi": 100, "tones": false})).unwrap();
    assert_eq!(r["skippedTotal"], 1);
    assert!(r["skipped"][0]["why"].as_str().unwrap().contains("gradient"), "{r}");
    assert_eq!(r["inks"].as_array().unwrap().len(), 1, "only the navy was separated");
    let src = png_source(8, 8, |_, _| [200, 30, 30, 255]);
    for bad in [
        json!({"source": src}),
        json!({"dpi": 5}),
        json!({"trapMm": 9}),
        json!({"inks": []}),
        json!({"inks": "navy"}),
        json!({"inks": vec![json!({"color": "#c81e1e"}); 13]}),
        json!({"format": "bmp"}),
        json!({"underbase": {"mesh": 0}}),
    ] {
        assert!(s.execute("screen.separateVector", &bad).is_err(), "{bad}");
    }
}

#[test]
fn vector_separation_honours_clipping_masks() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 200, "height": 100})).unwrap();
    let art = shape(&mut s, (0.0, 0.0, 200.0, 100.0), YELLOW);
    let mask = shape(&mut s, (50.0, 0.0, 50.0, 100.0), NAVY);
    s.execute("select.set", &json!({"ids": [art, mask]})).unwrap();
    s.execute("object.clippingMask.make", &json!({})).unwrap();
    let r = s.execute("screen.separateVector", &json!({"dpi": 100, "trapMm": 0})).unwrap();
    let names: Vec<_> = r["films"].as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap()).collect();
    assert_eq!(names.len(), 1, "the mask itself prints nothing: {names:?}");
    let img = film_image(&r["films"][0]);
    // 100 dpi: the mask is 50..100 pt = pixels 69.4..138.9.
    assert!(inked(&img, 100, 70));
    assert!(!inked(&img, 60, 70) && !inked(&img, 150, 70) && !inked(&img, 250, 70));
}

#[test]
fn vector_plates_can_come_as_vector_pdfs() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 200, "height": 100})).unwrap();
    shape(&mut s, (0.0, 0.0, 140.0, 100.0), YELLOW);
    shape(&mut s, (100.0, 0.0, 100.0, 100.0), NAVY);
    // Off by default.
    let plain = s.execute("screen.separateVector", &json!({"dpi": 100, "inks": inks()})).unwrap();
    assert!(plain["films"][0].get("vector").is_none());
    // Inline: one PDF a film, in the films' order, with paths and no picture.
    let r = s.execute("screen.separateVector", &json!({"dpi": 100, "inks": inks(), "vectorPdf": true})).unwrap();
    assert_eq!(r["films"].as_array().unwrap().len(), 2);
    for f in r["films"].as_array().unwrap() {
        assert_eq!(f["vector"]["format"], "pdf");
        let pdf = vectorcraft_format::base64_decode(f["vector"]["dataBase64"].as_str().unwrap()).unwrap();
        assert!(pdf.starts_with(b"%PDF-"), "{}", f["name"]);
        let text = String::from_utf8_lossy(&pdf);
        // The page is the artboard and holds no picture (the ProcSet always lists /ImageB and /ImageC).
        assert!(text.contains("/MediaBox[0 0 200 100]") && !text.contains("/XObject"), "a vector page, not a bitmap: {}", f["name"]);
    }
    // With marks the page grows by the raster films' margin (64 px = 46.08 pt a side at 100 dpi).
    let m = s.execute("screen.separateVector", &json!({"dpi": 100, "inks": inks(), "vectorPdf": true, "marks": true})).unwrap();
    for f in m["films"].as_array().unwrap() {
        let pdf = vectorcraft_format::base64_decode(f["vector"]["dataBase64"].as_str().unwrap()).unwrap();
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("/MediaBox[0 0 292.16 192.16]"), "{}", f["name"]);
        assert_eq!((f["width"].as_u64(), f["height"].as_u64()), (Some(406), Some(267)), "the raster film grew to match");
    }
    // To a folder: next to the raster film, named for it.
    let dir = vectorcraft_testkit::temp_dir("screen-vector");
    let w = s.execute("screen.separateVector", &json!({"dpi": 100, "inks": inks(), "vectorPdf": true, "dir": dir.to_string_lossy()})).unwrap();
    for f in w["films"].as_array().unwrap() {
        let path = f["vector"]["path"].as_str().unwrap();
        assert!(path.ends_with(".vector.pdf") && path.contains(&format!("-color-{}", f["name"].as_str().unwrap())), "{path}");
        assert!(std::fs::read(path).unwrap().starts_with(b"%PDF-"));
        assert!(f["vector"].get("dataBase64").is_none());
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn vector_spot_tints_and_gradients_print_as_dots() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 200, "height": 100})).unwrap();
    s.execute("swatch.new", &json!({"name": "PMS Navy", "color": NAVY, "spot": true})).unwrap();
    // Left: a 50% tint of the spot (`tint` is a percentage); right: the spot at 100%.
    for (x, tint) in [(0.0, Some(50)), (100.0, None)] {
        let id = shape(&mut s, (x, 0.0, 100.0, 100.0), NAVY);
        let mut p = json!({"ids": [id], "swatch": "PMS Navy"});
        if let Some(t) = tint {
            p["tint"] = json!(t);
        }
        s.execute("paint.setFill", &p).unwrap();
    }
    let r = s.execute("screen.separateVector", &json!({"dpi": 300, "minDot": 0, "maxDot": 1})).unwrap();
    assert_eq!(r["inks"].as_array().unwrap().len(), 1, "both are the one spot ink: {r}");
    let f = &r["films"][0];
    let (tint, solid) = (inked_share(f, 40, 380), inked_share(f, 450, 800));
    assert!((tint - 0.5).abs() < 0.1 && solid > 0.99, "{tint} {solid}");
    assert!(r["notes"].as_array().unwrap().iter().any(|n| n.as_str().unwrap().contains("print as dots")), "{r}");
    // With tones off the tint prints solid and is counted.
    let off = s.execute("screen.separateVector", &json!({"dpi": 300, "tones": false})).unwrap();
    assert!(inked_share(&off["films"][0], 40, 380) > 0.99);
    assert!(off["notes"].as_array().unwrap().iter().any(|n| n.as_str().unwrap().contains("tints")));

    // A white-to-black gradient: dots grow from the light end to the dark end.
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 200, "height": 100})).unwrap();
    let g = shape(&mut s, (0.0, 0.0, 200.0, 100.0), "#808080");
    s.execute("paint.setFill", &json!({"ids": [g], "gradient": {"kind": "linear"}})).unwrap();
    let r = s.execute("screen.separateVector", &json!({"dpi": 300, "minDot": 0, "maxDot": 1})).unwrap();
    assert_eq!(r["skippedTotal"], 0);
    let f = &r["films"][0];
    let (left, mid, right) = (inked_share(f, 20, 120), inked_share(f, 360, 480), inked_share(f, 720, 820));
    assert!(left < 0.25 && right > 0.75 && left < mid && mid < right, "{left} {mid} {right}");
    // FM screening works for tones too.
    let fm = s.execute("screen.separateVector", &json!({"dpi": 300, "minDot": 0, "maxDot": 1, "screening": "fm"})).unwrap();
    let f = &fm["films"][0];
    assert!(inked_share(f, 20, 120) < inked_share(f, 720, 820));
}

#[test]
fn vector_plates_can_come_as_svg() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 200, "height": 100})).unwrap();
    shape(&mut s, (0.0, 0.0, 140.0, 100.0), YELLOW);
    shape(&mut s, (100.0, 0.0, 100.0, 100.0), NAVY);
    let r = s.execute("screen.separateVector", &json!({"dpi": 100, "inks": inks(), "vectorSvg": true})).unwrap();
    for f in r["films"].as_array().unwrap() {
        assert_eq!(f["vector"]["format"], "svg");
        let svg = vectorcraft_format::base64_decode(f["vector"]["dataBase64"].as_str().unwrap()).unwrap();
        let text = String::from_utf8_lossy(&svg);
        assert!(text.contains("<svg") && text.contains("<path") && !text.contains("<image"), "{}: {text}", f["name"]);
    }
    // The PDF wins when both are asked for; a folder gets .vector.svg files.
    let both = s.execute("screen.separateVector", &json!({"dpi": 100, "inks": inks(), "vectorSvg": true, "vectorPdf": true})).unwrap();
    assert_eq!(both["films"][0]["vector"]["format"], "pdf");
    let dir = vectorcraft_testkit::temp_dir("screen-vector-svg");
    let w = s.execute("screen.separateVector", &json!({"dpi": 100, "inks": inks(), "vectorSvg": true, "dir": dir.to_string_lossy()})).unwrap();
    for f in w["films"].as_array().unwrap() {
        let path = f["vector"]["path"].as_str().unwrap();
        assert!(path.ends_with(".vector.svg"), "{path}");
        assert!(std::fs::read_to_string(path).unwrap().contains("<svg"));
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn vector_inks_can_be_one_per_layer() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 200, "height": 100})).unwrap();
    // Layer 1 holds a yellow and a red shape; a second layer holds the navy.
    shape(&mut s, (0.0, 0.0, 60.0, 100.0), YELLOW);
    shape(&mut s, (70.0, 0.0, 60.0, 100.0), "#c81e1e");
    s.execute("layer.new", &json!({"name": "Navy Plate"})).unwrap();
    shape(&mut s, (140.0, 0.0, 60.0, 100.0), NAVY);

    let by_color = s.execute("screen.separateVector", &json!({"dpi": 100})).unwrap();
    assert_eq!(by_color["inks"].as_array().unwrap().len(), 3, "a colour each");
    let r = s.execute("screen.separateVector", &json!({"dpi": 100, "inkBy": "layer"})).unwrap();
    let inks: Vec<_> = r["inks"].as_array().unwrap().iter().map(|i| (i["name"].as_str().unwrap(), i["shapes"].as_u64().unwrap())).collect();
    assert_eq!(inks, [("Layer 1", 2), ("Navy Plate", 1)]);
    let (names, colors): (Vec<_>, Vec<_>) =
        r["films"].as_array().unwrap().iter().map(|f| (f["name"].as_str().unwrap(), f["color"].as_str().unwrap())).unzip();
    assert_eq!(names, ["Layer 1", "Navy Plate"], "light to dark");
    assert_eq!(colors, [YELLOW, NAVY], "a layer takes the colour of its first shape");
    // Both colours of the first layer print on its one film; the navy layer's shape is on the other.
    let (l1, nav) = (film_image(film(&r, "Layer 1")), film_image(film(&r, "Navy Plate")));
    assert!(inked(&l1, 40, 70) && inked(&l1, 130, 70) && !inked(&l1, 250, 70));
    assert!(inked(&nav, 250, 70) && !inked(&nav, 40, 70) && !inked(&nav, 130, 70));
    // An ink named for the layer sets its colour and mesh.
    let o = s
        .execute("screen.separateVector", &json!({"dpi": 100, "inkBy": "layer", "inks": [{"name": "navy plate", "color": "#101050", "mesh": 200}]}))
        .unwrap();
    let f = film(&o, "Navy Plate");
    assert_eq!((f["color"].as_str().unwrap(), f["mesh"].as_f64().unwrap()), ("#101050", 200.0));
    assert_eq!(film(&o, "Layer 1")["mesh"], 156.0, "the others keep the defaults");
    assert!(s.execute("screen.separateVector", &json!({"inkBy": "shape"})).is_err());
}

#[test]
fn vector_inks_can_be_one_per_group() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 200, "height": 100})).unwrap();
    // A yellow and a red shape in one group, a navy one in another, and a loose red one.
    let a = shape(&mut s, (0.0, 0.0, 40.0, 100.0), YELLOW);
    let b = shape(&mut s, (50.0, 0.0, 40.0, 100.0), "#c81e1e");
    s.execute("object.group", &json!({"ids": [a, b]})).unwrap();
    let c = shape(&mut s, (100.0, 0.0, 40.0, 100.0), NAVY);
    s.execute("object.group", &json!({"ids": [c]})).unwrap();
    shape(&mut s, (150.0, 0.0, 40.0, 100.0), "#c81e1e");

    let r = s.execute("screen.separateVector", &json!({"dpi": 100, "inkBy": "group"})).unwrap();
    let mut inks: Vec<_> =
        r["inks"].as_array().unwrap().iter().map(|i| (i["name"].as_str().unwrap().to_string(), i["shapes"].as_u64().unwrap())).collect();
    inks.sort();
    assert_eq!(inks, [("Group 1".to_string(), 2), ("Group 2".to_string(), 1), ("Layer 1".to_string(), 1)]);
    // Both colours of the first group print on its one film.
    let g1 = film_image(film(&r, "Group 1"));
    assert!(inked(&g1, 30, 70) && inked(&g1, 100, 70) && !inked(&g1, 170, 70) && !inked(&g1, 240, 70));
    let loose = film_image(film(&r, "Layer 1"));
    assert!(inked(&loose, 240, 70) && !inked(&loose, 30, 70));
    // By colour the same art makes three inks, but the grouped red and the loose red are one.
    let by_color = s.execute("screen.separateVector", &json!({"dpi": 100})).unwrap();
    assert_eq!(by_color["inks"].as_array().unwrap().len(), 3);
}

#[test]
fn dtf_gives_mirrored_colour_and_a_choked_white_layer() {
    let mut s = Session::new();
    // 300 dpi: red on the left, blue on the right, a transparent column at the far right.
    let src = png_source(60, 20, |x, _| match x {
        0..=29 => [200, 30, 30, 255],
        30..=53 => [20, 40, 170, 255],
        _ => [0, 0, 0, 0],
    });
    let decode =
        |f: &Value| image::load_from_memory(&vectorcraft_format::base64_decode(f["dataBase64"].as_str().unwrap()).unwrap()).unwrap().to_rgba8();
    let r = s.execute("screen.dtf", &json!({"source": src, "whiteChokeMm": 0.3})).unwrap();
    let files = r["files"].as_array().unwrap();
    let names: Vec<_> = files.iter().map(|f| f["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["color", "white"]);
    assert_eq!((r["dpi"].as_f64(), r["width"].as_u64(), r["height"].as_u64()), (Some(300.0), Some(60), Some(20)));
    let (color, white) = (decode(&files[0]), decode(&files[1]));
    // Mirrored by default: the transparent column is on the left now and the red on the right.
    assert_eq!(color.get_pixel(0, 10).0[3], 0);
    assert_eq!(color.get_pixel(59, 10).0, [200, 30, 30, 255]);
    assert_eq!(color.get_pixel(20, 10).0, [20, 40, 170, 255]);
    // The white lies under the colour and 0.3 mm (3.5 px) in from its edges.
    assert_eq!(white.get_pixel(30, 10).0, [255, 255, 255, 255]);
    assert_eq!(white.get_pixel(59 - 1, 10).0[3], 0, "choked in from the art edge");
    assert_eq!(white.get_pixel(3, 10).0[3], 0, "no white where there is no art");
    for y in 0..20 {
        for x in 0..60 {
            if white.get_pixel(x, y).0[3] == 255 {
                assert_eq!(color.get_pixel(x, y).0[3], 255, "({x}, {y})");
            }
        }
    }
    // Unmirrored, no white, written to a folder.
    let dir = vectorcraft_testkit::temp_dir("screen-dtf");
    let o = s.execute("screen.dtf", &json!({"source": src, "mirror": false, "white": false, "dir": dir.to_string_lossy()})).unwrap();
    let files = o["files"].as_array().unwrap();
    assert_eq!(files.len(), 1);
    let path = files[0]["path"].as_str().unwrap();
    assert!(path.ends_with("/dtf-color.png"), "{path}");
    let flat = image::open(path).unwrap().to_rgba8();
    assert_eq!(flat.get_pixel(0, 10).0, [200, 30, 30, 255]);
    let _ = std::fs::remove_dir_all(dir);
    // No document and no source; out-of-range numbers. (Flags follow the other commands: a value
    // that is not a boolean means the default.)
    for bad in
        [json!({"source": src, "dpi": 5}), json!({"source": src, "whiteChokeMm": 9}), json!({"source": src, "whiteChokeMm": "wide"}), json!({})]
    {
        assert!(s.execute("screen.dtf", &bad).is_err(), "{bad}");
    }
}

#[test]
fn films_can_be_mirrored_for_emulsion_down_printing() {
    let mut s = session();
    let plain = s.execute("screen.separate", &json!({"dpi": 100, "inks": inks(), "trapMm": 0})).unwrap();
    let flipped = s.execute("screen.separate", &json!({"dpi": 100, "inks": inks(), "trapMm": 0, "mirror": true})).unwrap();
    let (a, b) = (film_image(film(&plain, "Yellow")), film_image(film(&flipped, "Yellow")));
    assert_eq!(a.dimensions(), b.dimensions());
    let w = a.width();
    // Yellow is the left half: after mirroring it is the right half.
    assert!(inked(&a, 20, 70) && !inked(&a, w - 20, 70));
    assert!(!inked(&b, 20, 70) && inked(&b, w - 20, 70));
    for y in [3, 50, 120] {
        for x in 0..w {
            assert_eq!(inked(&a, x, y), inked(&b, w - 1 - x, y), "x={x} y={y}");
        }
    }
    // Marks are symmetric, so they stay at the same corners.
    let m = s.execute("screen.separate", &json!({"dpi": 100, "inks": inks(), "marks": true, "mirror": true})).unwrap();
    let mi = film_image(film(&m, "Navy"));
    let spec = sc_core::marks::MarkSpec::for_dpi(100.0).unwrap();
    // The top-left mark's centre sits (size - size / 2 + 1) px in from the corner of the framed film.
    let c = (spec.size_px - spec.size_px / 2 + 1) as u32;
    let right = mi.width() - 1 - c;
    assert!(inked(&mi, c, c), "the left mark");
    assert!(inked(&mi, right, c) || inked(&mi, right + 1, c), "the right mark (flipping a mark is a pixel off its twin)");
    // A mirrored vector job mirrors its PDFs too (the same page flipped).
    let v = s.execute("screen.separateVector", &json!({"dpi": 100, "inks": inks(), "vectorPdf": true, "mirror": true})).unwrap();
    assert!(v["films"][0]["vector"]["dataBase64"].is_string());
}

#[test]
fn films_come_as_a_dcs_file_single_or_multiple() {
    let mut s = session();
    let b64 = |v: &Value| vectorcraft_format::base64_decode(v.as_str().unwrap()).unwrap();
    let find = |hay: &[u8], needle: &[u8]| hay.windows(needle.len()).position(|w| w == needle);
    // Single file, inline: the plates inside, in the order of the films.
    let r = s.execute("screen.separate", &json!({"dpi": 100, "inks": inks(), "trapMm": 0, "dcs": "single", "dcsName": "Spring Tee"})).unwrap();
    assert_eq!((r["dcs"]["mode"].as_str(), r["dcs"]["plates"].as_u64()), (Some("single"), Some(2)));
    let dcs = b64(&r["dcs"]["dataBase64"]);
    assert!(dcs.starts_with(b"%!PS-Adobe-2.0 EPSF-1.2\n"));
    let end = find(&dcs, b"%%EndComments\n").unwrap();
    let head = String::from_utf8_lossy(&dcs[..end]).into_owned();
    assert!(head.contains("%%Title: Spring Tee") && head.contains("%%DocumentCustomColors: (Yellow) (Navy)"), "{head}");
    let lines: Vec<_> = head.lines().filter_map(|l| l.strip_prefix("%%PlateFile: ")).collect();
    assert_eq!(lines.len(), 2);
    for (line, name) in lines.iter().zip(["Yellow", "Navy"]) {
        let f: Vec<_> = line.split_whitespace().collect();
        assert_eq!(&f[..2], &[format!("({name})"), "EPS".to_string()]);
        let (off, size): (usize, usize) = (f[2].strip_prefix('#').unwrap().parse().unwrap(), f[3].parse().unwrap());
        assert!(dcs[off..off + size].starts_with(b"%!PS-Adobe") && dcs[off..off + size].ends_with(b"%%Trailer\n"));
    }
    // To a folder: <name>.eps, named for the job.
    let dir = vectorcraft_testkit::temp_dir("screen-dcs");
    let one = s.execute("screen.separate", &json!({"dpi": 100, "inks": inks(), "dcs": "single", "dir": dir.to_string_lossy()})).unwrap();
    let path = one["dcs"]["path"].as_str().unwrap();
    assert!(path.ends_with("/separation.eps") && std::fs::read(path).unwrap().starts_with(b"%!PS-Adobe"), "{path}");
    // Multiple: a main file and a file a plate, which the main file names.
    let many = s
        .execute("screen.separate", &json!({"dpi": 100, "inks": inks(), "dcs": "multiple", "dcsName": "Job", "dir": dir.to_string_lossy()}))
        .unwrap();
    let files: Vec<&str> = many["dcs"]["plateFiles"].as_array().unwrap().iter().map(|f| f.as_str().unwrap()).collect();
    assert_eq!(files.len(), 2);
    let main = std::fs::read(many["dcs"]["path"].as_str().unwrap()).unwrap();
    let main_head = String::from_utf8_lossy(&main[..find(&main, b"%%EndComments\n").unwrap()]).into_owned();
    for f in &files {
        let name = f.rsplit('/').next().unwrap();
        assert!(main_head.contains(&format!("EPS Local {name}")), "{name} in {main_head}");
        assert!(std::fs::read(f).unwrap().starts_with(b"%!PS-Adobe"));
    }
    // The other film commands take it too.
    let src = png_source(60, 60, |_, _| [128, 128, 128, 255]);
    let h = s.execute("screen.halftone", &json!({"source": src, "dpi": 600, "dcs": "single"})).unwrap();
    assert_eq!(h["dcs"]["plates"], 1);
    let _ = std::fs::remove_dir_all(dir);
    // Without it there is no DCS entry; bad values and a missing folder are errors.
    assert!(s.execute("screen.separate", &json!({"dpi": 100, "inks": inks()})).unwrap().get("dcs").is_none());
    for bad in [json!({"dcs": "both"}), json!({"dcs": 1}), json!({"dcs": "multiple"})] {
        let mut p = json!({"dpi": 100, "inks": inks()});
        p.as_object_mut().unwrap().extend(bad.as_object().unwrap().clone());
        assert!(s.execute("screen.separate", &p).is_err(), "{p}");
    }
}

#[test]
fn dtf_can_be_tiff_graded_and_laid_out_on_a_gang_sheet() {
    let mut s = Session::new();
    let decode = |f: &Value| vectorcraft_format::base64_decode(f["dataBase64"].as_str().unwrap()).unwrap();
    // A 50% faded design.
    let src = png_source(60, 20, |x, _| [200, 30, 30, if x < 30 { 255 } else { 200 }]);
    let solid = s.execute("screen.dtf", &json!({"source": src, "mirror": false, "whiteChokeMm": 0})).unwrap();
    let graded = s.execute("screen.dtf", &json!({"source": src, "mirror": false, "whiteChokeMm": 0, "whiteMode": "graded"})).unwrap();
    let alpha = |r: &Value, x| image::load_from_memory(&decode(&r["files"][1])).unwrap().to_rgba8().get_pixel(x, 10).0[3];
    assert_eq!((alpha(&solid, 40), alpha(&graded, 40), alpha(&graded, 10)), (255, 200, 255));
    // TIFF.
    let t = s.execute("screen.dtf", &json!({"source": src, "format": "tiff"})).unwrap();
    assert_eq!(t["files"][0]["format"], "tiff");
    let img = image::load_from_memory_with_format(&decode(&t["files"][0]), image::ImageFormat::Tiff).unwrap().to_rgba8();
    assert_eq!((img.width(), img.height()), (60, 20));

    // A gang sheet of three artboards: 100 × 50 pt each, on a sheet 100 mm wide with 5 mm gaps.
    s.execute("file.new", &json!({"width": 100, "height": 50})).unwrap();
    s.execute("artboard.new", &json!({"x": 200, "y": 0, "width": 100, "height": 50})).unwrap();
    s.execute("artboard.new", &json!({"x": 400, "y": 0, "width": 100, "height": 80})).unwrap();
    for (x, y, w, h, fill) in [(0.0, 0.0, 100.0, 50.0, NAVY), (200.0, 0.0, 100.0, 50.0, YELLOW), (400.0, 0.0, 100.0, 80.0, "#c81e1e")] {
        let r = s.execute("shape.rectangle", &json!({"x": x, "y": y, "width": w, "height": h})).unwrap();
        s.execute("paint.setFill", &json!({"ids": [r["id"].as_u64().unwrap()], "color": fill})).unwrap();
    }
    let g = s.execute("screen.dtf", &json!({"artboards": "all", "sheetWidthMm": 100, "gapMm": 5, "dpi": 100, "mirror": false})).unwrap();
    let place = g["placements"].as_array().unwrap();
    assert_eq!(place.len(), 3);
    // 100 mm is 394 px at 100 dpi and a design is 139 px wide, so two fit on a row with the 20 px gap.
    let sheet = image::load_from_memory(&decode(&g["files"][0])).unwrap().to_rgba8();
    assert_eq!(sheet.width(), 394);
    for pl in place {
        let (x, y) = (pl["x"].as_u64().unwrap() as u32, pl["y"].as_u64().unwrap() as u32);
        assert_eq!(sheet.get_pixel(x + 10, y + 10).0[3], 255, "{pl}");
    }
    // The tallest design (artboard 3, 80 pt) comes first; nothing overlaps; mirroring flips the places.
    assert_eq!(place[2]["x"], 0);
    let m = s.execute("screen.dtf", &json!({"artboards": [3, 1], "sheetWidthMm": 100, "dpi": 100})).unwrap();
    assert_eq!(m["placements"].as_array().unwrap().len(), 2);
    assert_eq!(m["placements"][0]["artboard"], 3);
    assert_eq!(m["placements"][0]["x"].as_u64().unwrap() + m["placements"][0]["width"].as_u64().unwrap(), 394, "mirrored: now at the right");
    for bad in [
        json!({"artboards": "all"}),
        json!({"artboards": "all", "sheetWidthMm": 5}),
        json!({"artboards": [4], "sheetWidthMm": 100}),
        json!({"artboards": [], "sheetWidthMm": 100}),
        json!({"artboards": "some", "sheetWidthMm": 100}),
        json!({"artboards": "all", "sheetWidthMm": 30, "dpi": 300}),
        json!({"source": src, "artboards": "all", "sheetWidthMm": 100}),
        json!({"whiteMode": "pale"}),
        json!({"format": "bmp"}),
    ] {
        assert!(s.execute("screen.dtf", &bad).is_err(), "{bad}");
    }
}

#[test]
fn gang_sheets_can_turn_designs_and_dcs_has_a_chosen_composite_and_managed_cmyk() {
    let mut s = Session::new();
    let decode = |f: &Value| vectorcraft_format::base64_decode(f["dataBase64"].as_str().unwrap()).unwrap();
    // Four artboards 100 × 28.8 pt (a 139 × 40 px design at 100 dpi) on a sheet 200 mm (787 px) wide.
    s.execute("file.new", &json!({"width": 100, "height": 29})).unwrap();
    for i in 1..4 {
        s.execute("artboard.new", &json!({"x": 200.0 * f64::from(i), "y": 0, "width": 100, "height": 29})).unwrap();
    }
    for i in 0..4 {
        let r = s.execute("shape.rectangle", &json!({"x": 200.0 * f64::from(i), "y": 0, "width": 100, "height": 29})).unwrap();
        s.execute("paint.setFill", &json!({"ids": [r["id"].as_u64().unwrap()], "color": NAVY})).unwrap();
    }
    // 60 mm wide: one design (139 px) fits a 236 px row twice only with the gap, so rows run long;
    // standing them up (40 × 139) four fit across.
    let flat = s.execute("screen.dtf", &json!({"artboards": "all", "sheetWidthMm": 60, "gapMm": 2, "dpi": 100, "mirror": false})).unwrap();
    let turned =
        s.execute("screen.dtf", &json!({"artboards": "all", "sheetWidthMm": 60, "gapMm": 2, "dpi": 100, "mirror": false, "rotate": true})).unwrap();
    assert!(turned["height"].as_u64() < flat["height"].as_u64(), "{} vs {}", turned["height"], flat["height"]);
    assert!(flat["placements"].as_array().unwrap().iter().all(|p| p["rotated"] == false));
    let put = turned["placements"].as_array().unwrap();
    assert!(put.iter().all(|p| p["rotated"] == true && p["width"].as_u64() < p["height"].as_u64()), "{turned}");
    let sheet = image::load_from_memory(&decode(&turned["files"][0])).unwrap().to_rgba8();
    for p in put {
        let (x, y) = (p["x"].as_u64().unwrap() as u32, p["y"].as_u64().unwrap() as u32);
        assert_eq!(sheet.get_pixel(x + 5, y + 50).0[3], 255, "{p}");
    }

    // DCS: the composite resolution is chosen and the CMYK comes from the colour settings.
    let mut t = session();
    let inks = inks();
    let dcs_of = |t: &mut Session, extra: Value| {
        let mut p = json!({"dpi": 300, "inks": inks, "dcs": "single"});
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        let r = t.execute("screen.separate", &p).unwrap();
        vectorcraft_format::base64_decode(r["dcs"]["dataBase64"].as_str().unwrap()).unwrap()
    };
    let (small, full) = (dcs_of(&mut t, json!({})), dcs_of(&mut t, json!({"dcsCompositeDpi": 300})));
    assert!(full.len() > small.len() * 3, "a full resolution composite is far larger: {} vs {}", full.len(), small.len());
    let head = |f: &[u8]| String::from_utf8_lossy(&f[..f.windows(14).position(|w| w == b"%%EndComments\n").unwrap()]).into_owned();
    let lines: Vec<String> = head(&small).lines().filter(|l| l.starts_with("%%CMYKCustomColor: ")).map(str::to_string).collect();
    assert_eq!(lines.len(), 2);
    for l in &lines {
        let v: Vec<f64> = l["%%CMYKCustomColor: ".len()..].split_whitespace().take(4).map(|x| x.parse().unwrap()).collect();
        assert!(v.len() == 4 && v.iter().all(|c| (0.0..=1.0).contains(c)), "{l}");
    }
    for bad in [json!({"dcsCompositeDpi": 5}), json!({"dcsCompositeDpi": 9000}), json!({"dcsCompositeDpi": "full"})] {
        let mut p = json!({"dpi": 300, "inks": inks, "dcs": "single"});
        p.as_object_mut().unwrap().extend(bad.as_object().unwrap().clone());
        assert!(t.execute("screen.separate", &p).is_err(), "{p}");
    }
}

#[test]
fn fm_screening_scatters_equal_dots_with_no_angle_or_advice() {
    let mut s = Session::new();
    let navy = [10, 20, 90];
    // A white-to-navy ramp, 240 pixels wide: denser dots to the right.
    let src = png_source(240, 60, |x, _| {
        let c = mix_with_white(navy, f64::from(x) / 239.0);
        [c[0], c[1], c[2], 255]
    });
    let inks = json!([{"name": "Navy", "color": "#0a1458", "mesh": 110}]);
    // dotMm 0.4 at 600 dpi is 9 px a dot. Mesh 110 at 50 lpi would draw advice for an AM screen.
    let p = json!({"source": src, "dpi": 600, "lpi": 50, "inks": inks, "minDot": 0, "maxDot": 1, "screening": "fm", "dotMm": 0.4});
    let r = s.execute("screen.simulate", &p).unwrap();
    let f = &r["films"][0];
    let (left, mid, right) = (inked_share(f, 0, 60), inked_share(f, 90, 150), inked_share(f, 180, 240));
    assert!(left < mid && mid < right, "{left} < {mid} < {right}");
    assert!((left - 0.125).abs() < 0.07 && (mid - 0.5).abs() < 0.07 && (right - 0.875).abs() < 0.07, "{left} {mid} {right}");
    assert_eq!(r["advice"].as_array().unwrap().len(), 0, "an FM screen has no ruling or angle to check");
    // Dots are whole cells of 9 px.
    let img = film_image(f);
    for y in (0..img.height()).step_by(7) {
        for x in (0..img.width()).step_by(5) {
            assert_eq!(inked(&img, x, y), inked(&img, x - x % 9, y - y % 9), "({x}, {y})");
        }
    }
    // The same plate screened AM differs, and AM keeps its advice.
    let am = s.execute("screen.simulate", &json!({"source": src, "dpi": 600, "lpi": 50, "inks": inks, "minDot": 0, "maxDot": 1})).unwrap();
    assert!(!am["advice"].as_array().unwrap().is_empty());
    assert_ne!(film_image(&am["films"][0]), img);
    let explicit_am = s
        .execute("screen.simulate", &json!({"source": src, "dpi": 600, "lpi": 50, "inks": inks, "minDot": 0, "maxDot": 1, "screening": "am"}))
        .unwrap();
    assert_eq!(film_image(&explicit_am["films"][0]), film_image(&am["films"][0]));
    // The single-ink halftone takes it too, with its tone kept and gain compensated.
    let grey = png_source(120, 120, |_, _| [77, 77, 77, 255]);
    let h = s.execute("screen.halftone", &json!({"source": grey, "dpi": 600, "minDot": 0, "maxDot": 1, "screening": "fm", "dotMm": 0.2})).unwrap();
    assert!((h["films"][0]["coverage"].as_f64().unwrap() - 0.70).abs() < 0.04, "{h}");
    assert!(h["advice"].as_array().unwrap().is_empty());
    let g = s.execute("screen.halftone", &json!({"source": grey, "dpi": 600, "minDot": 0, "maxDot": 1, "screening": "fm", "dotGain": 0.2})).unwrap();
    assert!((g["films"][0]["coverage"].as_f64().unwrap() - 0.50).abs() < 0.04, "{g}");
    for bad in
        [json!({"screening": "stochastic"}), json!({"screening": 1}), json!({"screening": "fm", "dotMm": 0}), json!({"screening": "fm", "dotMm": 9})]
    {
        let mut q = json!({"source": grey, "dpi": 600});
        q.as_object_mut().unwrap().extend(bad.as_object().unwrap().clone());
        assert!(s.execute("screen.halftone", &q).is_err(), "{q}");
    }
}

#[test]
fn hybrid_screening_scatters_the_ends_and_angles_the_middle_and_kernels_differ() {
    let mut s = Session::new();
    // A ramp from white to black across 300 pixels at 600 dpi.
    let ramp = png_source(300, 60, |x, _| {
        let v = (255 - x * 255 / 299) as u8;
        [v, v, v, 255]
    });
    let run = |s: &mut Session, extra: Value| {
        let mut p = json!({"source": ramp, "dpi": 600, "minDot": 0, "maxDot": 1});
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        film_image(&s.execute("screen.halftone", &p).unwrap()["films"][0])
    };
    let am = run(&mut s, json!({}));
    let fm = run(&mut s, json!({"screening": "fm", "dotMm": 0.2}));
    let hy = run(&mut s, json!({"screening": "hybrid", "dotMm": 0.2, "hybridBelow": 0.2, "hybridAbove": 0.8}));
    for y in 0..60 {
        for x in 0..300u32 {
            // The tone the engine measures for this column: along the lightness axis to black ink.
            let v = (255 - x * 255 / 299) as u8;
            let t = (100.0 - sc_core::color::srgb_to_lab([v, v, v])[0]) / 100.0;
            if !(0.17..=0.83).contains(&t) {
                assert_eq!(inked(&hy, x, y), inked(&fm, x, y), "scattered at ({x}, {y})");
            } else if (0.23..0.77).contains(&t) {
                assert_eq!(inked(&hy, x, y), inked(&am, x, y), "angled at ({x}, {y})");
            }
        }
    }
    assert_ne!(hy, am);
    assert_ne!(hy, fm);
    // A hybrid keeps its mesh and line-screen advice (it still has angled dots); an FM screen has none.
    let sheet = |s: &mut Session, mode: &str| {
        let r = s.execute("screen.halftone", &json!({"source": ramp, "dpi": 600, "mesh": 60, "lpi": 55, "screening": mode})).unwrap();
        r["advice"].as_array().unwrap().len()
    };
    assert!(sheet(&mut s, "hybrid") > 0 && sheet(&mut s, "fm") == 0 && sheet(&mut s, "am") > 0);
    // The diffusion kernel changes the pattern but not the tone.
    let floyd = run(&mut s, json!({"screening": "fm"}));
    let jarvis = run(&mut s, json!({"screening": "fm", "fmKernel": "jarvis"}));
    let stucki = run(&mut s, json!({"screening": "fm", "fmKernel": "stucki"}));
    assert!(floyd != jarvis && jarvis != stucki && floyd != stucki);
    let ink = |img: &image::GrayImage| img.pixels().filter(|p| p.0[0] == 0).count() as f64 / f64::from(img.width() * img.height());
    assert!((ink(&floyd) - ink(&jarvis)).abs() < 0.01 && (ink(&floyd) - ink(&stucki)).abs() < 0.01);
    // The simulated process takes them too.
    let src = png_source(120, 120, |_, _| [128, 128, 128, 255]);
    let sim = s
        .execute(
            "screen.simulate",
            &json!({"source": src, "dpi": 600, "lpi": 50, "inks": [{"color": "#000000"}], "screening": "hybrid", "fmKernel": "stucki"}),
        )
        .unwrap();
    assert_eq!(sim["films"].as_array().unwrap().len(), 1);
    for bad in [
        json!({"screening": "hybrid", "hybridBelow": 0.9, "hybridAbove": 0.1}),
        json!({"screening": "hybrid", "hybridBelow": 2}),
        json!({"screening": "fm", "fmKernel": "atkinson"}),
        json!({"screening": "fm", "fmKernel": 3}),
        json!({"screening": "stochastic"}),
    ] {
        let mut q = json!({"source": ramp, "dpi": 600});
        q.as_object_mut().unwrap().extend(bad.as_object().unwrap().clone());
        assert!(s.execute("screen.halftone", &q).is_err(), "{q}");
    }
}

#[test]
fn the_print_test_page_makes_numbered_films_without_a_document() {
    let mut s = Session::new();
    let r = s.execute("screen.testPage", &json!({"dpi": 300})).unwrap();
    let names: Vec<_> = r["films"].as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Test tones", "Test lines", "Test trap (light)", "Test trap (dark)"]);
    assert_eq!((r["width"].as_u64(), r["height"].as_u64()), (Some(2598), Some(1063)));
    assert!(r["films"].as_array().unwrap().iter().all(|f| f["coverage"].as_f64().unwrap() > 0.0 && f["width"] == r["width"]));
    assert_eq!(r["layout"]["toneRows"], json!(["AM", "FM", "Hybrid"]));
    assert_eq!(r["layout"]["tonesPercent"][19], 100);
    assert_eq!(r["layout"]["trapHundredthsMm"][8], 50);
    // The 100% patch of the first row is solid ink and the 0% patch is clear: 300 dpi is 11.81 px a mm.
    let tones = film_image(&r["films"][0]);
    let mm = |v: f64| (v * 300.0 / 25.4).round() as u32;
    assert!(inked(&tones, mm(5.0 + 19.0 * 10.5 + 4.0), mm(9.0)));
    assert!(!inked(&tones, mm(9.0), mm(9.0)));
    // Parts can be chosen, the screens' settings apply, marks grow every film alike, and a folder gets files.
    let dir = vectorcraft_testkit::temp_dir("screen-testpage");
    let sub = s
        .execute(
            "screen.testPage",
            &json!({"dpi": 300, "parts": ["trap"], "marks": true, "lpi": 30, "angle": 45, "shape": "ellipse", "dotMm": 0.3, "fmKernel": "jarvis", "dir": dir.to_string_lossy()}),
        )
        .unwrap();
    let films = sub["films"].as_array().unwrap();
    assert_eq!(films.len(), 2);
    assert!(sub["width"].as_u64() > r["width"].as_u64(), "marks add a margin");
    for f in films {
        assert!(std::fs::metadata(f["path"].as_str().unwrap()).unwrap().len() > 0);
    }
    let _ = std::fs::remove_dir_all(dir);
    for bad in [
        json!({"dpi": 50}),
        json!({"dpi": 2400}),
        json!({"parts": []}),
        json!({"parts": ["tones", "colours"]}),
        json!({"parts": "all"}),
        json!({"lpi": 300}),
        json!({"shape": "star"}),
        json!({"fmKernel": "atkinson"}),
        json!({"hybridBelow": 0.9, "hybridAbove": 0.1}),
    ] {
        assert!(s.execute("screen.testPage", &bad).is_err(), "{bad}");
    }
}

#[test]
fn simulate_takes_a_dot_gain_for_each_ink() {
    let mut s = Session::new();
    let src = png_source(120, 60, |_, _| [128, 128, 128, 255]);
    let inks = json!([{"name": "Black", "color": "#000000"}]);
    let mut run = |extra: Value| {
        let mut p = json!({"source": src, "dpi": 600, "lpi": 50, "inks": inks, "minDot": 0, "maxDot": 1});
        for (k, v) in extra.as_object().unwrap() {
            p[k] = v.clone();
        }
        s.execute("screen.simulate", &p)
    };
    let share = |r: &Value| inked_share(&r["films"][0], 0, 120);
    let plain = share(&run(json!({})).unwrap());
    let own = share(&run(json!({"inkGains": [0.2]})).unwrap());
    let global = share(&run(json!({"dotGain": 0.2})).unwrap());
    assert!(own < plain - 0.05, "{own} vs {plain}");
    assert!((own - global).abs() < 0.01, "{own} vs {global}");
    for bad in [json!({"inkGains": [0.3]}), json!({"inkGains": ["a"]}), json!({"inkGains": 0.1}), json!({"inkGains": vec![0.1; 9]})] {
        assert!(run(bad.clone()).is_err(), "{bad}");
    }
}

#[test]
fn simulate_ink_limit_caps_the_darks() {
    let mut s = Session::new();
    let src = png_source(60, 60, |_, _| [0, 0, 0, 255]);
    let inks = json!([{"name": "Black", "color": "#000000"}]);
    let base = json!({"source": src, "dpi": 600, "lpi": 50, "inks": inks, "minDot": 0, "maxDot": 1});
    let share = |r: &Value| inked_share(&r["films"][0], 0, 60);
    let free = share(&s.execute("screen.simulate", &base).unwrap());
    let mut capped = base.clone();
    capped["inkLimit"] = json!(0.7);
    let capped = share(&s.execute("screen.simulate", &capped).unwrap());
    assert!(free > 0.97 && (capped - 0.7).abs() < 0.05, "{free} {capped}");
    for bad in [json!(0.0), json!(1.5), json!("x")] {
        let mut p = base.clone();
        p["inkLimit"] = bad.clone();
        assert!(s.execute("screen.simulate", &p).is_err(), "{bad}");
    }
}

#[test]
fn index_colour_takes_a_diffusion_kernel() {
    let mut s = Session::new();
    let src = png_source(64, 64, |_, _| [188, 188, 188, 255]);
    let inks = json!([{"name": "Black", "color": "#000000"}]);
    let run = |s: &mut Session, k: Option<&str>| {
        let mut p = json!({"source": src, "dpi": 300, "inks": inks, "cellMm": 0.17});
        if let Some(k) = k {
            p["kernel"] = json!(k);
        }
        s.execute("screen.index", &p)
    };
    let fs = run(&mut s, None).unwrap();
    let st = run(&mut s, Some("stucki")).unwrap();
    let (a, b) = (film_image(&fs["films"][0]), film_image(&st["films"][0]));
    assert_ne!(a.as_raw(), b.as_raw(), "different kernels, different patterns");
    assert!(run(&mut s, Some("jarvis")).is_ok() && run(&mut s, Some("nope")).is_err());
    // An ordered pattern is a different pattern again, and unknown names are refused.
    let mut p = json!({"source": src, "dpi": 300, "inks": inks, "cellMm": 0.17, "pattern": "ordered"});
    let ord = s.execute("screen.index", &p).unwrap();
    assert_ne!(film_image(&ord["films"][0]).as_raw(), a.as_raw());
    p["pattern"] = json!("random");
    assert!(s.execute("screen.index", &p).is_err());
}

#[test]
fn a_measured_dot_curve_replaces_the_plain_gain() {
    let mut s = Session::new();
    let src = png_source(120, 60, |_, _| [128, 128, 128, 255]);
    let run = |s: &mut Session, extra: Value| {
        let mut p = json!({"source": src, "dpi": 600, "lpi": 50, "minDot": 0, "maxDot": 1});
        for (k, v) in extra.as_object().unwrap() {
            p[k] = v.clone();
        }
        s.execute("screen.halftone", &p)
    };
    let share = |r: &Value| inked_share(&r["films"][0], 0, 120);
    let plain = share(&run(&mut s, json!({})).unwrap());
    // A press that prints a 50% film dot as 70%: the film is lightened to give back the tone.
    let curved = share(&run(&mut s, json!({"dotCurve": [[0.5, 0.7]]})).unwrap());
    assert!(curved < plain - 0.05, "{curved} vs {plain}");
    // The curve wins over dotGain.
    let both = share(&run(&mut s, json!({"dotCurve": [[0.5, 0.7]], "dotGain": 0.25})).unwrap());
    assert!((both - curved).abs() < 0.001, "{both} vs {curved}");
    for bad in
        [json!({"dotCurve": 3}), json!({"dotCurve": [[0.5]]}), json!({"dotCurve": [[0.5, 0.7], [0.4, 0.8]]}), json!({"dotCurve": [[2.0, 0.5]]})]
    {
        assert!(run(&mut s, bad.clone()).is_err(), "{bad}");
    }
    // It reaches the simulated process too.
    let inks = json!([{"name": "Black", "color": "#000000"}]);
    let sim = |s: &mut Session, curve: Option<Value>| {
        let mut p = json!({"source": src, "dpi": 600, "lpi": 50, "inks": inks, "minDot": 0, "maxDot": 1});
        if let Some(c) = curve {
            p["dotCurve"] = c;
        }
        inked_share(&s.execute("screen.simulate", &p).unwrap()["films"][0], 0, 120)
    };
    assert!(sim(&mut s, Some(json!([[0.5, 0.7]]))) < sim(&mut s, None) - 0.05);
}

#[test]
fn vector_plates_can_be_added_to_the_document_as_layers() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 200, "height": 100})).unwrap();
    shape(&mut s, (0.0, 0.0, 140.0, 100.0), YELLOW);
    shape(&mut s, (100.0, 0.0, 100.0, 100.0), NAVY);
    let before = s.doc().unwrap().doc.layers.len();
    // Off by default: the document is not touched.
    let r = s.execute("screen.separateVector", &json!({"dpi": 100, "inks": inks()})).unwrap();
    assert!(r.get("layers").is_none());
    assert_eq!(s.doc().unwrap().doc.layers.len(), before);
    let r = s.execute("screen.separateVector", &json!({"dpi": 100, "inks": inks(), "intoLayers": true})).unwrap();
    assert_eq!(r["layers"].as_array().unwrap().len(), 2);
    let doc = &s.doc().unwrap().doc;
    assert_eq!(doc.layers.len(), before + 2);
    let names: Vec<_> = doc.layers.iter().filter_map(|l| l.name.clone()).collect();
    assert!(names.contains(&"Plate: Yellow".to_string()) && names.contains(&"Plate: Navy".to_string()), "{names:?}");
    // One undo step takes them out again.
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc.layers.len(), before);
    // And separating again does not pick the plates up as art: they were undone, so nothing doubled.
    let again = s.execute("screen.separateVector", &json!({"dpi": 100, "inks": inks()})).unwrap();
    assert_eq!(again["films"].as_array().unwrap().len(), 2);
}

#[test]
fn vector_opacity_prints_as_dots_over_what_shows_through() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 200, "height": 100})).unwrap();
    shape(&mut s, (0.0, 0.0, 140.0, 100.0), YELLOW);
    let top = shape(&mut s, (100.0, 0.0, 100.0, 100.0), NAVY);
    s.execute("appearance.setItem", &json!({"ids": [top], "index": 0, "opacity": 50})).unwrap();
    let r = s.execute("screen.separateVector", &json!({"dpi": 300, "inks": inks(), "trapMm": 0, "minDot": 0, "maxDot": 1})).unwrap();
    let (navy, yellow) = (film(&r, "Navy"), film(&r, "Yellow"));
    // 300 dpi: 100 pt is pixel 417, 140 pt is 583. Half the navy ink where it is see-through,
    // and the yellow below is not knocked out.
    assert!((inked_share(navy, 450, 550) - 0.5).abs() < 0.1 && (inked_share(navy, 650, 800) - 0.5).abs() < 0.1);
    assert!(inked_share(yellow, 450, 550) > 0.99, "the yellow shows through");
    assert!(!r["notes"].as_array().unwrap().iter().any(|n| n.as_str().unwrap().contains("opacity")), "{r}");
    // With tones off the shape prints solid again and the note comes back.
    let off = s.execute("screen.separateVector", &json!({"dpi": 300, "inks": inks(), "trapMm": 0, "tones": false})).unwrap();
    assert!(inked_share(film(&off, "Navy"), 650, 800) > 0.99);
    assert!(off["notes"].as_array().unwrap().iter().any(|n| n.as_str().unwrap().contains("opacity")), "{off}");
}

#[test]
fn vector_symbol_instances_are_separated_through_their_art() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 200, "height": 100})).unwrap();
    let a = shape(&mut s, (10.0, 10.0, 40.0, 40.0), NAVY);
    s.execute("symbol.new", &json!({"ids": [a], "name": "Badge"})).unwrap();
    s.execute("symbol.place", &json!({"name": "Badge", "x": 150, "y": 50})).unwrap();
    let before = s.doc().unwrap().doc.clone();
    let r = s.execute("screen.separateVector", &json!({"dpi": 100})).unwrap();
    assert_eq!(r["skippedTotal"], 0, "{r}");
    assert_eq!(r["inks"][0]["shapes"], 2);
    let img = film_image(&r["films"][0]);
    // 100 dpi: the original at 10..50 pt (pixels 14..69), the placed one centred on x = 150 pt (pixel 208).
    assert!(inked(&img, 30, 30) && inked(&img, 208, 69) && !inked(&img, 120, 69));
    // The document itself still holds instances: separating works on a copy.
    assert!(std::sync::Arc::ptr_eq(&before, &s.doc().unwrap().doc) || *before == *s.doc().unwrap().doc);
}

#[test]
fn vector_multiply_and_darken_print_over_what_is_below_like_overprint() {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width": 200, "height": 100})).unwrap();
    shape(&mut s, (0.0, 0.0, 140.0, 100.0), YELLOW);
    let top = shape(&mut s, (100.0, 0.0, 100.0, 100.0), NAVY);
    let run = |s: &mut Session| s.execute("screen.separateVector", &json!({"dpi": 100, "inks": inks(), "trapMm": 0})).unwrap();
    // Normal: the navy knocks the yellow out where they meet (141 px is inside the overlap).
    let plain = run(&mut s);
    assert!(!inked(&film_image(film(&plain, "Yellow")), 141, 70));
    for mode in ["multiply", "darken"] {
        s.execute("appearance.setItem", &json!({"ids": [top], "index": 0, "blend": mode})).unwrap();
        let r = run(&mut s);
        assert!(inked(&film_image(film(&r, "Yellow")), 141, 70), "{mode}: the yellow prints under the navy");
        assert!(inked(&film_image(film(&r, "Navy")), 141, 70));
        assert!(!r["notes"].as_array().unwrap().iter().any(|n| n.as_str().unwrap().contains("blend")), "{mode}: {r}");
    }
    // Another mode still cannot be separated and is counted.
    s.execute("appearance.setItem", &json!({"ids": [top], "index": 0, "blend": "difference"})).unwrap();
    assert!(run(&mut s)["notes"].as_array().unwrap().iter().any(|n| n.as_str().unwrap().contains("blend")));
}

#[test]
fn simulate_can_add_a_highlight_white_screen_printed_last() {
    let mut s = Session::new();
    let src = png_source(240, 60, |x, _| {
        let v = (f64::from(x) / 239.0 * 255.0) as u8;
        [v, v, v, 255]
    });
    let inks = json!([{"name": "Black", "color": "#000000"}]);
    let base = json!({"source": src, "dpi": 600, "lpi": 50, "inks": inks, "minDot": 0, "maxDot": 1});
    let plain = s.execute("screen.simulate", &base).unwrap();
    assert_eq!(plain["films"].as_array().unwrap().len(), 1);
    let mut p = base.clone();
    p["highlightWhite"] = json!({"from": 0.5});
    let r = s.execute("screen.simulate", &p).unwrap();
    let films = r["films"].as_array().unwrap();
    assert_eq!(films.len(), 2);
    assert_eq!(films[1]["name"], "Highlight White");
    assert_eq!(films[1]["mesh"], 200.0);
    let f = &films[1];
    assert_eq!(inked_share(f, 0, 100), 0.0);
    assert!(inked_share(f, 220, 240) > inked_share(f, 150, 170) && inked_share(f, 220, 240) > 0.7);
    // true takes the defaults; false and null add nothing; bad values are refused.
    p["highlightWhite"] = json!(true);
    assert_eq!(s.execute("screen.simulate", &p).unwrap()["films"].as_array().unwrap().len(), 2);
    for none in [json!(false), Value::Null] {
        p["highlightWhite"] = none;
        assert_eq!(s.execute("screen.simulate", &p).unwrap()["films"].as_array().unwrap().len(), 1);
    }
    p["highlightWhite"] = json!({"from": 1.5});
    assert!(s.execute("screen.simulate", &p).is_err());
}
