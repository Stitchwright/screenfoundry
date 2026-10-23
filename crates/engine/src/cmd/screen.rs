//! Screen printing (SC-1): turn the artwork into one film per screen.
//!
//! The work is done by `sc-core`; these commands read the artwork (the document's first artboard,
//! an image file or inline image bytes), run the pipeline and return the films as PNG files or
//! base64. Millimetre parameters are converted to film pixels at the job's `dpi`. Every default
//! number (trap, choke, lpi, angle, dot limits, mesh) is an editable rule of thumb, not a fact.

use std::io::Cursor;

use sc_core::color::{parse_hex, to_hex};
use sc_core::dcs::{self, Plate};
use sc_core::dtf::{DtfOptions, gang_sheet, prepare};
use sc_core::fm::{FmOptions, Hybrid, Kernel, MAX_FM_CELL_PX, fm_screen, hybrid_screen};
use sc_core::halftone::{DotShape, HalftoneOptions, MAX_DOT_GAIN, halftone};
use sc_core::index::{MAX_CELL_PX, MAX_INDEX_INKS};
use sc_core::inklib::{self, DEFAULT_MAX_DELTA_E, LibraryInk};
use sc_core::job::{DEFAULT_ANGLES, Film, IndexJob, Ink, Role, SimulatedJob, SpotJob, UnderbaseOptions};
use sc_core::jobsheet::{JobSheet, SheetFilm};
use sc_core::marks::{MarkSpec, frame_with_marks};
use sc_core::profile::{Advice, check, recommended_lpi_range};
use sc_core::separate::{extract_palette, tone_plate};
use sc_core::testpage::{self, Parts, TestPage};
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
            "{source?, dpi?, inks?: [{name?, color: \"#rrggbb\", mesh? (156)}], colors?: how many inks to find in the art when `inks` is not given (4; 1–12), underbase?: true|{name?, color? (#ffffff), mesh? (110), chokeMm? (0.3)}, snap?: true|{maxDeltaE? (25), library?: [{name, color}]} replace each ink by the nearest library ink (see screen.matchInks) and use its name when none is given, trapMm?: spread of colours that print before a darker one (0.25; 0–5), marks?: add registration marks (false), format?: png|tiff|tiff-g4|pdf (png), mirror?: flip every film left to right for emulsion-down printing (false), dcs?: single|multiple (also write the films as a DCS 2.0 file; multiple puts each plate in a file beside a main file and needs `dir`), dcsName?: the job name in the DCS files (separation), dcsCompositeDpi?: the resolution of the composite in the DCS main file (150; 10–2400; the film resolution or more gives a full resolution composite and a large file), jobSheet?: true|{title?, notes?} (a PDF page listing the screens in print order, written as job-sheet.pdf in `dir`), dir?: a folder to write one 1-bit film file a screen (without it: base64)} spot-colour separation: one solid film per screen, the underbase first then the colours light to dark → {dpi, width, height, films: [{name, color, role: underbase|color, format, mesh, coverage, width, height, path | dataBase64}]}",
            always,
            separate
        ),
        cmd!(
            "screen.separateVector",
            "Separate Vector Art",
            [],
            None,
            "{dpi?, inkBy?: color|layer|group (color) with layer each layer that has shapes is one ink named after it and coloured by its first shape (sublayers count as their layer; an entry of `inks` with the layer's name sets its colour and mesh) and with group each outermost group is one ink named after the group (loose shapes of a layer share an ink named after the layer), inks?: [{name?, color, mesh? (156)}] the inks the art's colours go onto (each shape takes the nearest ink and a spot swatch takes the ink of its own name first; without `inks` each colour or spot swatch of the art is an ink up to 12), snap?: true|{maxDeltaE? (25), library?: [{name, color}]} replace each ink by the nearest library ink and use its name when none is given, underbase?: true|{name?, color? (#ffffff), mesh? (110), chokeMm? (0.3)}, trapMm? (0.25), pairs?: [{spread: the name of the lighter ink, under: the name of the ink printed after it that it tucks under, mm (0.25–0 for none; replaces trapMm for that pair)}], marks?, tones? (true: a spot tint below 100% and a gradient print as dots, a tint as its share of the ink and a gradient by the tone of what it draws with its darkest stop as the ink; false prints tints solid and skips gradients), lpi?: the line screen of those dots (45), shape?: round|ellipse|line (round), screening?: am|fm|hybrid (am; fm scatters equal dots with no angle or ruling so nothing can moire; hybrid scatters only the light and dark ends and angles dots between), dotMm?: the size of one FM dot (0.2; 0.02–5), fmKernel?: floyd|jarvis|stucki (floyd; how the error is spread), hybridBelow?: a hybrid scatters cells lighter than this tone (0.15; 0–1), hybridAbove?: a hybrid scatters cells darker than this tone (0.85; 0–1), minDot?: tones below this print nothing (0.08), maxDot?: tones above this print solid (0.95), dotCurve?: [[film, printed]] (a measured curve: what each film tone from 0 to 1 printed as, e.g. [[0.5, 0.7]]; replaces dotGain; read it off the print test page), dotGain?: how much bigger a 50% dot prints than on the film (0 = off; at most 0.25), intoLayers? (false: also add each plate to the open document as a layer named Plate: followed by the ink name; the paths are filled in the ink's colour; one undo step; the ids are in `layers`), vectorSvg?: give each plate as an SVG instead of a PDF in the `vector` entry (black paths, for editing; `vectorPdf` wins when both are given), vectorPdf?: also give each plate as a vector PDF in the `vector` entry of its film (the page is the artboard and with `marks` grows to hold the same four registration marks), format?: png|tiff|tiff-g4|pdf (png), mirror?: flip every film left to right for emulsion-down printing (false), dcs?: single|multiple (also write the films as a DCS 2.0 file; multiple puts each plate in a file beside a main file and needs `dir`), dcsName?: the job name in the DCS files (separation), dcsCompositeDpi?: the resolution of the composite in the DCS main file (150; 10–2400; the film resolution or more gives a full resolution composite and a large file), jobSheet?: true|{title?, notes?} (a PDF page listing the screens in print order, written as job-sheet.pdf in `dir`), dir?} the open document's first artboard separated as paths: solid fills and strokes (type too) go onto inks, a shape knocks out the other inks under it unless it overprints, the underbase and the traps are path operations, and each plate is drawn once at the film resolution; the underbase first then the inks light to dark → {dpi, width, height, films: [{name, color, role, format, mesh, coverage, width, height, path | dataBase64, vector?: {format, path | dataBase64}}], inks: [{name, color, shapes}], skipped: [{object, why}], skippedTotal, notes: [text]}",
            always,
            vector::separate_vector
        ),
        cmd!(
            "screen.dtf",
            "DTF Files",
            [],
            None,
            "{source?, dpi? (300; 25–2400), artboards?: all|[artboard numbers from 1] (a gang sheet of these artboards of the open document instead of the first one alone; needs sheetWidthMm), sheetWidthMm?: the sheet width (a gang sheet needs it; 10–2000), gapMm?: the space between designs (5; 0–100), rotate?: turn designs a quarter turn where that makes the sheet shorter (false), mirror?: flip the art left to right for the film (true), white?: also make the white layer (true), whiteMode?: solid|graded (solid; graded makes the white as opaque as the art), whiteChokeMm?: how far the white is pulled in from the colour edges (0.3; 0–5), format?: png|tiff (png), dir?: a folder to write the files in (without it: base64)} direct-to-film files from the artwork: the colour art on transparent (mirrored for the film) and a white layer (white where white ink prints and the art footprint pulled in by the choke) as a second file of the same size and mirroring; a gang sheet lays the designs out in rows tallest first without rotating them → {dpi, width, height, files: [{name: color|white, format, width, height, path | dataBase64}], placements?: [{artboard, x, y, width, height, rotated, xMm, yMm}] where the position is in the file (after mirroring) and the size is as placed}",
            always,
            dtf_files
        ),
        cmd!(
            "screen.testPage",
            "Print Test Page",
            [],
            None,
            "{dpi? (600; 100–1200), parts?: [tones|lines|trap] (all three; each part is its own screen), mesh? (230), lpi? (45), angle? (22.5), shape?: round|ellipse|line (round), dotMm? (0.2), fmKernel?: floyd|jarvis|stucki (floyd), hybridBelow? (0.15), hybridAbove? (0.85), dotGain? (0; 0–0.25), marks?, format?: png|tiff|tiff-g4|pdf (png), mirror?: flip every film left to right for emulsion-down printing (false), dcs?: single|multiple (also write the films as a DCS 2.0 file; multiple puts each plate in a file beside a main file and needs `dir`), dcsName?: the job name in the DCS files (separation), dcsCompositeDpi?: the resolution of the composite in the DCS main file (150; 10–2400; the film resolution or more gives a full resolution composite and a large file), jobSheet?: true|{title?, notes?} (a PDF page listing the screens in print order; written as job-sheet.pdf in `dir`), dir?: a folder to write the films in (without it: base64)} films of a 220 × 90 mm print-test page to expose and print once: tone patches 0–100% screened as angled dots then scattered dots then a hybrid (tones) and negative and positive lines of eight widths (lines) and light and dark squares overlapping by 0 to 0.5 mm (trap); each patch is numbered. The dot limits are not applied. Needs no open document → {dpi, width, height, films: [{name, color, role, format, mesh, coverage, width, height, path | dataBase64}], layout: {pageMm, toneRows, tonesPercent, lineWidthsHundredthsMm, trapHundredthsMm}}",
            always,
            test_page_films
        ),
        cmd!(
            "screen.simulate",
            "Simulated Process",
            [],
            None,
            "{source?, dpi?, inks?: [{name?, color, mesh? (230)}], colors?: how many inks to find in the art when `inks` is not given (4; 1–8), snap?: true|{maxDeltaE? (25), library?: [{name, color}]} replace each ink by the nearest library ink and use its name when none is given, base?: \"#rrggbb\" the colour under the dots (#ffffff), underbase?: true|{name?, color?, mesh?, chokeMm?}, lpi? (45), angles?: [degrees an ink] (22.5, 67.5, 45, 15, 75, 37.5, 52.5, 7.5), shape?: round|ellipse|line (round), screening?: am|fm|hybrid (am; fm scatters equal dots with no angle or ruling so nothing can moire; hybrid scatters only the light and dark ends and angles dots between), dotMm?: the size of one FM dot (0.2; 0.02–5), fmKernel?: floyd|jarvis|stucki (floyd; how the error is spread), hybridBelow?: a hybrid scatters cells lighter than this tone (0.15; 0–1), hybridAbove?: a hybrid scatters cells darker than this tone (0.85; 0–1), minDot? (0.08), maxDot? (0.95), dotCurve?: [[film, printed]] (a measured curve: what each film tone from 0 to 1 printed as, e.g. [[0.5, 0.7]]; replaces dotGain; read it off the print test page), dotGain?: how much bigger a 50% dot prints than on the film (0 = off; 0.2 = a 50% dot prints as 70%; at most 0.25; the film is lightened to cancel it), inkLimit? (1 = no cap; 0.1–1: the most of the area all inks may cover together; darks come out lighter), inkGains?: [each ink's own dot gain in order, 0–0.25; replaces dotGain for that ink], marks?, format?: png|tiff|tiff-g4|pdf (png), mirror?: flip every film left to right for emulsion-down printing (false), dcs?: single|multiple (also write the films as a DCS 2.0 file; multiple puts each plate in a file beside a main file and needs `dir`), dcsName?: the job name in the DCS files (separation), dcsCompositeDpi?: the resolution of the composite in the DCS main file (150; 10–2400; the film resolution or more gives a full resolution composite and a large file), jobSheet?: true|{title?, notes?} (a PDF page listing the screens in print order, written as job-sheet.pdf in `dir`), dir?} full-colour art as halftone dots of a few inks side by side over the base, each ink at its own screen angle, the underbase first then the inks light to dark → {dpi, width, height, films: [{name, color, role, format, mesh, coverage, width, height, path | dataBase64}], advice: [{ink, code, message}]}",
            always,
            simulate
        ),
        cmd!(
            "screen.index",
            "Index Color",
            [],
            None,
            "{source?, dpi?, inks?: [{name?, color, mesh? (156)}], colors?: how many inks to find in the art when `inks` is not given (6; 1–12), snap?: true|{maxDeltaE? (25), library?: [{name, color}]} replace each ink by the nearest library ink and use its name when none is given, base?: \"#rrggbb\" the colour under the pixels (#ffffff), underbase?: true|{name?, color?, mesh?, chokeMm?}, cellMm?: side of one printed pixel in millimetres (0.5; 0.05–5), dither?: how much of each pixel's error spreads to its neighbours (1; 0 = flat colour; 1 = full error diffusion), kernel?: floyd|jarvis|stucki (floyd; where the error goes: jarvis and stucki are smoother and softer), pattern?: diffusion|ordered (diffusion; ordered repeats an even 8 × 8 pattern instead of spreading error), marks?, format?: png|tiff|tiff-g4|pdf (png), mirror?: flip every film left to right for emulsion-down printing (false), dcs?: single|multiple (also write the films as a DCS 2.0 file; multiple puts each plate in a file beside a main file and needs `dir`), dcsName?: the job name in the DCS files (separation), dcsCompositeDpi?: the resolution of the composite in the DCS main file (150; 10–2400; the film resolution or more gives a full resolution composite and a large file), jobSheet?: true|{title?, notes?} (a PDF page listing the screens in print order, written as job-sheet.pdf in `dir`), dir?} full-colour art reduced to a fixed palette of inks with one ink per square pixel and no overlap or dots, the underbase first then the inks light to dark → {dpi, width, height, cellPx, films: [{name, color, role, format, mesh, coverage, width, height, path | dataBase64}]}",
            always,
            index_colour
        ),
        cmd!(
            "screen.halftone",
            "Halftone Film",
            [],
            None,
            "{source?, dpi?, ink?: \"#rrggbb\" (#000000), name?, mesh? (230), lpi? (45), angle? (22.5), shape?: round|ellipse|line (round), screening?: am|fm|hybrid (am; fm scatters equal dots with no angle or ruling so nothing can moire; hybrid scatters only the light and dark ends and angles dots between), dotMm?: the size of one FM dot (0.2; 0.02–5), fmKernel?: floyd|jarvis|stucki (floyd; how the error is spread), hybridBelow?: a hybrid scatters cells lighter than this tone (0.15; 0–1), hybridAbove?: a hybrid scatters cells darker than this tone (0.85; 0–1), minDot?: tones below this print nothing (0.08), maxDot?: tones above this print solid (0.95), dotCurve?: [[film, printed]] (a measured curve: what each film tone from 0 to 1 printed as, e.g. [[0.5, 0.7]]; replaces dotGain; read it off the print test page), dotGain?: how much bigger a 50% dot prints than on the film (0 = off; 0.2 = a 50% dot prints as 70%; at most 0.25; the film is lightened to cancel it), marks?, format?: png|tiff|tiff-g4|pdf (png), mirror?: flip every film left to right for emulsion-down printing (false), dcs?: single|multiple (also write the films as a DCS 2.0 file; multiple puts each plate in a file beside a main file and needs `dir`), dcsName?: the job name in the DCS files (separation), dcsCompositeDpi?: the resolution of the composite in the DCS main file (150; 10–2400; the film resolution or more gives a full resolution composite and a large file), jobSheet?: true|{title?, notes?} (a PDF page listing the screens in print order, written as job-sheet.pdf in `dir`), dir?} one ink's tones screened into dots → {dpi, width, height, films: [{name, color, role, format, mesh, coverage, width, height, path | dataBase64}], advice: [{code, message}]}",
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
            artboard_art(s, cmd, 0, dpi)
        }
    }
}

