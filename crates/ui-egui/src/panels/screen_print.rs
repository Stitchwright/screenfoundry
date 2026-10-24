//! Screen Print panel (SC-3): choose the inks, preview the film of every screen, save the films.
//!
//! Thin, like every panel: it keeps the choices, builds the parameters of the `screen.*` commands
//! from them and runs those through the session; the engine does the separating.

use egui::{Color32, RichText, Ui, vec2};
use serde_json::{Value, json};

use crate::VectorcraftApp;
use crate::theme::Tokens;
use crate::widgets::{self, menu_item};

pub const ID: &str = "screenPrint";

/// Resolution of the preview films (dots per inch) for solid films: small, so a preview is quick.
const PREVIEW_DPI: f64 = 100.0;
/// Film pixels per dot cell of a halftone preview: enough to show the dots (the engine needs 4).
const PREVIEW_PIXELS_PER_CELL: f64 = 5.0;
/// Resolution of saved films.
const SAVE_DPI: f64 = 600.0;
/// Longest side of a preview thumbnail (points).
const THUMB: f32 = 110.0;
/// Most inks the panel offers (simulated process separates eight).
const MAX_COLORS: usize = 8;

/// One ink in the list.
#[derive(Clone, Debug, PartialEq)]
pub struct InkRow {
    pub name: String,
    /// `#rrggbb`.
    pub color: String,
    pub mesh: f64,
}

/// How simulated process makes its dots.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Screening {
    /// Angled dots of changing size.
    #[default]
    Am,
    /// Scattered equal dots.
    Fm,
    /// Angled dots in the middle tones and scattered ones at the light and dark ends.
    Hybrid,
}

/// Where error goes in error diffusion (FM screening and index colour).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Diffusion {
    #[default]
    Floyd,
    Jarvis,
    Stucki,
}

impl Diffusion {
    fn id(self) -> &'static str {
        match self {
            Self::Floyd => "floyd",
            Self::Jarvis => "jarvis",
            Self::Stucki => "stucki",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Method {
    /// One solid film per colour (`screen.separate`).
    #[default]
    Spot,
    /// Full colour from a few inks as halftone dots (`screen.simulate`).
    Simulated,
    /// Full colour from a fixed palette of inks, one per square pixel (`screen.index`).
    Index,
    /// The document's paths separated as paths, one film per ink (`screen.separateVector`).
    Vector,
}

impl Method {
    fn command(self) -> &'static str {
        match self {
            Method::Spot => "screen.separate",
            Method::Simulated => "screen.simulate",
            Method::Index => "screen.index",
            Method::Vector => "screen.separateVector",
        }
    }
    fn default_mesh(self) -> f64 {
        match self {
            Method::Spot | Method::Index | Method::Vector => 156.0,
            Method::Simulated => 230.0,
        }
    }
}

/// One film of the last preview.
#[derive(Clone)]
pub struct PreviewFilm {
    pub name: String,
    pub coverage: f64,
    pub texture: egui::TextureHandle,
}

impl std::fmt::Debug for PreviewFilm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreviewFilm").field("name", &self.name).field("coverage", &self.coverage).finish_non_exhaustive()
    }
}

/// What the panel remembers while the app runs (not saved with the preferences).
#[derive(Clone, Debug)]
pub struct ScreenPrintState {
    pub method: Method,
    /// How many colours Find Colors looks for.
    pub colors: usize,
    pub inks: Vec<InkRow>,
    pub underbase: bool,
    /// Spread of the colours that print before a darker one (mm), for spot colours.
    pub trap_mm: f64,
    /// The line screen (lines per inch), for simulated process.
    pub lpi: f64,
    /// Dot gain at 50% in percent, for simulated process (0 turns the compensation off).
    pub dot_gain_pct: f64,
    /// Angled dots (AM), scattered dots (FM) or both, for simulated process.
    pub screening: Screening,
    /// The size of one FM dot in mm.
    pub dot_mm: f64,
    /// A hybrid scatters cells lighter than this tone and darker than the other, in percent.
    pub hybrid_below_pct: f64,
    pub hybrid_above_pct: f64,
    /// Side of one printed pixel in mm, for index colour.
    pub cell_mm: f64,
    /// How much error spreads between pixels in percent, for index colour (0 = flat).
    pub dither_pct: f64,
    /// Where the error goes, for FM dots and index colour.
    pub diffusion: Diffusion,
    /// The most of the area all inks may cover together in percent, for simulated process.
    pub ink_limit_pct: f64,
    /// Add a white screen for the highlights, printed last, for simulated process.
    pub highlight_white: bool,
    pub marks: bool,
    pub films: Vec<PreviewFilm>,
    /// Mesh and angle advice of the last preview, one line each.
    pub advice: Vec<String>,
    /// The last thing that went wrong or was done, shown under the buttons.
    pub message: Option<String>,
}

