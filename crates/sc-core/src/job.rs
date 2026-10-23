//! A spot-colour job and the pipeline that turns artwork into one film per screen.
//!
//! Order of screens: the underbase (if any) first, then the colours from light to dark. Each
//! colour except the last one is spread by the trap so it tucks under its darker neighbour. The
//! light-to-dark order is a common convention, not a verified rule: it is the default, and the
//! job keeps the order explicit so a user can change it later.

use crate::color::srgb_to_lab;
use crate::error::{Result, param};
use crate::fm::{FmOptions, Hybrid, fm_screen, hybrid_screen};
use crate::halftone::{HalftoneOptions, halftone, solid};
use crate::image::{Bitmap1, Gray8, Rgba8};
use crate::index::index_films;
use crate::morph::{choke, spread, union};
use crate::separate::{ALPHA_CUTOFF, separate_spot};
use crate::simulated::simulated_plates;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ink {
    pub name: String,
    pub rgb: [u8; 3],
    /// Mesh count (threads per inch) of the screen this ink prints through.
    pub mesh: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    Underbase,
    Color,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnderbaseOptions {
    pub ink: Ink,
    /// How far the underbase is pulled in from the colours' edges, so white never peeks out.
    pub choke_px: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpotJob {
    pub inks: Vec<Ink>,
    pub underbase: Option<UnderbaseOptions>,
    /// Spread given to every colour that prints before a darker one.
    pub trap_px: f64,
}

/// The film for one screen.
#[derive(Clone, Debug, PartialEq)]
pub struct Film {
    pub name: String,
    pub ink_rgb: [u8; 3],
    pub mesh: f64,
    pub role: Role,
    pub bitmap: Bitmap1,
}

/// Inks at least this light (Lab L*) get no underbase: they are the underbase's own colour.
const WHITE_L: f64 = 95.0;

impl SpotJob {
    pub fn run(&self, img: &Rgba8) -> Result<Vec<Film>> {
        if self.inks.is_empty() {
            return Err(param("inks", "a job needs at least one ink"));
        }
        let rgbs: Vec<[u8; 3]> = self.inks.iter().map(|i| i.rgb).collect();
        let plates = separate_spot(img, &rgbs)?;

        // Print order: light to dark.
        let mut order: Vec<usize> = (0..self.inks.len()).collect();
        let lightness = |i: usize| self.inks.get(i).map_or(0.0, |k| srgb_to_lab(k.rgb)[0]);
        order.sort_by(|&a, &b| lightness(b).total_cmp(&lightness(a)));

        let mut films = Vec::with_capacity(self.inks.len() + 1);
        if let Some(ub) = &self.underbase {
            let covered: Vec<_> = order.iter().filter(|&&i| lightness(i) < WHITE_L).filter_map(|&i| plates.get(i)).collect();
            if !covered.is_empty() {
                let footprint = union(&covered)?;
                let plate = choke(&footprint, ub.choke_px)?;
                films.push(Film { name: ub.ink.name.clone(), ink_rgb: ub.ink.rgb, mesh: ub.ink.mesh, role: Role::Underbase, bitmap: solid(&plate)? });
            }
        }
        for (pos, &i) in order.iter().enumerate() {
            let (Some(ink), Some(plate)) = (self.inks.get(i), plates.get(i)) else {
                return Err(param("inks", "plate/ink mismatch"));
            };
            let last = pos + 1 == order.len();
            let plate = if last { plate.clone() } else { spread(plate, self.trap_px)? };
            films.push(Film { name: ink.name.clone(), ink_rgb: ink.rgb, mesh: ink.mesh, role: Role::Color, bitmap: solid(&plate)? });
        }
        Ok(films)
    }
}

/// The underbase film of full-colour art: the artwork's opaque area, pulled in so the base never
/// shows past the dots.
fn footprint_underbase(img: &Rgba8, ub: &UnderbaseOptions) -> Result<Film> {
    let opaque: Vec<u8> = img.pixels().map(|p| if p[3] >= ALPHA_CUTOFF { 255 } else { 0 }).collect();
    let footprint = Gray8::new(img.width(), img.height(), opaque)?;
    Ok(Film {
        name: ub.ink.name.clone(),
        ink_rgb: ub.ink.rgb,
        mesh: ub.ink.mesh,
        role: Role::Underbase,
        bitmap: solid(&choke(&footprint, ub.choke_px)?)?,
    })
}

/// Screen angles for the inks of a simulated-process job, in order. Practitioner advice favours
/// 22.5 and 67.5 degrees for two colours and keeps clear of 0 and 90, which line up with the mesh
/// threads; the rest spread the other inks apart. A starting point, not a standard.
pub const DEFAULT_ANGLES: [f64; 8] = [22.5, 67.5, 45.0, 15.0, 75.0, 37.5, 52.5, 7.5];

/// Full-colour art as halftone dots of a few spot inks over a base (simulated process).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SimulatedJob {
    pub inks: Vec<Ink>,
    /// The colour under the dots: a white underbase, or the garment when there is none.
    pub base: [u8; 3],
    pub underbase: Option<UnderbaseOptions>,
    /// The screen angle of each ink in order (reused, in turn, when there are more inks).
    pub angles: Vec<f64>,
    /// Resolution, ruling, dot shape and dot limits; its `angle_deg` is replaced per ink.
    pub halftone: HalftoneOptions,
    /// Screen every ink as scattered equal dots (FM) instead of angled dots (AM): the ruling,
    /// angles and shape then do not apply.
    #[serde(default)]
    pub fm: Option<FmOptions>,
    /// With `fm` also set: angled dots in the middle tones and scattered ones only at the light
    /// and dark ends (a hybrid screen).
    #[serde(default)]
    pub hybrid: Option<Hybrid>,
}

impl SimulatedJob {
    pub fn run(&self, img: &Rgba8) -> Result<Vec<Film>> {
        if self.inks.is_empty() {
            return Err(param("inks", "a job needs at least one ink"));
        }
        let rgbs: Vec<[u8; 3]> = self.inks.iter().map(|i| i.rgb).collect();
        let plates = simulated_plates(img, &rgbs, self.base)?;
        let angles: &[f64] = if self.angles.is_empty() { &DEFAULT_ANGLES } else { &self.angles };

        let lightness = |i: usize| self.inks.get(i).map_or(0.0, |k| srgb_to_lab(k.rgb)[0]);
        let mut order: Vec<usize> = (0..self.inks.len()).collect();
        order.sort_by(|&a, &b| lightness(b).total_cmp(&lightness(a)));

        let mut films = Vec::with_capacity(self.inks.len() + 1);
        if let Some(ub) = &self.underbase {
            films.push(footprint_underbase(img, ub)?);
        }
        for &i in &order {
            let (Some(ink), Some(plate)) = (self.inks.get(i), plates.get(i)) else {
                return Err(param("inks", "plate/ink mismatch"));
            };
            let angle = angles.get(i % angles.len()).copied().unwrap_or(22.5);
            let am = HalftoneOptions { angle_deg: angle, ..self.halftone };
            let bitmap = match (&self.fm, &self.hybrid) {
                (Some(fm), Some(h)) => hybrid_screen(plate, &am, fm, h)?,
                (Some(fm), None) => fm_screen(plate, fm)?,
                _ => halftone(plate, &am)?,
            };
            films.push(Film { name: ink.name.clone(), ink_rgb: ink.rgb, mesh: ink.mesh, role: Role::Color, bitmap });
        }
        Ok(films)
    }
}

/// Full-colour art reduced to a fixed palette of inks, one ink per square pixel (index colour).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IndexJob {
    pub inks: Vec<Ink>,
    /// The colour under the pixels: a white underbase, or the garment when there is none.
    pub base: [u8; 3],
    pub underbase: Option<UnderbaseOptions>,
    /// Side of one printed pixel, in film pixels.
    pub cell_px: usize,
    /// How much of each pixel's error spreads to its neighbours: 0 (flat) to 1 (full diffusion).
    pub dither: f64,
}