/// Artboard `index` (0-based) of the open document drawn at `dpi` on transparent.
fn artboard_art(s: &Session, cmd: &str, index: usize, dpi: f64) -> Result<Rgba8> {
    let st = s.doc().map_err(|_| bad(cmd, "no document is open: give `source` as {path} or {dataBase64}"))?;
    let region = st.doc.artboards.get(index).ok_or_else(|| bad(cmd, format!("the document has no artboard {}", index + 1)))?.rect;
    let scale = dpi / 72.0;
    vectorcraft_render::raster_size(region, scale).map_err(|e| bad(cmd, e))?;
    let r = vectorcraft_render::Renderer::new().render_region(&st.doc, region, scale, false);
    Rgba8::new(r.width as usize, r.height as usize, r.to_straight()).map_err(sc(cmd))
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

/// The films as they print: registration marks around each and, with `mirror` in `p`, flipped left
/// to right (for emulsion-down printing and for machines that expect it). With each film's
/// coverage, measured on the art before the marks and their margin are added.
fn prepare_films(cmd: &str, p: &Value, films: Vec<Film>, dpi: f64, marks: bool) -> Result<(Vec<Film>, Vec<f64>)> {
    let spec = if marks { Some(MarkSpec::for_dpi(dpi).map_err(sc(cmd))?) } else { None };
    let mirror = bool_or(p, "mirror", false);
    let coverage: Vec<f64> = films.iter().map(|f| (f.bitmap.coverage() * 1e4).round() / 1e4).collect();
    let films = films
        .into_iter()
        .map(|f| {
            let framed = match &spec {
                Some(m) => frame_with_marks(&f.bitmap, m).map_err(sc(cmd))?,
                None => f.bitmap,
            };
            Ok(Film { bitmap: if mirror { framed.mirrored() } else { framed }, ..f })
        })
        .collect::<Result<_>>()?;
    Ok((films, coverage))
}

/// The result entries for `films` as they are, written under `dir` when given.
fn emit(cmd: &str, films: Vec<Film>, coverage: &[f64], dpi: f64, fmt: FilmFormat, dir: Option<&str>) -> Result<Vec<Value>> {
    let mut out = Vec::with_capacity(films.len());
    for (i, f) in films.into_iter().enumerate() {
        let coverage = coverage.get(i).copied().unwrap_or_default();
        let name = f.name;
        let bitmap = f.bitmap;
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

/// The DCS 2.0 file or files for `films` when `dcs` (`single` or `multiple`) is given: the plates
/// in one file, or each in a file of its own beside a main file (which needs `dir`). Named after
/// `dcsName`. `None` without `dcs`.
fn dcs_output(cmd: &str, p: &Value, films: &[Film], dpi: f64, dir: Option<&str>) -> Result<Option<Value>> {
    let multiple = match p.get("dcs") {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::String(m)) if m == "single" => false,
        Some(Value::String(m)) if m == "multiple" => true,
        Some(_) => return Err(bad(cmd, "`dcs` is single or multiple")),
    };
    let title = name_of(p.get("dcsName"), "separation".into());
    let base = slug(&title);
    let composite_dpi = num(p, cmd, "dcsCompositeDpi", dcs::DEFAULT_COMPOSITE_DPI, 10.0..=2400.0)?;
    // The CMYK of each spot colour goes through the document's colour settings.
    let cms = vectorcraft_color::cms::active();
    let intent = cms.settings().intent;
    let plates: Vec<Plate> = films
        .iter()
        .map(|f| {
            let cmyk = cms.srgb_to_cmyk(f.ink_rgb.map(|c| f32::from(c) / 255.0), intent).map(f64::from);
            Plate { name: &f.name, rgb: f.ink_rgb, cmyk: Some(cmyk), film: &f.bitmap }
        })
        .collect();
    let folder = dir.map(|d| d.trim_end_matches(['/', '\\']).to_string());
    if multiple {
        let folder = folder.ok_or_else(|| bad(cmd, "`dcs` multiple writes several files: give `dir`"))?;
        let names: Vec<String> = films.iter().enumerate().map(|(i, f)| format!("{base}-{:02}-{}.eps", i + 1, slug(&f.name))).collect();
        let (main, files) = dcs::multiple_files(&plates, dpi, &title, &names, composite_dpi).map_err(sc(cmd))?;
        let main_path = format!("{folder}/{base}.eps");
        super::fileio::write_file(&main_path, &main)?;
        let mut paths = Vec::with_capacity(files.len());
        for (name, bytes) in names.iter().zip(&files) {
            let path = format!("{folder}/{name}");
            super::fileio::write_file(&path, bytes)?;
            paths.push(path);
        }
        return Ok(Some(json!({ "mode": "multiple", "path": main_path, "plateFiles": paths, "plates": plates.len() })));
    }
    let bytes = dcs::single_file(&plates, dpi, &title, composite_dpi).map_err(sc(cmd))?;
    let mut v = json!({ "mode": "single", "plates": plates.len() });
    match folder {
        Some(folder) => {
            let path = format!("{folder}/{base}.eps");
            super::fileio::write_file(&path, &bytes)?;
            v["path"] = json!(path);
        }
        None => v["dataBase64"] = json!(vectorcraft_format::base64_encode(&bytes)),
    }
    Ok(Some(v))
}

/// Marks and mirroring, then the DCS file when asked, then the film files: what every film
/// command ends with. Returns the film entries and the DCS entry.
fn finish_films(cmd: &str, p: &Value, films: Vec<Film>, dpi: f64, marks: bool, fmt: FilmFormat) -> Result<(Vec<Value>, Option<Value>)> {
    let (films, coverage) = prepare_films(cmd, p, films, dpi, marks)?;
    let dcs = dcs_output(cmd, p, &films, dpi, dir_param(p))?;
    Ok((emit(cmd, films, &coverage, dpi, fmt, dir_param(p))?, dcs))
}

/// Adds the DCS entry to a result when one was made.
fn attach_dcs(out: &mut Value, dcs: Option<Value>) {
    if let Some(d) = dcs {
        out["dcs"] = d;
    }
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
    let (films, dcs) = finish_films(C, p, films, dpi, marks, fmt)?;
    let (w, h) = films.first().map(|f| (f["width"].clone(), f["height"].clone())).unwrap_or_default();
    let sheet = job_sheet(C, p, dpi, &films, &[], dir_param(p))?;
    let mut out = json!({ "dpi": dpi, "width": w, "height": h, "films": films });
    attach(&mut out, sheet);
    attach_dcs(&mut out, dcs);
    Ok(out)
}

/// `screen.dtf`: the artwork as direct-to-film files (SC-8): colour art mirrored on transparent,
/// and a white layer; with `artboards` several designs on one gang sheet.
fn dtf_files(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "screen.dtf";
    let dpi = num(p, C, "dpi", DEFAULT_DPI, 25.0..=2400.0)?;
    let choke_mm = num(p, C, "whiteChokeMm", 0.3, 0.0..=5.0)?;
    let mirror = bool_or(p, "mirror", true);
    let graded = match str_param(p, "whiteMode").unwrap_or("solid") {
        "solid" => false,
        "graded" => true,
        _ => return Err(bad(C, "`whiteMode` is solid or graded")),
    };
    let tiff = match str_param(p, "format").unwrap_or("png") {
        "png" => false,
        "tiff" | "tif" => true,
        _ => return Err(bad(C, "`format` is png or tiff")),
    };
    if !matches!(p.get("format"), None | Some(Value::Null | Value::String(_))) {
        return Err(bad(C, "`format` is png or tiff"));
    }
    let px = |mm: f64| (mm / 25.4 * dpi).round().max(0.0) as usize;
    let opts = DtfOptions { mirror, white: bool_or(p, "white", true), white_choke_px: choke_mm / 25.4 * dpi, graded };

    let mut placements = None;
    let img = match p.get("artboards") {
        None | Some(Value::Null) => artwork(s, p, C, dpi)?,
        Some(boards) => {
            if !matches!(p.get("source"), None | Some(Value::Null)) {
                return Err(bad(C, "`artboards` are the open document's: leave `source` out"));
            }
            let count = s.doc().map_err(|_| bad(C, "no document is open"))?.doc.artboards.len();
            let wanted: Vec<usize> = match boards {
                Value::String(a) if a == "all" => (1..=count).collect(),
                Value::Array(a) if !a.is_empty() && a.len() <= sc_core::dtf::MAX_DESIGNS => a
                    .iter()
                    .map(|v| {
                        v.as_u64()
                            .filter(|n| (1..=count as u64).contains(n))
                            .map(|n| n as usize)
                            .ok_or_else(|| bad(C, format!("`artboards` are numbers from 1 to {count}")))
                    })
                    .collect::<Result<_>>()?,
                _ => return Err(bad(C, "`artboards` is \"all\" or a list of artboard numbers")),
            };
            let width_mm = num_req(p, C, "sheetWidthMm", 10.0..=2000.0)?;
            let gap = px(num(p, C, "gapMm", 5.0, 0.0..=100.0)?);
            let designs = wanted.iter().map(|&n| artboard_art(s, C, n - 1, dpi)).collect::<Result<Vec<_>>>()?;
            let (sheet, put) = gang_sheet(&designs, px(width_mm).max(1), gap, bool_or(p, "rotate", false)).map_err(sc(C))?;
            let mm = |v: usize| (v as f64 / dpi * 25.4 * 10.0).round() / 10.0;
            placements = Some(
                put.iter()
                    .zip(&wanted)
                    .map(|(pl, &n)| {
                        // The files are mirrored as a whole, so a design's place is flipped too.
                        let x = if mirror { sheet.width() - pl.x - pl.width } else { pl.x };
                        json!({ "artboard": n, "x": x, "y": pl.y, "width": pl.width, "height": pl.height, "rotated": pl.rotated, "xMm": mm(x), "yMm": mm(pl.y) })
                    })
                    .collect::<Vec<_>>(),
            );
            sheet
        }
    };
    let files = prepare(&img, &opts).map_err(sc(C))?;
    let mut parts = vec![("color", files.color)];
    parts.extend(files.white.map(|w| ("white", w)));
    let dir = dir_param(p);
    let mut out = Vec::with_capacity(parts.len());
    for (name, image) in parts {
        let bytes = if tiff { sc_core::export::tiff_rgba(&image, dpi) } else { sc_core::export::png_rgba(&image, dpi) }.map_err(sc(C))?;
        let format = if tiff { "tiff" } else { "png" };
        let mut v = json!({ "name": name, "format": format, "width": image.width(), "height": image.height() });
        match dir {
            Some(dir) => {
                let path = format!("{}/dtf-{name}.{}", dir.trim_end_matches(['/', '\\']), if tiff { "tif" } else { "png" });
                super::fileio::write_file(&path, &bytes)?;
                v["path"] = json!(path);
            }
            None => v["dataBase64"] = json!(vectorcraft_format::base64_encode(&bytes)),
        }
        out.push(v);
    }
    let (w, h) = out.first().map(|f| (f["width"].clone(), f["height"].clone())).unwrap_or_default();
    let mut result = json!({ "dpi": dpi, "width": w, "height": h, "files": out });
    if let Some(pl) = placements {
        result["placements"] = json!(pl);
    }
    Ok(result)
}

/// `screen.testPage`: films of a print-test page (SC-10) to learn what a shop's screens hold.
fn test_page_films(_: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "screen.testPage";
    let dpi = num(p, C, "dpi", 600.0, 100.0..=1200.0)?;
    let marks = bool_or(p, "marks", false);
    let fmt = FilmFormat::parse(C, p)?;
    let mesh = num(p, C, "mesh", 230.0, 1.0..=1000.0)?;
    let parts = match p.get("parts") {
        None | Some(Value::Null) => Parts { tones: true, lines: true, trap: true },
        Some(Value::Array(a)) if !a.is_empty() && a.len() <= 3 => {
            let mut parts = Parts { tones: false, lines: false, trap: false };
            for v in a {
                match v.as_str() {
                    Some("tones") => parts.tones = true,
                    Some("lines") => parts.lines = true,
                    Some("trap") => parts.trap = true,
                    _ => return Err(bad(C, "`parts` are tones, lines or trap")),
                }
            }
            parts
        }
        Some(_) => return Err(bad(C, "`parts` is a list of tones, lines or trap")),
    };
    let shape = match str_param(p, "shape").unwrap_or("round") {
        "round" => DotShape::Round,
        "ellipse" => DotShape::Ellipse,
        "line" => DotShape::Line,
        _ => return Err(bad(C, "`shape` is round, ellipse or line")),
    };
    // The scattered and hybrid screens of the page take the usual parameters.
    let mut q = if p.is_object() { p.clone() } else { json!({}) };
    q["screening"] = json!("hybrid");
    let scatter = scatter_of(C, &q, dpi)?.ok_or_else(|| bad(C, "internal: no scattered screen"))?;
    let page = TestPage {
        dpi,
        parts,
        am: HalftoneOptions {
            dpi,
            lpi: num(p, C, "lpi", 45.0, 1.0..=500.0)?,
            angle_deg: num(p, C, "angle", 22.5, -3600.0..=3600.0)?,
            shape,
            min_dot: 0.0,
            max_dot: 1.0,
            dot_gain: num(p, C, "dotGain", 0.0, 0.0..=MAX_DOT_GAIN)?,
        },
        fm: scatter.fm,
        hybrid: scatter.hybrid.unwrap_or(Hybrid { below: 0.15, above: 0.85 }),
    };
    let films: Vec<Film> = testpage::test_page(&page)
        .map_err(sc(C))?
        .into_iter()
        .map(|f| Film { name: f.name.to_string(), ink_rgb: f.rgb, mesh, role: Role::Color, bitmap: f.bitmap })
        .collect();
    let (films, dcs) = finish_films(C, p, films, dpi, marks, fmt)?;
    let (w, h) = films.first().map(|f| (f["width"].clone(), f["height"].clone())).unwrap_or_default();
    let sheet = job_sheet(C, p, dpi, &films, &[], dir_param(p))?;
    let mut out = json!({
        "dpi": dpi, "width": w, "height": h, "films": films,
        "layout": {
            "pageMm": [testpage::PAGE_MM.0, testpage::PAGE_MM.1],
            "toneRows": testpage::TONE_ROWS,
            "tonesPercent": testpage::TONES,
            "lineWidthsHundredthsMm": testpage::LINES,
            "trapHundredthsMm": testpage::TRAPS,
        },
    });
    attach(&mut out, sheet);
    attach_dcs(&mut out, dcs);
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
    let ordered = match p.get("pattern") {
        None | Some(Value::Null) => false,
        Some(Value::String(k)) if k == "diffusion" => false,
        Some(Value::String(k)) if k == "ordered" => true,
        Some(_) => return Err(bad(C, "`pattern` is diffusion or ordered")),
    };
    let job = IndexJob { inks, base, underbase: underbase_of(p, C, mm_px)?, cell_px, dither, kernel: kernel_of(p, C, "kernel")?, ordered };
    let films = job.run(&img).map_err(sc(C))?;
    let (films, dcs) = finish_films(C, p, films, dpi, marks, fmt)?;
    let (w, h) = films.first().map(|f| (f["width"].clone(), f["height"].clone())).unwrap_or_default();
    let sheet = job_sheet(C, p, dpi, &films, &[], dir_param(p))?;
    let mut out = json!({ "dpi": dpi, "width": w, "height": h, "cellPx": cell_px, "films": films });
    attach(&mut out, sheet);
    attach_dcs(&mut out, dcs);
    Ok(out)
}

/// `dotCurve`: a measured curve as [[film, printed], …] tones in 0..1 (what each film tone printed as).
fn curve_of(p: &Value, cmd: &str) -> Result<Option<sc_core::curve::ToneCurve>> {
    let list = match p.get("dotCurve") {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::Array(a)) => a,
        Some(_) => return Err(bad(cmd, "`dotCurve` is a list of [film, printed] tones")),
    };
    let pair = |v: &Value| -> Option<(f64, f64)> {
        let a = v.as_array()?;
        if a.len() != 2 {
            return None;
        }
        Some((a.first()?.as_f64()?, a.get(1)?.as_f64()?))
    };
    let points: Vec<(f64, f64)> =
        list.iter().map(pair).collect::<Option<_>>().ok_or_else(|| bad(cmd, "`dotCurve` is a list of [film, printed] tones"))?;
    sc_core::curve::ToneCurve::new(&points).map(Some).map_err(|e| bad(cmd, e.to_string()))
}

/// A tone plate screened into film: angled dots, or scattered ones when `scatter` is given.
fn screen_plate(plate: &sc_core::Gray8, opts: &HalftoneOptions, scatter: &Option<Scatter>) -> sc_core::Result<Bitmap1> {
    match scatter {
        Some(Scatter { fm, hybrid: Some(h) }) => hybrid_screen(plate, opts, fm, h),
        Some(Scatter { fm, hybrid: None }) => fm_screen(plate, fm),
        None => halftone(plate, opts),
    }
}

/// The dot shape named by `shape`: round (default), ellipse or line.
fn shape_of(p: &Value, cmd: &str) -> Result<DotShape> {
    match str_param(p, "shape").unwrap_or("round") {
        "round" => Ok(DotShape::Round),
        "ellipse" => Ok(DotShape::Ellipse),
        "line" => Ok(DotShape::Line),
        _ => Err(bad(cmd, "`shape` is round, ellipse or line")),
    }
}

/// The scattered-dot screening asked for.
struct Scatter {
    fm: FmOptions,
    /// Angled dots in the middle tones, scattered ones only at the ends.
    hybrid: Option<Hybrid>,
}

/// The FM or hybrid screening asked for (`screening`: am, fm or hybrid): the dot size (`dotMm`, in
/// millimetres, made pixels at `dpi`), the dot limits and gain the AM screens take, the diffusion
/// `fmKernel` and, for a hybrid, where it scatters (`hybridBelow`, `hybridAbove`). `None` for AM.
fn scatter_of(cmd: &str, p: &Value, dpi: f64) -> Result<Option<Scatter>> {
    let hybrid = match p.get("screening") {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::String(m)) if m == "am" => return Ok(None),
        Some(Value::String(m)) if m == "fm" => false,
        Some(Value::String(m)) if m == "hybrid" => true,
        Some(_) => return Err(bad(cmd, "`screening` is am, fm or hybrid")),
    };
    let kernel = kernel_of(p, cmd, "fmKernel")?;
    let dot_mm = num(p, cmd, "dotMm", 0.2, 0.02..=5.0)?;
    let fm = FmOptions {
        cell_px: ((dot_mm / 25.4 * dpi).round() as usize).clamp(1, MAX_FM_CELL_PX),
        min_dot: num(p, cmd, "minDot", 0.08, 0.0..=1.0)?,
        max_dot: num(p, cmd, "maxDot", 0.95, 0.0..=1.0)?,
        dot_gain: num(p, cmd, "dotGain", 0.0, 0.0..=MAX_DOT_GAIN)?,
        kernel,
    };
    let hybrid = if hybrid {
        let h = Hybrid { below: num(p, cmd, "hybridBelow", 0.15, 0.0..=1.0)?, above: num(p, cmd, "hybridAbove", 0.85, 0.0..=1.0)? };
        if h.below > h.above {
            return Err(bad(cmd, "`hybridBelow` is not above `hybridAbove`"));
        }
        Some(h)
    } else {
        None
    };
    Ok(Some(Scatter { fm, hybrid }))
}