impl Default for ScreenPrintState {
    fn default() -> Self {
        Self {
            method: Method::Spot,
            colors: 4,
            inks: Vec::new(),
            underbase: false,
            trap_mm: 0.25,
            lpi: 45.0,
            dot_gain_pct: 0.0,
            screening: Screening::Am,
            dot_mm: 0.2,
            hybrid_below_pct: 15.0,
            hybrid_above_pct: 85.0,
            cell_mm: 0.5,
            dither_pct: 100.0,
            diffusion: Diffusion::Floyd,
            ink_limit_pct: 100.0,
            highlight_white: false,
            marks: true,
            films: Vec::new(),
            advice: Vec::new(),
            message: None,
        }
    }
}

impl ScreenPrintState {
    /// The resolution of a preview: low for solid films, finer for halftones, whose dots need a few
    /// film pixels each.
    pub fn preview_dpi(&self) -> f64 {
        match self.method {
            Method::Spot | Method::Vector => PREVIEW_DPI,
            Method::Simulated => (self.lpi * PREVIEW_PIXELS_PER_CELL).clamp(PREVIEW_DPI, 1000.0),
            // A printed pixel of the default size is a few preview pixels wide.
            Method::Index => 2.0 * PREVIEW_DPI,
        }
    }

    /// The command to run and its parameters, at `dpi`, with registration marks when `marks`.
    pub fn request(&self, dpi: f64, marks: bool) -> (&'static str, Value) {
        let mut p = json!({ "dpi": dpi, "marks": marks, "underbase": self.underbase });
        // Vector art finds its own inks in the paths; the others look for `colors` in the picture.
        if self.inks.is_empty() {
            if self.method != Method::Vector {
                p["colors"] = json!(self.colors);
            }
        } else {
            p["inks"] = json!(self.inks.iter().map(|i| json!({ "name": i.name, "color": i.color, "mesh": i.mesh })).collect::<Vec<_>>());
        }
        match self.method {
            Method::Spot | Method::Vector => p["trapMm"] = json!(self.trap_mm),
            Method::Simulated => {
                p["lpi"] = json!(self.lpi);
                p["dotGain"] = json!(self.dot_gain_pct / 100.0);
                p["inkLimit"] = json!(self.ink_limit_pct / 100.0);
                if self.highlight_white {
                    p["highlightWhite"] = json!(true);
                }
                match self.screening {
                    Screening::Am => {}
                    Screening::Fm => {
                        p["screening"] = json!("fm");
                        p["dotMm"] = json!(self.dot_mm);
                        p["fmKernel"] = json!(self.diffusion.id());
                    }
                    Screening::Hybrid => {
                        p["screening"] = json!("hybrid");
                        p["dotMm"] = json!(self.dot_mm);
                        p["fmKernel"] = json!(self.diffusion.id());
                        p["hybridBelow"] = json!(self.hybrid_below_pct / 100.0);
                        p["hybridAbove"] = json!(self.hybrid_above_pct / 100.0);
                    }
                }
            }
            Method::Index => {
                p["cellMm"] = json!(self.cell_mm);
                p["dither"] = json!(self.dither_pct / 100.0);
                p["kernel"] = json!(self.diffusion.id());
            }
        }
        (self.method.command(), p)
    }
}

/// `#rrggbb` (or `rrggbb`) as a colour; None when it is not one.
fn parse_hex(s: &str) -> Option<Color32> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 || !h.is_ascii() {
        return None;
    }
    let v = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    Some(Color32::from_rgb(v(0)?, v(2)?, v(4)?))
}