impl IndexJob {
    pub fn run(&self, img: &Rgba8) -> Result<Vec<Film>> {
        if self.inks.is_empty() {
            return Err(param("inks", "a job needs at least one ink"));
        }
        let rgbs: Vec<[u8; 3]> = self.inks.iter().map(|i| i.rgb).collect();
        let bitmaps = index_films(img, &rgbs, self.base, self.cell_px, self.dither)?;

        let lightness = |i: usize| self.inks.get(i).map_or(0.0, |k| srgb_to_lab(k.rgb)[0]);
        let mut order: Vec<usize> = (0..self.inks.len()).collect();
        order.sort_by(|&a, &b| lightness(b).total_cmp(&lightness(a)));

        let mut films = Vec::with_capacity(self.inks.len() + 1);
        if let Some(ub) = &self.underbase {
            films.push(footprint_underbase(img, ub)?);
        }
        for &i in &order {
            let (Some(ink), Some(bitmap)) = (self.inks.get(i), bitmaps.get(i)) else {
                return Err(param("inks", "film/ink mismatch"));
            };
            films.push(Film { name: ink.name.clone(), ink_rgb: ink.rgb, mesh: ink.mesh, role: Role::Color, bitmap: bitmap.clone() });
        }
        Ok(films)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ink(name: &str, rgb: [u8; 3]) -> Ink {
        Ink { name: name.into(), rgb, mesh: 156.0 }
    }

    /// Left third yellow, middle third navy, right third transparent.
    fn art() -> Rgba8 {
        let (w, h) = (60, 20);
        let mut d = Vec::new();
        for _ in 0..h {
            for x in 0..w {
                let p = if x < 20 {
                    [250, 220, 20, 255]
                } else if x < 40 {
                    [10, 20, 90, 255]
                } else {
                    [0, 0, 0, 0]
                };
                d.extend_from_slice(&p);
            }
        }
        Rgba8::new(w, h, d).unwrap()
    }

    fn job(trap: f64) -> SpotJob {
        SpotJob {
            inks: vec![ink("Navy", [10, 20, 90]), ink("Yellow", [250, 220, 20])],
            underbase: Some(UnderbaseOptions { ink: ink("White", [255, 255, 255]), choke_px: 2.0 }),
            trap_px: trap,
        }
    }

    #[test]
    fn screens_come_out_underbase_then_light_to_dark() {
        let films = job(1.0).run(&art()).unwrap();
        let names: Vec<_> = films.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["White", "Yellow", "Navy"]);
        assert_eq!(films[0].role, Role::Underbase);
    }

