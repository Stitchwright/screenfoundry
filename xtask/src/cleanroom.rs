//! `cargo xtask cleanroom`: no tracked file may carry an Adobe copyright notice or name an Adobe
//! application as the program that made it, except the files in [`ALLOWED`].
//!
//! The clean-room rule (AGENTS.md › Non-negotiables) says no Adobe-authored code, text or sample
//! files come into this repository, and no fixture is made with an Adobe application. This check
//! catches the two marks such files usually carry:
//! - a copyright notice naming Adobe (`Copyright … Adobe`, `© Adobe`, `(c) Adobe`,
//!   `Adobe Systems Incorporated`), as procsets, presets, help text and sample files have;
//! - a creator stamp naming an Adobe application (`%%Creator: Adobe …` in PostScript/EPS,
//!   `/Creator (Adobe …)` or `/Producer (Adobe …)` in PDF, `CreatorTool>Adobe …` in XMP), as a
//!   file saved by one has.
//!
//! Every file is read as bytes (binary fixtures too), compared without regard to ASCII case. Hits
//! in [`ALLOWED`] are known and legitimate; adding to that list is the owner's decision.

use std::path::Path;

/// Files allowed to match, with why. A new entry needs the project owner's agreement.
const ALLOWED: &[(&str, &str)] = &[
    ("NOTICE", "third-party notices: Adobe's BSD-licensed predefined CMaps (via hayro-cmap) and the OFL Source fonts"),
    ("assets/fonts/OFL-SourceSans3.txt", "the SIL Open Font License text of Source Sans 3, which Adobe publishes under the OFL"),
    ("assets/fonts/OFL-SourceSerif4.txt", "the SIL Open Font License text of Source Serif 4, which Adobe publishes under the OFL"),
    ("licenses/pdf-backend/hayro-cmap/LICENSE.txt", "the BSD-3-Clause licence of the predefined CMaps a PDF dependency bundles"),
    ("crates/eps/src/import/tests_illustrator.rs", "hand-written import tests: a one-line `%%Creator:` comment written from the DSC specification"),
    (
        "crates/engine/src/cmd/fileio/tests_epsimport.rs",
        "hand-written import tests: a one-line `%%Creator:` comment written from the DSC specification",
    ),
    ("crates/engine/tests/import_fuzz.rs", "hand-written fuzz input: a one-line `%%Creator:` comment written from the DSC specification"),
    ("xtask/src/cleanroom.rs", "this check: its own patterns and tests"),
];

/// The most bytes of years, punctuation and `(c)` / `©` between a copyright key and "adobe".
const NOTICE_REACH: usize = 40;
/// The most bytes of spacing and delimiters between a creator key and its value "adobe".
const CREATOR_REACH: usize = 8;

const NOTICE: &str = "an Adobe copyright notice";
const CREATOR: &str = "a creator stamp naming an Adobe application";

/// The first mark `text` carries, as a short description, or `None`.
pub fn mark(text: &[u8]) -> Option<&'static str> {
    let lower = text.to_ascii_lowercase();
    find(&lower, b"adobe")?;
    if find(&lower, b"adobe systems incorporated").is_some() {
        return Some(NOTICE);
    }
    // a notice's holder follows its key with only years and punctuation between:
    // `Copyright 1990-2023 Adobe`, `Copyright (c) 2001, 2010 Adobe`, `© 2020 Adobe`
    let in_notice = |rest: &[u8]| {
        // `(c)` and `©` (0xc2 0xa9 in UTF-8, 0xa9 in Latin-1) may sit among the years
        rest.iter()
            .take(NOTICE_REACH)
            .take_while(|b| b.is_ascii_digit() || matches!(b, b' ' | b'\t' | b',' | b'-' | b'.' | b':' | b'(' | b'c' | b')' | 0xc2 | 0xa9))
            .count()
    };
    for key in [&b"copyright"[..], "\u{a9}".as_bytes(), b"(c)"] {
        if followed_by_adobe(&lower, key, in_notice) {
            return Some(NOTICE);
        }
    }
    // a creator stamp's value is Adobe's: `%%Creator: Adobe`, `/Creator (Adobe`, `CreatorTool>Adobe`
    let in_stamp =
        |rest: &[u8]| rest.iter().take(CREATOR_REACH).take_while(|b| matches!(b, b' ' | b'\t' | b':' | b'(' | b'>' | b'=' | b'"' | b'\'')).count();
    for key in [&b"%%creator:"[..], b"/creator", b"/producer", b"creatortool"] {
        if followed_by_adobe(&lower, key, in_stamp) {
            return Some(CREATOR);
        }
    }
    None
}

/// Whether some `key` in `hay` is followed, after the `skip` bytes it allows, by "adobe".
fn followed_by_adobe(hay: &[u8], key: &[u8], skip: impl Fn(&[u8]) -> usize) -> bool {
    let mut from = 0;
    while let Some(at) = hay.get(from..).and_then(|rest| find(rest, key)) {
        let start = from + at + key.len();
        let rest = hay.get(start..).unwrap_or_default();
        if rest.get(skip(rest)..).is_some_and(|r| r.starts_with(b"adobe")) {
            return true;
        }
        from = start;
    }
    false
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Whether `path` is in [`ALLOWED`].
fn allowed(path: &str) -> bool {
    ALLOWED.iter().any(|(allowed, _)| *allowed == path)
}

/// The files (not in [`ALLOWED`]) that carry a mark, with the mark.
pub fn offenders<'a>(files: impl IntoIterator<Item = (String, &'a [u8])>) -> Vec<(String, &'static str)> {
    files.into_iter().filter(|(path, _)| !allowed(path)).filter_map(|(path, bytes)| mark(bytes).map(|m| (path, m))).collect()
}