fn run(app: &mut VectorcraftApp, id: &str, params: Value) -> Result<Value, String> {
    app.session.execute(id, &params).map_err(|e| e.to_string())
}

/// Find Colors: the dominant colours of the first artboard become the ink list.
fn find_colors(app: &mut VectorcraftApp, st: &mut ScreenPrintState) {
    match run(app, "screen.palette", json!({ "colors": st.colors, "dpi": 150 })) {
        Ok(r) => {
            let mesh = st.method.default_mesh();
            st.inks = r["colors"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .enumerate()
                        .filter_map(|(i, c)| Some(InkRow { name: format!("Ink {}", i + 1), color: c.as_str()?.to_string(), mesh }))
                        .collect()
                })
                .unwrap_or_default();
            st.message = None;
        }
        Err(e) => st.message = Some(e),
    }
}

/// Snap to Library: each ink takes the nearest library ink within reach, and its name unless the
/// ink was renamed. Two inks never end up the same colour (the later keeps its own).
fn snap_to_library(app: &mut VectorcraftApp, st: &mut ScreenPrintState) {
    let colors: Vec<&str> = st.inks.iter().map(|i| i.color.as_str()).collect();
    match run(app, "screen.matchInks", json!({ "colors": colors })) {
        Ok(r) => {
            let mut used: Vec<String> = Vec::new();
            for (ink, m) in st.inks.iter_mut().zip(r["matches"].as_array().into_iter().flatten()) {
                let (Some(color), Some(name)) = (m["library"].as_str(), m["name"].as_str()) else { continue };
                if used.iter().any(|u| u == color) {
                    continue;
                }
                used.push(color.to_string());
                ink.color = color.to_string();
                if ink.name.strip_prefix("Ink ").is_some_and(|n| n.parse::<u32>().is_ok()) {
                    ink.name = name.to_string();
                }
            }
            st.message = None;
        }
        Err(e) => st.message = Some(e),
    }
}

/// Preview: run the separation at a low resolution and keep each film as a thumbnail.
fn preview(app: &mut VectorcraftApp, ctx: &egui::Context, st: &mut ScreenPrintState) {
    let (cmd, params) = st.request(st.preview_dpi(), false);
    st.films.clear();
    st.advice.clear();
    let result = match run(app, cmd, params) {
        Ok(r) => r,
        Err(e) => {
            st.message = Some(e);
            return;
        }
    };
    for (i, f) in result["films"].as_array().map(Vec::as_slice).unwrap_or_default().iter().enumerate() {
        let Some(bytes) = f["dataBase64"].as_str().and_then(vectorcraft_format::base64_decode) else { continue };
        let Ok(img) = image::load_from_memory(&bytes) else { continue };
        let gray = img.to_luma8();
        let size = [gray.width() as usize, gray.height() as usize];
        let color = egui::ColorImage::from_gray(size, gray.as_raw());
        st.films.push(PreviewFilm {
            name: f["name"].as_str().unwrap_or_default().to_string(),
            coverage: f["coverage"].as_f64().unwrap_or(0.0),
            texture: ctx.load_texture(format!("screen-print-film-{i}"), color, egui::TextureOptions::LINEAR),
        });
    }
    st.advice = result["advice"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|v| match v["ink"].as_str() {
                    Some(ink) => format!("{ink}: {}", v["message"].as_str().unwrap_or_default()),
                    None => v["message"].as_str().unwrap_or_default().to_string(),
                })
                .collect()
        })
        .unwrap_or_default();
    st.message = None;
}

/// Save Films…: pick a folder and write the films there at full resolution, with marks.
fn save(app: &mut VectorcraftApp, st: &mut ScreenPrintState) {
    let Some(folder) = crate::picks::folder(app) else { return };
    let (cmd, mut params) = st.request(SAVE_DPI, st.marks);
    params["dir"] = json!(folder);
    match run(app, cmd, params) {
        Ok(r) => {
            let n = r["films"].as_array().map_or(0, Vec::len);
            st.message = Some(format!("{n} → {folder}"));
            app.status(format!("{} {}", tl!("Films saved to:"), folder));
        }
        Err(e) => st.message = Some(e),
    }
}