    #[test]
    fn underbase_is_the_artwork_footprint_pulled_in() {
        let films = job(0.0).run(&art()).unwrap();
        let ub = &films[0].bitmap;
        assert!(ub.get(10, 10) && ub.get(30, 10));
        assert!(!ub.get(50, 10), "nothing under the transparent area");
        assert!(!ub.get(0, 10) && !ub.get(1, 10), "choked in from the left edge");
        assert!(!ub.get(39, 10), "choked in from the right edge");
        assert!(!ub.get(10, 0), "choked in from the top");
    }

    #[test]
    fn lighter_colour_is_trapped_under_the_darker_one() {
        let films = job(2.0).run(&art()).unwrap();
        let (yellow, navy) = (&films[1].bitmap, &films[2].bitmap);
        assert!(yellow.get(20, 10) && yellow.get(21, 10), "yellow spreads into navy");
        assert!(!yellow.get(22, 10));
        assert!(navy.get(20, 10) && !navy.get(19, 10), "the darkest colour is not spread");
    }

    #[test]
    fn white_ink_gets_no_underbase_and_all_white_art_has_none() {
        let img = Rgba8::new(4, 4, vec![255; 64]).unwrap();
        let job = SpotJob {
            inks: vec![ink("White", [255, 255, 255])],
            underbase: Some(UnderbaseOptions { ink: ink("White", [255, 255, 255]), choke_px: 1.0 }),
            trap_px: 0.0,
        };
        let films = job.run(&img).unwrap();
        assert_eq!(films.len(), 1);
        assert_eq!(films[0].role, Role::Color);
    }

