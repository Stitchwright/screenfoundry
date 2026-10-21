//! Screen printing (SC-1): turn the artwork into one film per screen.
//!
//! The work is done by `sc-core`; these commands read the artwork (the document's first artboard,
//! an image file or inline image bytes), run the pipeline and return the films as PNG files or
//! base64. Millimetre parameters are converted to film pixels at the job's `dpi`. Every default
//! number (trap, choke, lpi, angle, dot limits, mesh) is an editable rule of thumb, not a fact.

use std::io::Cursor;

use sc_core::color::{parse_hex, to_hex};
use sc_core::halftone::{DotShape, HalftoneOptions, halftone};
use sc_core::job::{Film, Ink, Role, SpotJob, UnderbaseOptions};
use sc_core::marks::{MarkSpec, frame_with_marks};
use sc_core::profile::{Advice, check, recommended_lpi_range};
use sc_core::separate::{extract_palette, tone_plate};
use sc_core::{Bitmap1, Rgba8};
use serde_json::{Value, json};

use super::*;

/// Source resolution assumed for an image file or inline bytes when `dpi` is not given.
const DEFAULT_DPI: f64 = 300.0;
const MAX_FILE_BYTES: u64 = 256 << 20;
const MAX_IMAGE_SIDE: u32 = 20_000;
const MAX_INKS: usize = 12;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "screen.palette",
            "Find Ink Colors",
            [],
            None,
            "{source?: \"document\" (the first artboard, default)|{path}|{dataBase64}, dpi?: pixels an inch (300; 25–2400), colors?: how many (4; 1–12)} the artwork's dominant colours, most used first → {colors: [\"#rrggbb\"], width, height}",
            always,
            palette
        ),
        cmd!(
            query "screen.advice",
            "Check Mesh and Line Screen",
            [],
            None,
            "{mesh: threads an inch, lpi: screen lines an inch, angle?: degrees (22.5)} whether the line screen suits the mesh, and whether the angle lines up with the mesh threads → {recommendedLpi: {min, max}, advice: [{code, message}]}",
            always,
            mesh_advice
        ),
        cmd!(
            "screen.separate",
            "Separate Colors",
            [],
            None,
            "{source?, dpi?, inks?: [{name?, color: \"#rrggbb\", mesh? (156)}], colors?: how many inks to find in the art when `inks` is not given (4; 1–12), underbase?: true|{name?, color? (#ffffff), mesh? (110), chokeMm? (0.3)}, trapMm?: spread of colours that print before a darker one (0.25; 0–5), marks?: add registration marks (false), dir?: a folder to write one PNG film a screen (without it: base64)} spot-colour separation: one solid film per screen, the underbase first then the colours light to dark → {dpi, width, height, films: [{name, color, role: underbase|color, mesh, coverage, width, height, path | dataBase64}]}",
            always,
            separate
        ),
        cmd!(
            "screen.halftone",
            "Halftone Film",
            [],
            None,
            "{source?, dpi?, ink?: \"#rrggbb\" (#000000), name?, mesh? (230), lpi? (45), angle? (22.5), shape?: round|ellipse|line (round), minDot?: tones below this print nothing (0.08), maxDot?: tones above this print solid (0.95), marks?, dir?} one ink's tones screened into dots → {dpi, width, height, films: [{name, color, role, mesh, coverage, width, height, path | dataBase64}], advice: [{code, message}]}",
            always,
            halftone_film
        ),
    ]
}

fn always(_: &Session) -> std::result::Result<(), String> {
    Ok(())
}

fn num(p: &Value, cmd: &str, key: &str, default: f64, range: std::ops::RangeInclusive<f64>) -> Result<f64> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => {
            v.as_f64().filter(|v| range.contains(v)).ok_or_else(|| bad(cmd, format!("`{key}` is a number from {} to {}", range.start(), range.end())))
        }
    }
}

/// A number the caller has to give.
fn num_req(p: &Value, cmd: &str, key: &str, range: std::ops::RangeInclusive<f64>) -> Result<f64> {
    match p.get(key) {
        None | Some(Value::Null) => Err(bad(cmd, format!("give `{key}`, a number from {} to {}", range.start(), range.end()))),
        Some(_) => num(p, cmd, key, 0.0, range),
    }
}