pub fn show(app: &mut VectorcraftApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let ctx = ui.ctx().clone();
    let has_doc = app.session.active().is_some();
    let can_save = has_doc && crate::picks::can(app, &crate::picks::PickRequest::Folder);
    // Take the state out so the buttons below can run commands on `app` while it is edited.
    let mut st = std::mem::take(&mut app.ui.screen_print);
    let (mut find, mut snap, mut prev, mut save_clicked) = (false, false, false, false);

    ui.horizontal(|ui| {
        ui.label(RichText::new(tl!("Method")).color(t.text_dim));
        ui.selectable_value(&mut st.method, Method::Spot, tl!("Spot Colors"));
        ui.selectable_value(&mut st.method, Method::Simulated, tl!("Simulated Process"));
        ui.selectable_value(&mut st.method, Method::Index, tl!("Index Color"));
        ui.selectable_value(&mut st.method, Method::Vector, tl!("Vector Art"));
    });
    ui.horizontal(|ui| {
        // Vector art finds its own inks in the paths: no count to give, nothing to find.
        if st.method != Method::Vector {
            ui.label(RichText::new(tl!("Colors")).color(t.text_dim));
            ui.add(egui::DragValue::new(&mut st.colors).range(1..=MAX_COLORS));
            ui.add_enabled_ui(has_doc, |ui| find = widgets::secondary_button(ui, tl!("Find Colors")).clicked());
        }
        ui.add_enabled_ui(!st.inks.is_empty(), |ui| snap = widgets::secondary_button(ui, tl!("Snap to Library")).clicked());
    });

    // The ink list: a swatch, the colour, the name and the mesh of each screen.
    let mut remove = None;
    for (i, ink) in st.inks.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            let c = parse_hex(&ink.color).unwrap_or(Color32::GRAY);
            let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), egui::Sense::hover());
            ui.painter().rect_filled(r, 2.0, c);
            ui.add(egui::TextEdit::singleline(&mut ink.color).desired_width(64.0).font(egui::TextStyle::Monospace));
            ui.add(egui::TextEdit::singleline(&mut ink.name).desired_width(70.0));
            ui.label(RichText::new(tl!("Mesh")).color(t.text_dim));
            ui.add(egui::DragValue::new(&mut ink.mesh).range(1.0..=1000.0).speed(1.0));
            if widgets::icon_button(ui, "x", tl!("Remove"), false, 16.0).clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        st.inks.remove(i);
    }

    widgets::divider(ui);
    ui.checkbox(&mut st.underbase, tl!("Underbase"));
    match st.method {
        Method::Spot | Method::Vector => {
            ui.horizontal(|ui| {
                ui.label(RichText::new(tl!("Trap")).color(t.text_dim));
                ui.add(egui::DragValue::new(&mut st.trap_mm).range(0.0..=5.0).speed(0.05).suffix(" mm"));
            });
        }
        Method::Simulated => {
            ui.horizontal(|ui| {
                ui.label(RichText::new(tl!("Screening")).color(t.text_dim));
                ui.selectable_value(&mut st.screening, Screening::Am, tl!("AM"));
                ui.selectable_value(&mut st.screening, Screening::Fm, tl!("FM"));
                ui.selectable_value(&mut st.screening, Screening::Hybrid, tl!("Hybrid"));
            });
            if st.screening != Screening::Am {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tl!("Dot Size")).color(t.text_dim));
                    ui.add(egui::DragValue::new(&mut st.dot_mm).range(0.02..=5.0).speed(0.01).suffix(" mm"));
                });
            }
            if st.screening != Screening::Fm {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tl!("Line Screen")).color(t.text_dim));
                    ui.add(egui::DragValue::new(&mut st.lpi).range(5.0..=150.0).speed(0.5).suffix(" lpi"));
                });
            }
            if st.screening != Screening::Am {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tl!("Diffusion")).color(t.text_dim));
                    ui.selectable_value(&mut st.diffusion, Diffusion::Floyd, "Floyd-Steinberg");
                    ui.selectable_value(&mut st.diffusion, Diffusion::Jarvis, "Jarvis");
                    ui.selectable_value(&mut st.diffusion, Diffusion::Stucki, "Stucki");
                });
            }
            if st.screening == Screening::Hybrid {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tl!("FM Below")).color(t.text_dim));
                    ui.add(egui::DragValue::new(&mut st.hybrid_below_pct).range(0.0..=100.0).speed(0.5).suffix(" %"));
                    ui.label(RichText::new(tl!("FM Above")).color(t.text_dim));
                    ui.add(egui::DragValue::new(&mut st.hybrid_above_pct).range(0.0..=100.0).speed(0.5).suffix(" %"));
                });
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new(tl!("Dot Gain")).color(t.text_dim));
                ui.add(egui::DragValue::new(&mut st.dot_gain_pct).range(0.0..=25.0).speed(0.25).suffix(" %"));
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new(tl!("Ink Limit")).color(t.text_dim));
                ui.add(egui::DragValue::new(&mut st.ink_limit_pct).range(10.0..=100.0).speed(0.5).suffix(" %"));
            });
            ui.checkbox(&mut st.highlight_white, tl!("Highlight White"));
        }
        Method::Index => {
            ui.horizontal(|ui| {
                ui.label(RichText::new(tl!("Pixel Size")).color(t.text_dim));
                ui.add(egui::DragValue::new(&mut st.cell_mm).range(0.05..=5.0).speed(0.01).suffix(" mm"));
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new(tl!("Dither")).color(t.text_dim));
                ui.add(egui::DragValue::new(&mut st.dither_pct).range(0.0..=100.0).speed(1.0).suffix(" %"));
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new(tl!("Diffusion")).color(t.text_dim));
                ui.selectable_value(&mut st.diffusion, Diffusion::Floyd, "Floyd-Steinberg");
                ui.selectable_value(&mut st.diffusion, Diffusion::Jarvis, "Jarvis");
                ui.selectable_value(&mut st.diffusion, Diffusion::Stucki, "Stucki");
            });
        }
    }
    ui.checkbox(&mut st.marks, tl!("Registration Marks"));

    ui.horizontal(|ui| {
        ui.add_enabled_ui(has_doc, |ui| prev = widgets::secondary_button(ui, tl!("Preview")).clicked());
        ui.add_enabled_ui(can_save, |ui| save_clicked = widgets::primary_button_width(ui, tl!("Save Films…"), 96.0).clicked());
    });

    if find {
        find_colors(app, &mut st);
    }
    if snap {
        snap_to_library(app, &mut st);
    }
    if prev {
        preview(app, &ctx, &mut st);
    }
    if save_clicked {
        save(app, &mut st);
    }

    if let Some(m) = &st.message {
        ui.label(RichText::new(m).color(t.text_dim));
    }
    for line in &st.advice {
        ui.label(RichText::new(line).color(t.text_dim).size(11.5));
    }
    if !st.films.is_empty() {
        widgets::divider(ui);
        ui.horizontal_wrapped(|ui| {
            for f in &st.films {
                ui.vertical(|ui| {
                    let size = f.texture.size_vec2();
                    let k = THUMB / size.x.max(size.y).max(1.0);
                    let (r, _) = ui.allocate_exact_size(size * k, egui::Sense::hover());
                    ui.painter().rect_filled(r.expand(1.0), 0.0, t.border);
                    ui.painter().image(f.texture.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                    ui.label(RichText::new(&f.name).size(11.5).color(t.text));
                    ui.label(RichText::new(format!("{:.0}%", f.coverage * 100.0)).size(11.0).color(t.text_dim));
                });
            }
        });
    }
    app.ui.screen_print = st;
}

