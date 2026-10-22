//! Screen printing (SC-1): turn the artwork into one film per screen.
//!
//! The work is done by `sc-core`; these commands read the artwork (the document's first artboard,
//! an image file or inline image bytes), run the pipeline and return the films as PNG files or
//! base64. Millimetre parameters are converted to film pixels at the job's `dpi`. Every default
//! number (trap, choke, lpi, angle, dot limits, mesh) is an editable rule of thumb, not a fact.

use std::io::Cursor;

use sc_core::color::{parse_hex, to_hex};
use sc_core::halftone::{DotShape, HalftoneOptions, MAX_DOT_GAIN, halftone};
use sc_core::index::{MAX_CELL_PX, MAX_INDEX_INKS};
use sc_core::inklib::{self, DEFAULT_MAX_DELTA_E, LibraryInk};
use sc_core::job::{DEFAULT_ANGLES, Film, IndexJob, Ink, Role, SimulatedJob, SpotJob, UnderbaseOptions};
use sc_core::jobsheet::{JobSheet, SheetFilm};
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

mod vector;

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
            query "screen.inkLibrary",
            "Ink Library",
            [],
            None,
            "{} the built-in generic inks (original starting points, not measured or brand colours) → {inks: [{name, color: \"#rrggbb\", family}]}",
            always,
            ink_library
        ),
        cmd!(
            query "screen.matchInks",
            "Match Colors to Inks",
            [],
            None,
            "{colors: [\"#rrggbb\"] (up to 64), maxDeltaE?: how far a match may be, CIE76 (25; 0–150), library?: [{name, color}] your own inks (tried before the built-in ones)} the library ink nearest each colour → {matches: [{color, name, library: \"#rrggbb\", deltaE} | null]} (null when none is within maxDeltaE)",
            always,
            match_inks
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
            "{source?, dpi?, inks?: [{name?, color: \"#rrggbb\", mesh? (156)}], colors?: how many inks to find in the art when `inks` is not given (4; 1–12), underbase?: true|{name?, color? (#ffffff), mesh? (110), chokeMm? (0.3)}, snap?: true|{maxDeltaE? (25), library?: [{name, color}]} replace each ink by the nearest library ink (see screen.matchInks) and use its name when none is given, trapMm?: spread of colours that print before a darker one (0.25; 0–5), marks?: add registration marks (false), format?: png|tiff|tiff-g4|pdf (png), jobSheet?: true|{title?, notes?} (a PDF page listing the screens in print order, written as job-sheet.pdf in `dir`), dir?: a folder to write one 1-bit film file a screen (without it: base64)} spot-colour separation: one solid film per screen, the underbase first then the colours light to dark → {dpi, width, height, films: [{name, color, role: underbase|color, format, mesh, coverage, width, height, path | dataBase64}]}",
            always,
            separate
        ),
        cmd!(
            "screen.separateVector",
            "Separate Vector Art",
            [],
            None,
            "{dpi?, inkBy?: color|layer (color) with layer each layer that has shapes is one ink named after it and coloured by its first shape (sublayers count as their layer; an entry of `inks` with the layer's name sets its colour and mesh), inks?: [{name?, color, mesh? (156)}] the inks the art's colours go onto (each shape takes the nearest ink and a spot swatch takes the ink of its own name first; without `inks` each colour or spot swatch of the art is an ink up to 12), snap?: true|{maxDeltaE? (25), library?: [{name, color}]} replace each ink by the nearest library ink and use its name when none is given, underbase?: true|{name?, color? (#ffffff), mesh? (110), chokeMm? (0.3)}, trapMm? (0.25), marks?, vectorPdf?: also give each plate as a vector PDF in the `vector` entry of its film (the page is the artboard and with `marks` grows to hold the same four registration marks), format?: png|tiff|tiff-g4|pdf (png), jobSheet?: true|{title?, notes?} (a PDF page listing the screens in print order, written as job-sheet.pdf in `dir`), dir?} the open document's first artboard separated as paths: solid fills and strokes (type too) go onto inks, a shape knocks out the other inks under it unless it overprints, the underbase and the traps are path operations, and each plate is drawn once at the film resolution; the underbase first then the inks light to dark → {dpi, width, height, films: [{name, color, role, format, mesh, coverage, width, height, path | dataBase64, vector?: {format, path | dataBase64}}], inks: [{name, color, shapes}], skipped: [{object, why}], skippedTotal, notes: [text]}",
            always,
            vector::separate_vector
        ),
        cmd!(
            "screen.simulate",
            "Simulated Process",
            [],
            None,
            "{source?, dpi?, inks?: [{name?, color, mesh? (230)}], colors?: how many inks to find in the art when `inks` is not given (4; 1–8), snap?: true|{maxDeltaE? (25), library?: [{name, color}]} replace each ink by the nearest library ink and use its name when none is given, base?: \"#rrggbb\" the colour under the dots (#ffffff), underbase?: true|{name?, color?, mesh?, chokeMm?}, lpi? (45), angles?: [degrees an ink] (22.5, 67.5, 45, 15, 75, 37.5, 52.5, 7.5), shape?: round|ellipse|line (round), minDot? (0.08), maxDot? (0.95), dotGain?: how much bigger a 50% dot prints than on the film (0 = off; 0.2 = a 50% dot prints as 70%; at most 0.25; the film is lightened to cancel it), marks?, format?: png|tiff|tiff-g4|pdf (png), jobSheet?: true|{title?, notes?} (a PDF page listing the screens in print order, written as job-sheet.pdf in `dir`), dir?} full-colour art as halftone dots of a few inks side by side over the base, each ink at its own screen angle, the underbase first then the inks light to dark → {dpi, width, height, films: [{name, color, role, format, mesh, coverage, width, height, path | dataBase64}], advice: [{ink, code, message}]}",
            always,
            simulate
        ),
        cmd!(
            "screen.index",
            "Index Color",
            [],
            None,
            "{source?, dpi?, inks?: [{name?, color, mesh? (156)}], colors?: how many inks to find in the art when `inks` is not given (6; 1–12), snap?: true|{maxDeltaE? (25), library?: [{name, color}]} replace each ink by the nearest library ink and use its name when none is given, base?: \"#rrggbb\" the colour under the pixels (#ffffff), underbase?: true|{name?, color?, mesh?, chokeMm?}, cellMm?: side of one printed pixel in millimetres (0.5; 0.05–5), dither?: how much of each pixel's error spreads to its neighbours (1; 0 = flat colour; 1 = full error diffusion), marks?, format?: png|tiff|tiff-g4|pdf (png), jobSheet?: true|{title?, notes?} (a PDF page listing the screens in print order, written as job-sheet.pdf in `dir`), dir?} full-colour art reduced to a fixed palette of inks with one ink per square pixel and no overlap or dots, the underbase first then the inks light to dark → {dpi, width, height, cellPx, films: [{name, color, role, format, mesh, coverage, width, height, path | dataBase64}]}",
            always,
            index_colour
        ),
        cmd!(
            "screen.halftone",
            "Halftone Film",
            [],
            None,
            "{source?, dpi?, ink?: \"#rrggbb\" (#000000), name?, mesh? (230), lpi? (45), angle? (22.5), shape?: round|ellipse|line (round), minDot?: tones below this print nothing (0.08), maxDot?: tones above this print solid (0.95), dotGain?: how much bigger a 50% dot prints than on the film (0 = off; 0.2 = a 50% dot prints as 70%; at most 0.25; the film is lightened to cancel it), marks?, format?: png|tiff|tiff-g4|pdf (png), jobSheet?: true|{title?, notes?} (a PDF page listing the screens in print order, written as job-sheet.pdf in `dir`), dir?} one ink's tones screened into dots → {dpi, width, height, films: [{name, color, role, format, mesh, coverage, width, height, path | dataBase64}], advice: [{code, message}]}",
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

/// A film file format.
#[derive(Clone, Copy)]
enum FilmFormat {
    Png,
    Tiff,
    TiffG4,
    Pdf,
}

impl FilmFormat {
    fn parse(cmd: &str, p: &Value) -> Result<Self> {
        let given = match p.get("format") {
            None | Some(Value::Null) => "png",
            Some(Value::String(f)) => f.as_str(),
            Some(_) => return Err(bad(cmd, "`format` is png, tiff, tiff-g4 or pdf")),
        };
        match given {
            "png" => Ok(Self::Png),
            "tiff" | "tif" => Ok(Self::Tiff),
            "tiff-g4" | "tif-g4" | "g4" => Ok(Self::TiffG4),
            "pdf" => Ok(Self::Pdf),
            _ => Err(bad(cmd, "`format` is png, tiff, tiff-g4 or pdf")),
        }
    }
    fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Pdf => "pdf",
            Self::Tiff | Self::TiffG4 => "tif",
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Tiff => "tiff",
            Self::TiffG4 => "tiff-g4",
            Self::Pdf => "pdf",
        }
    }
    /// A 1-bit file with the film's resolution: black where ink must reach the garment.
    fn encode(self, cmd: &str, film: &Bitmap1, dpi: f64) -> Result<Vec<u8>> {
        match self {
            Self::Png => sc_core::export::png_1bit(film, dpi),
            Self::Tiff => sc_core::export::tiff_1bit(film, dpi),
            Self::TiffG4 => sc_core::export::tiff_g4(film, dpi),
            Self::Pdf => sc_core::export::pdf_g4(film, dpi),
        }
        .map_err(sc(cmd))
    }
}