fn color(cmd: &str, key: &str, v: Option<&Value>, default: [u8; 3]) -> Result<[u8; 3]> {
    match v {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v.as_str().and_then(parse_hex).ok_or_else(|| bad(cmd, format!("`{key}` is a colour like \"#rrggbb\""))),
    }
}

fn name_of(v: Option<&Value>, default: String) -> String {
    v.and_then(Value::as_str).map(|n| n.trim().chars().take(60).collect::<String>()).filter(|n| !n.is_empty()).unwrap_or(default)
}

fn sc(cmd: &str) -> impl Fn(sc_core::ScError) -> EngineError + '_ {
    move |e| bad(cmd, e.to_string())
}

/// The artwork: the first artboard rendered at `dpi`, or an image from a file or inline bytes.
fn artwork(s: &Session, p: &Value, cmd: &str, dpi: f64) -> Result<Rgba8> {
    let bytes = match p.get("source") {
        None | Some(Value::Null) => None,
        Some(Value::String(k)) if k == "document" => None,
        Some(Value::Object(o)) => {
            if let Some(path) = o.get("path").and_then(Value::as_str) {
                let len = std::fs::metadata(path).map_err(|e| bad(cmd, format!("{path}: {e}")))?.len();
                if len > MAX_FILE_BYTES {
                    return Err(bad(cmd, format!("{path} is larger than {} MB", MAX_FILE_BYTES >> 20)));
                }
                Some(std::fs::read(path).map_err(|e| bad(cmd, format!("{path}: {e}")))?)
            } else if let Some(b64) = o.get("dataBase64").and_then(Value::as_str) {
                Some(vectorcraft_format::base64_decode(b64).ok_or_else(|| bad(cmd, "`source.dataBase64` isn't base64"))?)
            } else {
                return Err(bad(cmd, "`source` is \"document\", {path} or {dataBase64}"));
            }
        }
        Some(_) => return Err(bad(cmd, "`source` is \"document\", {path} or {dataBase64}")),
    };
    match bytes {
        Some(bytes) => {
            let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format().map_err(|e| bad(cmd, e.to_string()))?;
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(MAX_IMAGE_SIDE);
            limits.max_image_height = Some(MAX_IMAGE_SIDE);
            limits.max_alloc = Some(1 << 30);
            reader.limits(limits);
            let img = reader.decode().map_err(|e| bad(cmd, format!("can't read the image: {e}")))?.to_rgba8();
            Rgba8::new(img.width() as usize, img.height() as usize, img.into_raw()).map_err(sc(cmd))
        }
        None => {
            // These commands also work on a file, so they stay enabled with no document open;
            // say what to give instead of failing with `NoDocument`.
            let st = s.doc().map_err(|_| bad(cmd, "no document is open: give `source` as {path} or {dataBase64}"))?;
            let region = st.doc.artboards.first().ok_or_else(|| bad(cmd, "the document has no artboard"))?.rect;
            let scale = dpi / 72.0;
            vectorcraft_render::raster_size(region, scale).map_err(|e| bad(cmd, e))?;
            let r = vectorcraft_render::Renderer::new().render_region(&st.doc, region, scale, false);
            Rgba8::new(r.width as usize, r.height as usize, r.to_straight()).map_err(sc(cmd))
        }
    }
}

fn palette(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "screen.palette";
    let dpi = num(p, C, "dpi", DEFAULT_DPI, 25.0..=2400.0)?;
    let n = num(p, C, "colors", 4.0, 1.0..=MAX_INKS as f64)? as usize;
    let img = artwork(s, p, C, dpi)?;
    let colors: Vec<String> = extract_palette(&img, n).map_err(sc(C))?.into_iter().map(to_hex).collect();
    Ok(json!({ "colors": colors, "width": img.width(), "height": img.height() }))
}