pub fn menu(app: &mut VectorcraftApp, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    if menu_item(ui, tl!("Refresh"), app.session.active().is_some(), false) {
        let mut st = std::mem::take(&mut app.ui.screen_print);
        preview(app, &ctx, &mut st);
        app.ui.screen_print = st;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn session_with_two_colors() -> VectorcraftApp {
        let mut app = VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        let mut run = |id: &str, p: Value| app.session.execute(id, &p).unwrap();
        run("file.new", json!({"width": 200, "height": 100}));
        for (x, fill) in [(0.0, "#fadc14"), (100.0, "#0a1458")] {
            let r = run("shape.rectangle", json!({"x": x, "y": 0, "width": 100, "height": 100}));
            run("select.set", json!({"ids": [r["id"].as_u64().unwrap()]}));
            run("paint.setFill", json!({"color": fill}));
        }
        app
    }

    #[test]
    fn the_request_follows_the_choices() {
        let mut st = ScreenPrintState { underbase: true, ..Default::default() };
        let (cmd, p) = st.request(100.0, false);
        assert_eq!(cmd, "screen.separate");
        assert_eq!(
            (p["colors"].clone(), p["trapMm"].clone(), p["underbase"].clone(), p["marks"].clone()),
            (json!(4), json!(0.25), json!(true), json!(false))
        );
        assert!(p.get("inks").is_none() && p.get("lpi").is_none());
        st.method = Method::Simulated;
        st.inks = vec![InkRow { name: "Navy".into(), color: "#0a1458".into(), mesh: 110.0 }];
        let (cmd, p) = st.request(600.0, true);
        assert_eq!(cmd, "screen.simulate");
        assert_eq!(p["inks"], json!([{"name": "Navy", "color": "#0a1458", "mesh": 110.0}]));
        assert_eq!((p["lpi"].clone(), p["dpi"].clone()), (json!(45.0), json!(600.0)));
        assert_eq!(p["dotGain"], json!(0.0));
        st.dot_gain_pct = 20.0;
        assert_eq!(st.request(600.0, true).1["dotGain"], json!(0.2));
        assert!(st.request(600.0, true).1.get("screening").is_none(), "AM is the default");
        assert_eq!(st.request(600.0, true).1["inkLimit"], json!(1.0));
        assert!(st.request(600.0, true).1.get("highlightWhite").is_none());
        st.highlight_white = true;
        assert_eq!(st.request(600.0, true).1["highlightWhite"], json!(true));
        st.highlight_white = false;
        st.ink_limit_pct = 80.0;
        assert_eq!(st.request(600.0, true).1["inkLimit"], json!(0.8));
        st.ink_limit_pct = 100.0;
        st.screening = Screening::Fm;
        st.dot_mm = 0.3;
        let (_, fm) = st.request(600.0, true);
        assert_eq!((fm["screening"].clone(), fm["dotMm"].clone()), (json!("fm"), json!(0.3)));
        assert!(fm.get("hybridBelow").is_none());
        assert_eq!(fm["fmKernel"], json!("floyd"));
        st.screening = Screening::Hybrid;
        let (_, hy) = st.request(600.0, true);
        assert_eq!(
            (hy["screening"].clone(), hy["hybridBelow"].clone(), hy["hybridAbove"].clone(), hy["lpi"].clone()),
            (json!("hybrid"), json!(0.15), json!(0.85), json!(45.0))
        );
        assert!(p.get("colors").is_none() && p.get("trapMm").is_none());
        st.method = Method::Index;
        let (cmd, p) = st.request(200.0, false);
        assert_eq!(cmd, "screen.index");
        assert_eq!((p["cellMm"].clone(), p["dither"].clone()), (json!(0.5), json!(1.0)));
        assert_eq!(p["kernel"], json!("floyd"));
        st.diffusion = Diffusion::Stucki;
        assert_eq!(st.request(200.0, false).1["kernel"], json!("stucki"));
        assert!(p.get("lpi").is_none() && p.get("dotGain").is_none() && p.get("trapMm").is_none());
        assert_eq!(st.preview_dpi(), 200.0);
        st.method = Method::Vector;
        st.inks.clear();
        let (cmd, p) = st.request(100.0, false);
        assert_eq!(cmd, "screen.separateVector");
        assert_eq!(p["trapMm"], json!(0.25));
        assert!(p.get("colors").is_none() && p.get("lpi").is_none() && p.get("cellMm").is_none(), "{p}");
        assert_eq!(st.preview_dpi(), 100.0);
    }

    #[test]
    fn find_colors_fills_the_list_and_preview_makes_a_thumbnail_a_film() {
        let mut app = session_with_two_colors();
        let ctx = egui::Context::default();
        let mut st = ScreenPrintState { colors: 2, ..Default::default() };
        find_colors(&mut app, &mut st);
        let colors: Vec<_> = st.inks.iter().map(|i| i.color.as_str()).collect();
        assert_eq!(st.inks.len(), 2, "{colors:?}");
        assert!(colors.contains(&"#fadc14") && colors.contains(&"#0a1458"), "{colors:?}");
        assert!(st.inks.iter().all(|i| i.mesh == 156.0));
        preview(&mut app, &ctx, &mut st);
        assert!(st.message.is_none(), "{:?}", st.message);
        assert_eq!(st.films.len(), 2);
        assert!(st.films.iter().all(|f| st.inks.iter().any(|i| i.name == f.name)), "each film is named after its ink");
        let total: f64 = st.films.iter().map(|f| f.coverage).sum();
        assert!((total - 1.0).abs() < 0.02, "the films partition the art: {total}");
        // A halftone preview is made fine enough to show its dots.
        let fine = ScreenPrintState { method: Method::Simulated, lpi: 45.0, ..Default::default() };
        assert_eq!(fine.preview_dpi(), 225.0);
        assert_eq!(ScreenPrintState { method: Method::Simulated, lpi: 2.0, ..Default::default() }.preview_dpi(), 100.0);
        assert_eq!(ScreenPrintState::default().preview_dpi(), 100.0);
        // Simulated process shows mesh advice for a too-fine screen.
        st.method = Method::Simulated;
        st.lpi = 100.0;
        for i in &mut st.inks {
            i.mesh = 110.0;
        }
        preview(&mut app, &ctx, &mut st);
        assert!(st.advice.iter().any(|a| a.contains("moiré") || a.contains("mesh")), "{:?}", st.advice);
    }

    #[test]
    fn snap_to_library_swaps_in_library_inks_and_keeps_renamed_ones() {
        let mut app = VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        let row = |name: &str, color: &str| InkRow { name: name.into(), color: color.into(), mesh: 156.0 };
        let mut st = ScreenPrintState {
            inks: vec![row("Ink 1", "#cd2328"), row("Hero Blue", "#1d4fa6"), row("Ink 3", "#d0222a"), row("Ink 4", "#00ff00")],
            ..Default::default()
        };
        snap_to_library(&mut app, &mut st);
        assert!(st.message.is_none(), "{:?}", st.message);
        let got: Vec<_> = st.inks.iter().map(|i| (i.name.as_str(), i.color.as_str())).collect();
        assert_eq!(
            got,
            [("Red", "#d21f26"), ("Hero Blue", "#1f4fa8"), ("Ink 3", "#d0222a"), ("Ink 4", "#00ff00")],
            "the second near-red and the far green keep their colours"
        );
        // A bad colour in the list is reported, not applied.
        st.inks = vec![row("Ink 1", "nope")];
        snap_to_library(&mut app, &mut st);
        assert!(st.message.is_some());
        assert_eq!(st.inks[0].color, "nope");
    }

    #[test]
    fn hex_colours_parse_or_are_refused() {
        assert_eq!(parse_hex("#fadc14"), Some(Color32::from_rgb(250, 220, 20)));
        assert_eq!(parse_hex("0a1458"), Some(Color32::from_rgb(10, 20, 88)));
        for bad in ["", "#", "#12345", "#1234567", "#gg0000", "é12345"] {
            assert_eq!(parse_hex(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn errors_show_in_the_panel_instead_of_failing() {
        let mut app = VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        let ctx = egui::Context::default();
        let mut st = ScreenPrintState::default();
        preview(&mut app, &ctx, &mut st);
        assert!(st.message.is_some() && st.films.is_empty(), "no document, no films: {:?}", st.message);
        find_colors(&mut app, &mut st);
        assert!(st.message.is_some() && st.inks.is_empty());
    }

    #[test]
    fn the_panel_draws_without_and_with_a_document() {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        let mut empty = VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        let mut app = session_with_two_colors();
        for app in [&mut empty, &mut app] {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| show(app, ui));
            out.textures_delta.clear();
        }
        assert_eq!(app.ui.screen_print.inks.len(), 0, "drawing alone changes nothing");
    }
}