/// The result entries for `films`, written under `dir` when given.
fn emit(cmd: &str, films: Vec<Film>, dpi: f64, marks: bool, fmt: FilmFormat, dir: Option<&str>) -> Result<Vec<Value>> {
    let spec = if marks { Some(MarkSpec::for_dpi(dpi).map_err(sc(cmd))?) } else { None };
    let mut out = Vec::with_capacity(films.len());
    for (i, f) in films.into_iter().enumerate() {
        let coverage = (f.bitmap.coverage() * 1e4).round() / 1e4;
        let name = f.name;
        let bitmap = match &spec {
            Some(m) => frame_with_marks(&f.bitmap, m).map_err(sc(cmd))?,
            None => f.bitmap,
        };
        let file = fmt.encode(cmd, &bitmap, dpi)?;
        let role = match f.role {
            Role::Underbase => "underbase",
            Role::Color => "color",
        };
        let mut v = json!({
            "name": name, "color": to_hex(f.ink_rgb), "role": role, "format": fmt.name(), "mesh": f.mesh,
            "coverage": coverage,
            "width": bitmap.width(), "height": bitmap.height(),
        });
        match dir {
            Some(dir) => {
                let path = format!("{}/{:02}-{}-{}.{}", dir.trim_end_matches(['/', '\\']), i + 1, role, slug(&name), fmt.extension());
                super::fileio::write_file(&path, &file)?;
                v["path"] = json!(path);
            }
            None => v["dataBase64"] = json!(vectorcraft_format::base64_encode(&file)),
        }
        out.push(v);
    }
    Ok(out)
}