    #[test]
    fn empty_ink_list_and_bad_trap_are_errors() {
        let mut j = job(1.0);
        j.trap_px = f64::NAN;
        assert!(j.run(&art()).is_err());
        j.inks.clear();
        assert!(j.run(&art()).is_err());
    }

    fn sim_job(underbase: bool) -> SimulatedJob {
        SimulatedJob {
            inks: vec![ink("Navy", [10, 20, 90]), ink("Red", [200, 30, 30])],
            base: [255, 255, 255],
            underbase: underbase.then(|| UnderbaseOptions { ink: ink("White", [255, 255, 255]), choke_px: 1.0 }),
            angles: vec![22.5, 67.5],
            halftone: HalftoneOptions {
                dpi: 600.0,
                lpi: 50.0,
                angle_deg: 0.0,
                shape: crate::halftone::DotShape::Round,
                min_dot: 0.0,
                max_dot: 1.0,
                dot_gain: 0.0,
            },
            fm: None,
            hybrid: None,
        }
    }

    /// 120 × 120 pixels of one colour that is half navy, half red, in linear light.
    fn half_and_half() -> Rgba8 {
        let (a, b) = (crate::simulated::to_linear([10, 20, 90]), crate::simulated::to_linear([200, 30, 30]));
        let enc = |v: f64| -> u8 {
            let s = if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
            (s * 255.0).round().clamp(0.0, 255.0) as u8
        };
        let px = [enc((a[0] + b[0]) / 2.0), enc((a[1] + b[1]) / 2.0), enc((a[2] + b[2]) / 2.0), 255];
        Rgba8::new(120, 120, px.repeat(120 * 120)).unwrap()
    }

