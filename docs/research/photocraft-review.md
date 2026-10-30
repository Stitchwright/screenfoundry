# PhotoCraft review for reuse (2026-10-29)

Status: reviewed `storytold/photocraft` 0.11.1 (commit 04d0212, MIT OR Apache-2.0) against what
ScreenFoundry (`crates/sc-core`) already does. Nothing was copied; this lists what is there, what
overlaps and what would be worth adopting later. Any code taken from it needs a line in `NOTICE`.

| PhotoCraft | What it has | ScreenFoundry today | Verdict |
|---|---|---|---|
| `algo/quantize.rs` | Palette builders (median cut and others), Bayer 8×8, Floyd–Steinberg style diffusion, bitmap conversion (threshold, pattern, diffusion), halftone spot shapes (round, ellipse, line, square, diamond, cross) | `sc-core`: median-cut colour reduction, ordered dither, error-diffusion kernels, FM and hybrid screening, dot shapes and curves, all tied to plates, dpi and lpi | Overlaps. Ours is richer for printing; no gain in copying. |
| `algo/pixelate.rs` | Colour halftone filter (a look, not a separation) | `screen.halftone` makes real films per ink | Not useful: different purpose. |
| `cms` (pure-Rust ICC v2/v4, CMYK LUT profiles, black point compensation, gamut check) | Profile parsing and writing, 3D/4D tetrahedral transforms, built-in profiles | VectorCraft's `crates/color` already uses `moxcms` for ICC | Redundant now. Worth revisiting if separations need CMYK-from-profile with intent and black point control, or a gamut warning for spot-colour inks. It has no workspace dependencies, so it could be vendored whole. |
| `psd` (ASE, ACO, GRD, patterns) | Swatch and gradient file readers | VectorCraft imports swatch libraries through its own readers | Possible later: Photoshop swatch (`.aco`) and Swatch Exchange (`.ase`) import for ink libraries. Check that VectorCraft's importer doesn't already cover it first. |
| `color/blend.rs`, `ops` (tiles) | Blend modes, tile processing | Not needed by the separation engine | Skip. |

Not decided: whether to wire PhotoCraft as a git dependency (SC-0d in `docs/needs-owner.md`).
That question is unchanged; the review found no code that needs it today.