/// The library to match against: the caller's own inks (`library`: [{name, color}]) first, then the built-in ones.
fn library_of(cmd: &str, v: Option<&Value>) -> Result<Vec<LibraryInk>> {
    let mut lib = Vec::new();
    match v {
        None | Some(Value::Null) => {}
        Some(Value::Array(a)) if a.len() <= 500 => {
            for (i, e) in a.iter().enumerate() {
                let rgb = color(cmd, "library[].color", e.get("color"), [0, 0, 0])?;
                lib.push(LibraryInk { name: name_of(e.get("name"), format!("Shop ink {}", i + 1)), rgb, family: "shop".into() });
            }
        }
        Some(_) => return Err(bad(cmd, "`library` is a list of up to 500 {name?, color}")),
    }
    lib.extend(inklib::builtin());
    Ok(lib)
}

fn ink_library(_: &mut Session, _: &Value) -> Result<Value> {
    let inks: Vec<Value> = inklib::builtin().iter().map(|i| json!({ "name": i.name, "color": to_hex(i.rgb), "family": i.family })).collect();
    Ok(json!({ "inks": inks }))
}

fn match_inks(_: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "screen.matchInks";
    let max = num(p, C, "maxDeltaE", DEFAULT_MAX_DELTA_E, 0.0..=150.0)?;
    let lib = library_of(C, p.get("library"))?;
    let colors = match p.get("colors") {
        Some(Value::Array(a)) if a.len() <= 64 => a,
        _ => return Err(bad(C, "`colors` is a list of up to 64 colours like \"#rrggbb\"")),
    };
    let mut matches = Vec::with_capacity(colors.len());
    for c in colors {
        let rgb = color(C, "colors[]", Some(c), [0, 0, 0])?;
        matches.push(match inklib::snap(&lib, rgb, max) {
            Some(m) => json!({
                "color": to_hex(rgb), "name": m.ink.name, "library": to_hex(m.ink.rgb),
                "deltaE": (m.delta_e * 100.0).round() / 100.0,
            }),
            None => Value::Null,
        });
    }
    Ok(json!({ "matches": matches }))
}