    #[test]
    fn simulated_process_prints_each_ink_as_dots_at_its_own_angle() {
        let films = sim_job(false).run(&half_and_half()).unwrap();
        let names: Vec<_> = films.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["Red", "Navy"], "light to dark, no underbase asked for");
        // Each ink covers about half the area, and the two films are different patterns.
        for f in &films {
            assert!((f.bitmap.coverage() - 0.5).abs() < 0.04, "{}: {}", f.name, f.bitmap.coverage());
        }
        assert_ne!(films[0].bitmap, films[1].bitmap, "different angles");
    }

    #[test]
    fn simulated_process_underbase_covers_the_opaque_area_only() {
        let mut d = half_and_half().data().to_vec();
        for px in d.chunks_exact_mut(4).take(120 * 20) {
            px[3] = 0; // the top 20 rows are background
        }
        let img = Rgba8::new(120, 120, d).unwrap();
        let films = sim_job(true).run(&img).unwrap();
        assert_eq!(films[0].role, Role::Underbase);
        let ub = &films[0].bitmap;
        assert!(ub.get(60, 70) && !ub.get(60, 5), "under the art, not under the background");
        assert!(!ub.get(60, 20), "choked in from the edge");
    }

    #[test]
    fn simulated_process_rejects_bad_jobs() {
        let mut j = sim_job(false);
        j.inks.clear();
        assert!(j.run(&half_and_half()).is_err());
        let mut j = sim_job(false);
        j.halftone.dpi = f64::NAN;
        assert!(j.run(&half_and_half()).is_err());
        let json = serde_json::to_string(&sim_job(true)).unwrap();
        assert_eq!(serde_json::from_str::<SimulatedJob>(&json).unwrap(), sim_job(true));
    }

    #[test]
    fn job_round_trips_through_json() {
        let j = job(1.5);
        let s = serde_json::to_string(&j).unwrap();
        assert_eq!(serde_json::from_str::<SpotJob>(&s).unwrap(), j);
    }

    #[test]
    fn index_job_orders_the_screens_and_keeps_them_apart() {
        let job = IndexJob {
            inks: vec![ink("Navy", [10, 20, 90]), ink("Yellow", [250, 220, 20])],
            base: [255, 255, 255],
            underbase: Some(UnderbaseOptions { ink: ink("White", [255, 255, 255]), choke_px: 2.0 }),
            cell_px: 2,
            dither: 1.0,
        };
        let films = job.run(&art()).unwrap();
        let names: Vec<_> = films.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["White", "Yellow", "Navy"]);
        let (yellow, navy) = (&films[1].bitmap, &films[2].bitmap);
        assert!(yellow.get(10, 10) && !navy.get(10, 10), "the yellow third prints yellow only");
        assert!(navy.get(30, 10) && !yellow.get(30, 10));
        assert!(!yellow.get(50, 10) && !navy.get(50, 10), "nothing in the transparent area");
        assert!(films[0].bitmap.get(10, 10) && !films[0].bitmap.get(50, 10));
        let bad = IndexJob { inks: Vec::new(), ..job };
        assert!(bad.run(&art()).is_err());
    }

    #[test]
    fn an_fm_job_screens_every_ink_as_scattered_whole_cells_at_the_tone() {
        let fm = FmOptions { cell_px: 4, min_dot: 0.0, max_dot: 1.0, dot_gain: 0.0, kernel: Default::default() };
        let films = SimulatedJob { fm: Some(fm), ..sim_job(false) }.run(&half_and_half()).unwrap();
        assert_eq!(films.len(), 2);
        for f in &films {
            assert!((f.bitmap.coverage() - 0.5).abs() < 0.06, "{}: {}", f.name, f.bitmap.coverage());
            for y in 0..120 {
                for x in 0..120 {
                    assert_eq!(f.bitmap.get(x, y), f.bitmap.get(x - x % 4, y - y % 4), "{} ({x}, {y})", f.name);
                }
            }
        }
        assert!(films[0].bitmap.coverage() > 0.0);
        let am = sim_job(false).run(&half_and_half()).unwrap();
        assert_ne!(am[0].bitmap, films[0].bitmap, "a different screening");
    }

    #[test]
    fn a_hybrid_job_angles_the_middle_and_scatters_the_ends() {
        let fm = FmOptions { cell_px: 4, min_dot: 0.0, max_dot: 1.0, dot_gain: 0.0, kernel: Default::default() };
        let hybrid = Hybrid { below: 0.1, above: 0.9 };
        // Half-tone art is a middle tone: the hybrid is the angled screen there, not the scattered one.
        let films = SimulatedJob { fm: Some(fm), hybrid: Some(hybrid), ..sim_job(false) }.run(&half_and_half()).unwrap();
        let am = sim_job(false).run(&half_and_half()).unwrap();
        assert_eq!(films.len(), 2);
        for (f, a) in films.iter().zip(&am) {
            assert_eq!(f.bitmap, a.bitmap, "{}: a middle tone stays angled", f.name);
        }
        let fm_only = SimulatedJob { fm: Some(fm), ..sim_job(false) }.run(&half_and_half()).unwrap();
        assert_ne!(films[0].bitmap, fm_only[0].bitmap);
        // The setting is part of the saved job and reads back, and an old job without it still loads.
        let job = SimulatedJob { fm: Some(fm), hybrid: Some(hybrid), ..sim_job(false) };
        let back: SimulatedJob = serde_json::from_str(&serde_json::to_string(&job).unwrap()).unwrap();
        assert_eq!(back, job);
        let mut v = serde_json::to_value(&job).unwrap();
        v.as_object_mut().unwrap().remove("hybrid");
        v.as_object_mut().unwrap().remove("fm");
        let old: SimulatedJob = serde_json::from_value(v).unwrap();
        assert_eq!((old.fm, old.hybrid), (None, None));
    }
}