pub fn run(root: &Path) -> Result<(), String> {
    let paths = crate::repo_files(root)?;
    let mut bad = Vec::new();
    for path in paths.iter().filter(|p| !allowed(p)) {
        let bytes = std::fs::read(root.join(path)).map_err(|e| format!("{path}: {e}"))?;
        bad.extend(offenders([(path.clone(), bytes.as_slice())]).into_iter().map(|(p, m)| format!("{p}: {m}")));
    }
    if bad.is_empty() {
        println!("cleanroom: {} files checked, no Adobe copyright notices or creator stamps ({} allowed files skipped)", paths.len(), ALLOWED.len());
        Ok(())
    } else {
        Err(format!(
            "{} file(s) carry an Adobe mark (clean-room rule, AGENTS.md › Non-negotiables). Remove the file, or ask the project owner to add it to ALLOWED in xtask/src/cleanroom.rs:\n  {}",
            bad.len(),
            bad.join("\n  ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copyright_notices_naming_adobe_are_found() {
        for text in [
            "% Copyright 1997 Adobe Systems Incorporated.",
            "Copyright (c) 2001-2010 Adobe. All rights reserved.",
            "\u{a9} Adobe 2020",
            "(C) ADOBE",
            "copyright 2019, 2020 adobe inc.",
            "Adobe Systems Incorporated",
        ] {
            assert_eq!(mark(text.as_bytes()), Some("an Adobe copyright notice"), "{text:?}");
        }
    }

    #[test]
    fn creator_stamps_naming_adobe_are_found() {
        for text in [
            "%!PS-Adobe-3.0 EPSF-3.0\n%%Creator: Adobe Illustrator(R) 24.0\n",
            "%%Creator:Adobe Photoshop",
            "<< /Creator (Adobe InDesign 18.0) /Producer (Adobe PDF Library 17.0) >>",
            "<xmp:CreatorTool>Adobe Illustrator 28.0 (Macintosh)</xmp:CreatorTool>",
        ] {
            assert_eq!(mark(text.as_bytes()), Some("a creator stamp naming an Adobe application"), "{text:?}");
        }
        // binary around the stamp, as in a TIFF or PSD fixture
        let mut bytes = vec![0u8, 0xff, 0x13, 0x88];
        bytes.extend_from_slice(b"<xmp:CreatorTool>Adobe Photoshop</xmp:CreatorTool>");
        bytes.extend_from_slice(&[0, 1, 2]);
        assert!(mark(&bytes).is_some());
    }

    #[test]
    fn plain_mentions_are_not_marks() {
        for text in [
            // the PostScript and EPSF headers name Adobe's specifications, not a maker
            "%!PS-Adobe-3.0 EPSF-3.0\n%%Creator: ScreenFoundry\n",
            "Never use Adobe software.",
            "Adobe copyright notices make this check fail.",
            "fails on Adobe copyright notices and on fixtures made with Adobe applications",
            "Copyright 2026 the ScreenFoundry contributors; formats from Adobe's public specifications",
            "%%Creator: ScreenFoundry (reads files from Adobe Illustrator)",
            "Copyright 2026 the ScreenFoundry contributors\nAdobe RGB (1998) is a colour space name.",
            "%%Creator: ScreenFoundry\n%%Title: not by Adobe",
            "/Creator (ScreenFoundry) /Title (an Adobe-free file)",
            "copyright",
            "",
        ] {
            assert_eq!(mark(text.as_bytes()), None, "{text:?}");
        }
    }

    #[test]
    fn allowed_files_are_skipped_and_others_reported() {
        let notice = b"Copyright 1990-2023 Adobe. All rights reserved.".to_vec();
        let files = [
            ("NOTICE".to_string(), notice.clone()),
            ("crates/x/tests/fixture.eps".to_string(), b"%%Creator: Adobe Illustrator(R) 8.0".to_vec()),
            ("docs/prologs/agm.ps".to_string(), notice),
            ("crates/x/src/lib.rs".to_string(), b"// Adobe RGB".to_vec()),
        ];
        assert_eq!(
            offenders(files.iter().map(|(p, b)| (p.clone(), b.as_slice()))),
            vec![
                ("crates/x/tests/fixture.eps".to_string(), "a creator stamp naming an Adobe application"),
                ("docs/prologs/agm.ps".to_string(), "an Adobe copyright notice"),
            ]
        );
    }

    #[test]
    fn every_allowed_file_exists_and_still_matches() {
        // a stale entry would let a new file at that path in unchecked
        for (path, _) in ALLOWED {
            let bytes = std::fs::read(crate::root().join(path)).unwrap_or_else(|e| panic!("{path}: {e}"));
            assert!(mark(&bytes).is_some(), "{path} no longer carries a mark: drop it from ALLOWED");
        }
    }

    #[test]
    fn every_allowed_entry_says_why() {
        for (path, why) in ALLOWED {
            assert!(!path.is_empty() && !why.trim().is_empty(), "{path}");
        }
    }
}