/// `snap` (true or {maxDeltaE?, library?}): each colour becomes the nearest library ink within reach,
/// and the library's name where `names[i]` is not given. Two colours never land on one ink: the
/// later keeps its own colour, so every screen stays distinct.
fn snap_inks(cmd: &str, p: &Value, rgbs: &mut [[u8; 3]], names: &mut [Option<String>]) -> Result<()> {
    let opts = match p.get("snap") {
        None | Some(Value::Null | Value::Bool(false)) => return Ok(()),
        Some(Value::Bool(true)) => json!({}),
        Some(o @ Value::Object(_)) => o.clone(),
        Some(_) => return Err(bad(cmd, "`snap` is true or {maxDeltaE?, library?}")),
    };
    let max = num(&opts, cmd, "maxDeltaE", DEFAULT_MAX_DELTA_E, 0.0..=150.0)?;
    let lib = library_of(cmd, opts.get("library"))?;
    let mut used: Vec<[u8; 3]> = Vec::new();
    for (rgb, name) in rgbs.iter_mut().zip(names.iter_mut()) {
        let Some(m) = inklib::snap(&lib, *rgb, max) else { continue };
        if used.contains(&m.ink.rgb) {
            continue;
        }
        used.push(m.ink.rgb);
        *rgb = m.ink.rgb;
        name.get_or_insert_with(|| m.ink.name.clone());
    }
    Ok(())
}