/// A diffusion kernel named by `key`: floyd (default), jarvis or stucki.
fn kernel_of(p: &Value, cmd: &str, key: &str) -> Result<Kernel> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(Kernel::FloydSteinberg),
        Some(Value::String(k)) if k == "floyd" => Ok(Kernel::FloydSteinberg),
        Some(Value::String(k)) if k == "jarvis" => Ok(Kernel::Jarvis),
        Some(Value::String(k)) if k == "stucki" => Ok(Kernel::Stucki),
        Some(_) => Err(bad(cmd, format!("`{key}` is floyd, jarvis or stucki"))),
    }
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
    let mut scatter = scatter_of(C, p, dpi)?;
    let curve = curve_of(p, C)?;
    if curve.is_some()
        && let Some(sc) = scatter.as_mut()
    {
        sc.fm.dot_gain = 0.0;
    }
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
        fm: scatter.as_ref().map(|s| s.fm),
        hybrid: scatter.as_ref().and_then(|s| s.hybrid),
        ink_limit: num(p, C, "inkLimit", 1.0, sc_core::simulated::MIN_INK_LIMIT..=1.0)?,
        curve,
        gains: match p.get("inkGains") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(a)) if a.len() <= sc_core::simulated::MAX_INKS => a
                .iter()
                .map(|v| v.as_f64().filter(|g| (0.0..=MAX_DOT_GAIN).contains(g)).ok_or_else(|| bad(C, "`inkGains` are dot gains from 0 to 0.25")))
                .collect::<Result<_>>()?,
            Some(_) => return Err(bad(C, "`inkGains` is a list of dot gains, one per ink")),
        },
    };
    // A scattered screen has no ruling or angle to check; a hybrid still has angled dots.
    let fm = job.fm.is_some() && job.hybrid.is_none();
    let films = job.run(&img).map_err(sc(C))?;
    let (films, dcs) = finish_films(C, p, films, dpi, marks, fmt)?;
    let (w, h) = films.first().map(|f| (f["width"].clone(), f["height"].clone())).unwrap_or_default();
    // Mesh and line-screen advice for each ink at the angle it is printed at.
    let mut advice = Vec::new();
    for (i, ink) in inks.iter().enumerate().filter(|_| !fm) {
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
    attach_dcs(&mut out, dcs);
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
    let mut scatter = scatter_of(C, p, dpi)?;
    // A measured curve replaces the plain dot gain.
    let (plate, opts) = match curve_of(p, C)? {
        Some(curve) => {
            if let Some(sc) = scatter.as_mut() {
                sc.fm.dot_gain = 0.0;
            }
            (curve.compensate(&plate).map_err(sc(C))?, HalftoneOptions { dot_gain: 0.0, ..opts })
        }
        None => (plate, opts),
    };
    let bitmap = screen_plate(&plate, &opts, &scatter).map_err(sc(C))?;
    let film = Film { name: name_of(p.get("name"), "Halftone".into()), ink_rgb: ink, mesh, role: Role::Color, bitmap };
    let (films, dcs) = finish_films(C, p, vec![film], dpi, bool_or(p, "marks", false), FilmFormat::parse(C, p)?)?;
    let (w, h) = films.first().map(|f| (f["width"].clone(), f["height"].clone())).unwrap_or_default();
    let pure_fm = matches!(&scatter, Some(Scatter { hybrid: None, .. }));
    let advice: Vec<Value> = if pure_fm { Vec::new() } else { check(mesh, lpi, angle).iter().map(advice_json).collect() };
    let lines: Vec<String> = advice.iter().map(|a| a["message"].as_str().unwrap_or_default().to_string()).collect();
    let sheet = job_sheet(C, p, dpi, &films, &lines, dir_param(p))?;
    let mut out = json!({ "dpi": dpi, "width": w, "height": h, "films": films, "advice": advice });
    attach(&mut out, sheet);
    attach_dcs(&mut out, dcs);
    Ok(out)
}