fn advice_json(a: &Advice) -> Value {
    match a {
        Advice::LpiTooHigh { max_lpi } => json!({
            "code": "lpiTooHigh",
            "message": format!("this line screen is more than a third of the mesh count: dots will not resolve (at most {max_lpi:.0} lpi)"),
        }),
        Advice::LpiAboveRecommended { recommended_max } => json!({
            "code": "lpiAboveRecommended",
            "message": format!("this line screen is above a quarter of the mesh count: highlights may fill in (recommended at most {recommended_max:.0} lpi)"),
        }),
        Advice::AngleAlignsWithThreads => json!({
            "code": "angleAlignsWithThreads",
            "message": "this angle lines the dots up with the mesh threads, which invites moiré",
        }),
    }
}

fn mesh_advice(_: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "screen.advice";
    let mesh = num_req(p, C, "mesh", 1.0..=1000.0)?;
    let lpi = num_req(p, C, "lpi", 1.0..=500.0)?;
    let angle = num(p, C, "angle", 22.5, -3600.0..=3600.0)?;
    let (min, max) = recommended_lpi_range(mesh);
    Ok(json!({
        "recommendedLpi": { "min": min, "max": max },
        "advice": check(mesh, lpi, angle).iter().map(advice_json).collect::<Vec<_>>(),
    }))
}

/// File-name part: letters, digits and dashes.
fn slug(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let s = s.trim_matches('-');
    if s.is_empty() { "screen".into() } else { s.chars().take(40).collect() }
}

fn film_png(b: &Bitmap1) -> Result<Vec<u8>> {
    // Black where ink must reach the garment, white elsewhere.
    let data: Vec<u8> = b.bits().iter().map(|&on| if on { 0 } else { 255 }).collect();
    let img = image::GrayImage::from_raw(b.width() as u32, b.height() as u32, data)
        .ok_or_else(|| bad("screen", "the film's pixel buffer doesn't match its size"))?;
    let mut out = Vec::new();
    img.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png).map_err(|e| EngineError::Other(format!("PNG encoding failed: {e}")))?;
    Ok(out)
}

/// The result entries for `films`, written under `dir` when given.
fn emit(cmd: &str, films: Vec<Film>, dpi: f64, marks: bool, dir: Option<&str>) -> Result<Vec<Value>> {
    let spec = if marks { Some(MarkSpec::for_dpi(dpi).map_err(sc(cmd))?) } else { None };
    let mut out = Vec::with_capacity(films.len());
    for (i, f) in films.into_iter().enumerate() {
        let coverage = (f.bitmap.coverage() * 1e4).round() / 1e4;
        let name = f.name;
        let bitmap = match &spec {
            Some(m) => frame_with_marks(&f.bitmap, m).map_err(sc(cmd))?,
            None => f.bitmap,
        };
        let png = film_png(&bitmap)?;
        let role = match f.role {
            Role::Underbase => "underbase",
            Role::Color => "color",
        };
        let mut v = json!({
            "name": name, "color": to_hex(f.ink_rgb), "role": role, "mesh": f.mesh,
            "coverage": coverage,
            "width": bitmap.width(), "height": bitmap.height(),
        });
        match dir {
            Some(dir) => {
                let path = format!("{}/{:02}-{}-{}.png", dir.trim_end_matches(['/', '\\']), i + 1, role, slug(&name));
                super::fileio::write_file(&path, &png)?;
                v["path"] = json!(path);
            }
            None => v["dataBase64"] = json!(vectorcraft_format::base64_encode(&png)),
        }
        out.push(v);
    }
    Ok(out)
}

fn dir_param(p: &Value) -> Option<&str> {
    str_param(p, "dir").map(str::trim).filter(|d| !d.is_empty())
}