/// The job sheet when `jobSheet` is given (`true` or `{title?, notes?}`): a PDF listing the screens
/// and the advice, written as `job-sheet.pdf` under `dir` or returned as base64.
fn job_sheet(cmd: &str, p: &Value, dpi: f64, films: &[Value], advice: &[String], dir: Option<&str>) -> Result<Option<Value>> {
    let opts = match p.get("jobSheet") {
        None | Some(Value::Null | Value::Bool(false)) => return Ok(None),
        Some(Value::Bool(true)) => json!({}),
        Some(o @ Value::Object(_)) => o.clone(),
        Some(_) => return Err(bad(cmd, "`jobSheet` is true or {title?, notes?}")),
    };
    let text = |k: &str| opts.get(k).and_then(Value::as_str).map(str::trim).unwrap_or_default().chars().take(600).collect::<String>();
    let size = |k: &str| films.first().and_then(|f| f[k].as_u64()).unwrap_or(0) as usize;
    let rows = films
        .iter()
        .map(|f| SheetFilm {
            name: f["name"].as_str().unwrap_or_default().to_string(),
            rgb: f["color"].as_str().and_then(parse_hex).unwrap_or([0, 0, 0]),
            role: f["role"].as_str().unwrap_or("color").to_string(),
            mesh: f["mesh"].as_f64().unwrap_or(0.0),
            coverage: f["coverage"].as_f64().unwrap_or(0.0),
            file: f["path"].as_str().and_then(|p| p.rsplit(['/', '\\']).next()).map(str::to_string),
        })
        .collect();
    let title = Some(text("title")).filter(|t| !t.is_empty()).unwrap_or_else(|| "ScreenFoundry job".into());
    let sheet =
        JobSheet { title, notes: text("notes"), dpi, width_px: size("width"), height_px: size("height"), films: rows, advice: advice.to_vec() };
    let bytes = sc_core::jobsheet::pdf(&sheet).map_err(sc(cmd))?;
    Ok(Some(match dir {
        Some(dir) => {
            let path = format!("{}/job-sheet.pdf", dir.trim_end_matches(['/', '\\']));
            super::fileio::write_file(&path, &bytes)?;
            json!({ "path": path })
        }
        None => json!({ "dataBase64": vectorcraft_format::base64_encode(&bytes) }),
    }))
}

/// Adds the job sheet to a result when one was made.
fn attach(out: &mut Value, sheet: Option<Value>) {
    if let Some(sheet) = sheet {
        out["jobSheet"] = sheet;
    }
}

fn dir_param(p: &Value) -> Option<&str> {
    str_param(p, "dir").map(str::trim).filter(|d| !d.is_empty())
}

/// The inks of a job: the `inks` given (name, color, mesh), else `colors` of them found in the art.
fn inks_of(p: &Value, cmd: &str, img: &Rgba8, max: usize, default_mesh: f64) -> Result<Vec<Ink>> {
    let mut rgbs: Vec<[u8; 3]> = match p.get("inks") {
        Some(Value::Array(a)) => {
            if a.is_empty() || a.len() > max {
                return Err(bad(cmd, format!("`inks` holds 1 to {max} inks")));
            }
            a.iter().map(|i| color(cmd, "inks[].color", i.get("color"), [0, 0, 0])).collect::<Result<_>>()?
        }
        None | Some(Value::Null) => {
            let n = num(p, cmd, "colors", 4.0, 1.0..=max as f64)? as usize;
            extract_palette(img, n).map_err(sc(cmd))?
        }
        Some(_) => return Err(bad(cmd, "`inks` is a list of {name?, color, mesh?}")),
    };
    if rgbs.is_empty() {
        return Err(bad(cmd, "the artwork has no opaque pixels to separate"));
    }
    // A name the caller gave stays; the library's name fills in only where there is none.
    let mut names: Vec<Option<String>> = (0..rgbs.len())
        .map(|i| Some(name_of(p.get("inks").and_then(|a| a.get(i)).and_then(|g| g.get("name")), String::new())).filter(|n| !n.is_empty()))
        .collect();
    snap_inks(cmd, p, &mut rgbs, &mut names)?;
    Ok(rgbs
        .iter()
        .enumerate()
        .map(|(i, &rgb)| {
            let given = p.get("inks").and_then(|a| a.get(i));
            Ink {
                name: names.get(i).cloned().flatten().unwrap_or_else(|| format!("Ink {}", i + 1)),
                rgb,
                mesh: given.and_then(|g| g.get("mesh")).and_then(Value::as_f64).filter(|m| (1.0..=1000.0).contains(m)).unwrap_or(default_mesh),
            }
        })
        .collect())
}

