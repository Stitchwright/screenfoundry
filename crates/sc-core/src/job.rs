//! A spot-colour job and the pipeline that turns artwork into one film per screen.
//!
//! Order of screens: the underbase (if any) first, then the colours from light to dark. Each
//! colour except the last one is spread by the trap so it tucks under its darker neighbour. The
//! light-to-dark order is a common convention, not a verified rule: it is the default, and the
//! job keeps the order explicit so a user can change it later.

use crate::color::srgb_to_lab;
use crate::error::{Result, param};
use crate::halftone::solid;
use crate::image::{Bitmap1, Rgba8};
use crate::morph::{choke, spread, union};
use crate::separate::separate_spot;
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

    #[test]
    fn job_round_trips_through_json() {
        let j = job(1.5);
        let s = serde_json::to_string(&j).unwrap();
        assert_eq!(serde_json::from_str::<SpotJob>(&s).unwrap(), j);
    }
}