fn separate(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "screen.separate";
    let dpi = num(p, C, "dpi", DEFAULT_DPI, 25.0..=2400.0)?;
    let trap_mm = num(p, C, "trapMm", 0.25, 0.0..=5.0)?;
    let marks = bool_or(p, "marks", false);
    let mm_px = |mm: f64| mm / 25.4 * dpi;
    let img = artwork(s, p, C, dpi)?;

    let rgbs: Vec<[u8; 3]> = match p.get("inks") {
        Some(Value::Array(a)) => {
            if a.is_empty() || a.len() > MAX_INKS {
                return Err(bad(C, format!("`inks` holds 1 to {MAX_INKS} inks")));
            }
            a.iter().map(|i| color(C, "inks[].color", i.get("color"), [0, 0, 0])).collect::<Result<_>>()?
        }
        None | Some(Value::Null) => {
            let n = num(p, C, "colors", 4.0, 1.0..=MAX_INKS as f64)? as usize;
            extract_palette(&img, n).map_err(sc(C))?
        }
        Some(_) => return Err(bad(C, "`inks` is a list of {name?, color, mesh?}")),
    };
    if rgbs.is_empty() {
        return Err(bad(C, "the artwork has no opaque pixels to separate"));
    }
    let inks: Vec<Ink> = rgbs
        .iter()
        .enumerate()
        .map(|(i, &rgb)| {
            let given = p.get("inks").and_then(|a| a.get(i));
            Ink {
                name: name_of(given.and_then(|g| g.get("name")), format!("Ink {}", i + 1)),
                rgb,
                mesh: given.and_then(|g| g.get("mesh")).and_then(Value::as_f64).filter(|m| (1.0..=1000.0).contains(m)).unwrap_or(156.0),
            }
        })
        .collect();

    let underbase = match p.get("underbase") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => None,
        Some(u) => {
            let empty = json!({});
            let u = if u.is_object() { u } else { &empty };
            Some(UnderbaseOptions {
                ink: Ink {
                    name: name_of(u.get("name"), "Underbase".into()),
                    rgb: color(C, "underbase.color", u.get("color"), [255, 255, 255])?,
                    mesh: num(u, C, "mesh", 110.0, 1.0..=1000.0)?,
                },
                choke_px: mm_px(num(u, C, "chokeMm", 0.3, 0.0..=5.0)?),
            })
        }
    };

    let job = SpotJob { inks, underbase, trap_px: mm_px(trap_mm) };
    let films = job.run(&img).map_err(sc(C))?;
    let films = emit(C, films, dpi, marks, dir_param(p))?;
    let (w, h) = films.first().map(|f| (f["width"].clone(), f["height"].clone())).unwrap_or_default();
    Ok(json!({ "dpi": dpi, "width": w, "height": h, "films": films }))
}

fn halftone_film(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "screen.halftone";
    let dpi = num(p, C, "dpi", DEFAULT_DPI, 25.0..=2400.0)?;
    let lpi = num(p, C, "lpi", 45.0, 1.0..=500.0)?;
    let angle = num(p, C, "angle", 22.5, -3600.0..=3600.0)?;
    let mesh = num(p, C, "mesh", 230.0, 1.0..=1000.0)?;
    let shape = match str_param(p, "shape").unwrap_or("round") {
        "round" => DotShape::Round,
        "ellipse" => DotShape::Ellipse,
        "line" => DotShape::Line,
        _ => return Err(bad(C, "`shape` is round, ellipse or line")),
    };
    let ink = color(C, "ink", p.get("ink"), [0, 0, 0])?;
    let img = artwork(s, p, C, dpi)?;
    let plate = tone_plate(&img, ink).map_err(sc(C))?;
    let opts = HalftoneOptions {
        dpi,
        lpi,
        angle_deg: angle,
        shape,
        min_dot: num(p, C, "minDot", 0.08, 0.0..=1.0)?,
        max_dot: num(p, C, "maxDot", 0.95, 0.0..=1.0)?,
    };
    let bitmap = halftone(&plate, &opts).map_err(sc(C))?;
    let film = Film { name: name_of(p.get("name"), "Halftone".into()), ink_rgb: ink, mesh, role: Role::Color, bitmap };
    let films = emit(C, vec![film], dpi, bool_or(p, "marks", false), dir_param(p))?;
    let (w, h) = films.first().map(|f| (f["width"].clone(), f["height"].clone())).unwrap_or_default();
    Ok(json!({
        "dpi": dpi, "width": w, "height": h, "films": films,
        "advice": check(mesh, lpi, angle).iter().map(advice_json).collect::<Vec<_>>(),
    }))
}