/// The underbase asked for: `underbase` true or {name?, color?, mesh?, chokeMm?}; none when absent or false.
fn underbase_of(p: &Value, cmd: &str, mm_px: impl Fn(f64) -> f64) -> Result<Option<UnderbaseOptions>> {
    match p.get("underbase") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => Ok(None),
        Some(u) => {
            let empty = json!({});
            let u = if u.is_object() { u } else { &empty };
            Ok(Some(UnderbaseOptions {
                ink: Ink {
                    name: name_of(u.get("name"), "Underbase".into()),
                    rgb: color(cmd, "underbase.color", u.get("color"), [255, 255, 255])?,
                    mesh: num(u, cmd, "mesh", 110.0, 1.0..=1000.0)?,
                },
                choke_px: mm_px(num(u, cmd, "chokeMm", 0.3, 0.0..=5.0)?),
            }))
        }
    }
}

fn separate(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "screen.separate";
    let dpi = num(p, C, "dpi", DEFAULT_DPI, 25.0..=2400.0)?;
    let trap_mm = num(p, C, "trapMm", 0.25, 0.0..=5.0)?;
    let marks = bool_or(p, "marks", false);
    let fmt = FilmFormat::parse(C, p)?;
    let mm_px = |mm: f64| mm / 25.4 * dpi;
    let img = artwork(s, p, C, dpi)?;

    let inks = inks_of(p, C, &img, MAX_INKS, 156.0)?;
    let underbase = underbase_of(p, C, mm_px)?;

    let job = SpotJob { inks, underbase, trap_px: mm_px(trap_mm) };
    let films = job.run(&img).map_err(sc(C))?;
    let films = emit(C, films, dpi, marks, fmt, dir_param(p))?;
    let (w, h) = films.first().map(|f| (f["width"].clone(), f["height"].clone())).unwrap_or_default();
    let sheet = job_sheet(C, p, dpi, &films, &[], dir_param(p))?;
    let mut out = json!({ "dpi": dpi, "width": w, "height": h, "films": films });
    attach(&mut out, sheet);
    Ok(out)
}

/// `screen.index`: full-colour art as square pixels of a fixed palette of inks, none overlapping.
fn index_colour(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "screen.index";
    let dpi = num(p, C, "dpi", DEFAULT_DPI, 25.0..=2400.0)?;
    let marks = bool_or(p, "marks", false);
    let fmt = FilmFormat::parse(C, p)?;
    let mm_px = |mm: f64| mm / 25.4 * dpi;
    let cell_px = (mm_px(num(p, C, "cellMm", 0.5, 0.05..=5.0)?).round().max(1.0) as usize).min(MAX_CELL_PX);
    let dither = num(p, C, "dither", 1.0, 0.0..=1.0)?;
    let base = color(C, "base", p.get("base"), [255, 255, 255])?;
    let img = artwork(s, p, C, dpi)?;
    // Index colour has no dots to blend with, so it looks for more inks than the other methods.
    let mut q = p.clone();
    if q.get("inks").is_none()
        && let Some(o) = q.as_object_mut()
    {
        o.entry("colors").or_insert(json!(6));
    }
    let inks = inks_of(&q, C, &img, MAX_INDEX_INKS, 156.0)?;
    let job = IndexJob { inks, base, underbase: underbase_of(p, C, mm_px)?, cell_px, dither };
    let films = job.run(&img).map_err(sc(C))?;
    let films = emit(C, films, dpi, marks, fmt, dir_param(p))?;
    let (w, h) = films.first().map(|f| (f["width"].clone(), f["height"].clone())).unwrap_or_default();
    let sheet = job_sheet(C, p, dpi, &films, &[], dir_param(p))?;
    let mut out = json!({ "dpi": dpi, "width": w, "height": h, "cellPx": cell_px, "films": films });
    attach(&mut out, sheet);
    Ok(out)
}

