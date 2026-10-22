//! A small library of generic screen-printing inks and nearest-ink matching.
//!
//! The inks are original, plainly named colours (no brand or swatch-book names or data): their
//! sRGB values are on-screen starting points chosen for this crate, not measurements of any ink.
//! A shop's own inks go in as extra library entries; matching is by CIE76 ΔE in Lab, the same
//! space the separation uses.

use crate::color::{delta_e_sq, srgb_to_lab};

/// One ink the shop can mix or buy.
#[derive(Clone, Debug, PartialEq)]
pub struct LibraryInk {
    pub name: String,
    pub rgb: [u8; 3],
    /// A loose grouping for pickers: neutral, red, orange, yellow, green, blue, purple, brown or metallic.
    pub family: String,
}

/// The closest library ink to a colour and how far off it is (CIE76 ΔE).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Match<'a> {
    pub ink: &'a LibraryInk,
    pub delta_e: f64,
}

/// A colour within this ΔE counts as "the same ink" unless the caller says otherwise. Around 25 is
/// a clearly visible but same-family difference, enough to catch "my red is the shop's red".
pub const DEFAULT_MAX_DELTA_E: f64 = 25.0;

const BUILTIN: &[(&str, [u8; 3], &str)] = &[
    ("White", [255, 255, 255], "neutral"),
    ("Off-White", [242, 236, 224], "neutral"),
    ("Light Gray", [200, 200, 200], "neutral"),
    ("Gray", [128, 128, 128], "neutral"),
    ("Charcoal", [60, 60, 60], "neutral"),
    ("Black", [17, 17, 17], "neutral"),
    ("Red", [210, 31, 38], "red"),
    ("Scarlet", [238, 59, 47], "red"),
    ("Burgundy", [106, 27, 42], "red"),
    ("Coral", [242, 107, 94], "red"),
    ("Pink", [242, 161, 189], "red"),
    ("Hot Pink", [229, 57, 138], "red"),
    ("Orange", [242, 138, 28], "orange"),
    ("Safety Orange", [255, 106, 19], "orange"),
    ("Gold", [240, 179, 35], "yellow"),
    ("Yellow", [247, 212, 23], "yellow"),
    ("Lemon", [251, 233, 74], "yellow"),
    ("Lime", [140, 198, 62], "green"),
    ("Kelly Green", [31, 157, 85], "green"),
    ("Forest Green", [29, 90, 56], "green"),
    ("Mint", [159, 216, 185], "green"),
    ("Olive", [107, 107, 42], "green"),
    ("Teal", [15, 139, 141], "blue"),
    ("Turquoise", [44, 193, 201], "blue"),
    ("Sky Blue", [111, 180, 230], "blue"),
    ("Royal Blue", [31, 79, 168], "blue"),
    ("Navy", [20, 38, 79], "blue"),
    ("Lavender", [183, 162, 214], "purple"),
    ("Purple", [107, 58, 143], "purple"),
    ("Magenta", [194, 24, 91], "purple"),
    ("Tan", [201, 167, 124], "brown"),
    ("Brown", [91, 58, 36], "brown"),
    ("Silver", [185, 188, 192], "metallic"),
    ("Metallic Gold", [201, 162, 75], "metallic"),
];

/// The built-in inks.
pub fn builtin() -> Vec<LibraryInk> {
    BUILTIN.iter().map(|&(name, rgb, family)| LibraryInk { name: name.into(), rgb, family: family.into() }).collect()
}

/// The library ink nearest to `rgb`, whatever the distance; `None` for an empty library. The first
/// of equally near inks wins, so put the shop's own inks before the built-ins.
pub fn nearest(library: &[LibraryInk], rgb: [u8; 3]) -> Option<Match<'_>> {
    let lab = srgb_to_lab(rgb);
    library
        .iter()
        .map(|ink| (ink, delta_e_sq(lab, srgb_to_lab(ink.rgb))))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(ink, d2)| Match { ink, delta_e: d2.sqrt() })
}

/// [`nearest`] when it is within `max_delta_e`.
pub fn snap(library: &[LibraryInk], rgb: [u8; 3], max_delta_e: f64) -> Option<Match<'_>> {
    nearest(library, rgb).filter(|m| m.delta_e <= max_delta_e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_builtin_library_is_sane() {
        let lib = builtin();
        assert!(lib.len() >= 30);
        let mut names: Vec<&str> = lib.iter().map(|i| i.name.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), lib.len(), "names are unique");
        // Each ink is its own nearest ink at distance zero, so no two inks share a colour.
        for ink in &lib {
            let m = nearest(&lib, ink.rgb).unwrap();
            assert_eq!((m.ink.name.as_str(), m.delta_e), (ink.name.as_str(), 0.0));
        }
        for fam in ["neutral", "red", "orange", "yellow", "green", "blue", "purple", "brown", "metallic"] {
            assert!(lib.iter().any(|i| i.family == fam), "{fam}");
        }
    }

    #[test]
    fn a_near_colour_snaps_to_its_ink_and_a_far_one_does_not() {
        let lib = builtin();
        let m = snap(&lib, [205, 35, 40], DEFAULT_MAX_DELTA_E).unwrap();
        assert_eq!(m.ink.name, "Red");
        assert!(m.delta_e > 0.0 && m.delta_e < 10.0, "{}", m.delta_e);
        // Pure green is nowhere near any ink within 3 ΔE.
        assert!(snap(&lib, [0, 255, 0], 3.0).is_none());
        assert!(nearest(&lib, [0, 255, 0]).is_some());
    }

    #[test]
    fn the_shops_own_ink_wins_a_tie_and_an_empty_library_matches_nothing() {
        let mut lib = vec![LibraryInk { name: "Shop Red".into(), rgb: [210, 31, 38], family: "red".into() }];
        lib.extend(builtin());
        assert_eq!(nearest(&lib, [210, 31, 38]).unwrap().ink.name, "Shop Red");
        assert!(nearest(&[], [1, 2, 3]).is_none());
        assert!(snap(&[], [1, 2, 3], 1e9).is_none());
    }
}