/// `screen.simulate`: full-colour art as halftone dots of a few inks over a base.
fn simulate(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "screen.simulate";
    let dpi = num(p, C, "dpi", DEFAULT_DPI, 25.0..=2400.0)?;
    let lpi = num(p, C, "lpi", 45.0, 1.0..=500.0)?;
    let marks = bool_or(p, "marks", false);
    let fmt = FilmFormat::parse(C, p)?;
    let mm_px = |mm: f64| mm / 25.4 * dpi;
    let shape = match str_param(p, "shape").unwrap_or("round") {
        "round" => DotShape::Round,
        "ellipse" => DotShape::Ellipse,
        "line" => DotShape::Line,
        _ => return Err(bad(C, "`shape` is round, ellipse or line")),
    };
    let angles: Vec<f64> = match p.get("angles") {
        None | Some(Value::Null) => DEFAULT_ANGLES.to_vec(),
        Some(Value::Array(a)) if !a.is_empty() && a.len() <= 16 => a
            .iter()
            .map(|v| v.as_f64().filter(|d| d.is_finite() && d.abs() <= 3600.0).ok_or_else(|| bad(C, "`angles` are degrees")))
            .collect::<Result<_>>()?,
        Some(_) => return Err(bad(C, "`angles` is a list of 1 to 16 degrees")),
    };
    let base = color(C, "base", p.get("base"), [255, 255, 255])?;
    let img = artwork(s, p, C, dpi)?;
    let inks = inks_of(p, C, &img, sc_core::simulated::MAX_INKS, 230.0)?;
    let job = SimulatedJob {
        inks: inks.clone(),
        base,
        underbase: underbase_of(p, C, mm_px)?,
        angles: angles.clone(),
        halftone: HalftoneOptions {
            dpi,
            lpi,
            angle_deg: 0.0,
            shape,
            min_dot: num(p, C, "minDot", 0.08, 0.0..=1.0)?,
            max_dot: num(p, C, "maxDot", 0.95, 0.0..=1.0)?,
            dot_gain: num(p, C, "dotGain", 0.0, 0.0..=MAX_DOT_GAIN)?,
        },
    };
    let films = job.run(&img).map_err(sc(C))?;
    let films = emit(C, films, dpi, marks, fmt, dir_param(p))?;
    let (w, h) = films.first().map(|f| (f["width"].clone(), f["height"].clone())).unwrap_or_default();
    // Mesh and line-screen advice for each ink at the angle it is printed at.
    let mut advice = Vec::new();
    for (i, ink) in inks.iter().enumerate() {
        let angle = angles.get(i % angles.len()).copied().unwrap_or(22.5);
        for a in check(ink.mesh, lpi, angle) {
            let mut v = advice_json(&a);
            v["ink"] = json!(ink.name);
            advice.push(v);
        }
    }
    let lines: Vec<String> =
        advice.iter().map(|a| format!("{}: {}", a["ink"].as_str().unwrap_or_default(), a["message"].as_str().unwrap_or_default())).collect();
    let sheet = job_sheet(C, p, dpi, &films, &lines, dir_param(p))?;
    let mut out = json!({ "dpi": dpi, "width": w, "height": h, "films": films, "advice": advice });
    attach(&mut out, sheet);
    Ok(out)
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
        dot_gain: num(p, C, "dotGain", 0.0, 0.0..=MAX_DOT_GAIN)?,
    };
    let bitmap = halftone(&plate, &opts).map_err(sc(C))?;
    let film = Film { name: name_of(p.get("name"), "Halftone".into()), ink_rgb: ink, mesh, role: Role::Color, bitmap };
    let films = emit(C, vec![film], dpi, bool_or(p, "marks", false), FilmFormat::parse(C, p)?, dir_param(p))?;
    let (w, h) = films.first().map(|f| (f["width"].clone(), f["height"].clone())).unwrap_or_default();
    let advice: Vec<Value> = check(mesh, lpi, angle).iter().map(advice_json).collect();
    let lines: Vec<String> = advice.iter().map(|a| a["message"].as_str().unwrap_or_default().to_string()).collect();
    let sheet = job_sheet(C, p, dpi, &films, &lines, dir_param(p))?;
    let mut out = json!({ "dpi": dpi, "width": w, "height": h, "films": films, "advice": advice });
    attach(&mut out, sheet);
    Ok(out)
}
