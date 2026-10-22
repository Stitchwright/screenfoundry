# ScreenFoundry build spec

Hand-off document for anyone, person or Claude Code session, continuing ScreenFoundry.
Status: end of the first build session (2026-10-21). Research behind it:
[`research/silkscreen-landscape.md`](research/silkscreen-landscape.md).

## 1. What ScreenFoundry is

A free, open-source screen-printing prepress application: it turns artwork into the films (or
CTS output) used to expose screens. Today's market is a stack of separate paid products: a design
host (Photoshop, Illustrator or CorelDRAW), a separator (Separation Studio, UltraSeps, T-Seps,
Separo, InkSeps) and a film RIP (AccuRIP, Wasatch). There is no maintained open-source
alternative (the research lists what exists and their licences).

ScreenFoundry is a fork of StitchFoundry, itself a fork of VectorCraft, so it inherits a vector
editor, a command engine, MCP and a WASM build. The raster tools come from
[PhotoCraft](https://github.com/storytold/photocraft) (MIT OR Apache-2.0, same family), reused
by reading and re-implementing or by copying with attribution in `NOTICE`.

**Users:** garment decorators (small shops, schools, hobbyists) and poster/art printmakers.
**Out of scope for now:** shop management/job tracking, mockups, vendor RIP drivers.

## 2. Ground rules

1. [`CLAUDE.md`](../CLAUDE.md) applies unchanged: clean-room (never use Adobe software), no
   panics in shipped code, everything is a command, wasm must build.
2. **Licences:** only MIT/Apache/BSD/public-domain code may be copied. GPL/AGPL projects
   (InkSplit, Separate+, Ghostscript, `imagequant` 3.x/4.x) are ideas only.
3. **Numbers are inputs.** Mesh/lpi ratios, angles, minimum dots, choke widths and print order
   come from practitioner sources that disagree. Ship them as editable profiles and cite sources;
   never hard-code them as truth.
4. **Test inputs are written from scratch** (synthetic artwork in the test), never made with a
   competing product.

## 3. What exists today

`crates/sc-core` (package `sc-core`, layer: standalone): see the README table. 33 unit tests;
clippy clean with the workspace no-panic lints. Wired into the engine as `screen.*` commands (SC-1) and so reachable from the menu, the CLI, the control channel and MCP.

Known simplifications to fix:

- Separation is hard assignment to the nearest ink: no anti-aliased edge handling, no
  simulated-process (overlapping halftone channels), no index-colour dithering.
- Trap and choke use one global radius; no per-pair or per-edge control.
- Film files are 1-bit PNG or uncompressed TIFF only: no compressed TIFF, PDF or job sheet yet.
- The app icon artwork is still StitchFoundry's (renamed files, old picture): a new original icon is part of SC-9.
- The bug-report page of the StitchHub still knows this app as `stitchfoundry` (`help.rs`); switch it when the hub has `screenfoundry`.

## 4. Roadmap

| Id | Task | Notes |
|---|---|---|
| SC-1 (done: commands `screen.palette`, `screen.advice`, `screen.separate`, `screen.halftone`; Screen Print menu with Separate Colors… and Halftone Film…; the panel comes with SC-3) | Engine commands: `screen.palette`, `screen.separate`, `screen.underbase`, `screen.halftone`, `screen.marks` | Wrap `sc-core`; params documented; tests per command; reachable from MCP and CLI. |
| SC-2 (partly done: 1-bit PNG and uncompressed TIFF, with the resolution recorded) | Film export: 1-bit TIFF (CCITT G4) and PDF per screen, at job dpi, with job-sheet page | Left: compressed TIFF, PDF, the job sheet. Use permissive crates only. |
| SC-3 (first part done: Screen Print panel with method, ink list, find colors, underbase, trap, marks, per-film preview and save; ink library still to do) | Separations panel: palette editor, per-plate preview, ink library, mesh and lpi advice | Panels read engine state only. |
| SC-4 | Vector-first spot separation: each ink = a layer or object set; overprint/knockout, trap on paths | Uses the vector document directly; no rasterising until output. |
| SC-5 (first part done: `screen.simulate`) | Simulated process: spot-channel halftones over an underbase, highlight white | Done: per-pixel ink shares by optical mixing in linear light, least-ink tie-break, per-ink angles. Left: dot-gain compensation, highlight white, ink-opacity/overlap model, an ink-limit option. Calibrate with real shops. |
| SC-6 | Index colour with a fixed ink palette and non-overlapping patterns | Palette selection per the research. |
| SC-7 | Stochastic (FM) and hybrid screening | Check void-and-cluster patent status first. |
| SC-8 | CTS / DTF / DCS 2.0 output; job-sheet export | After SC-2. |
| SC-9 | Rename to `screenfoundry` | Done 2026-10-21 except the icon artwork (needs a new original icon) and the hub name above. Binaries `screenfoundry` and `screenfoundry-cli`; documents save as `.screenfoundry`; `.stitchfoundry`, `.vectorcraft` and `.drawcraft` still open; settings come from the old StitchFoundry and VectorCraft folders when there are none yet. |
| SC-10 | Calibration: print-test pages and shop feedback loop to tune the default profiles | The only way to settle the research's open conflicts. |

## 5. Decisions for the owner

Decided by the owner on 2026-10-21: remove the embroidery code (done, `docs/plans/remove-embroidery.md`); use PhotoCraft both as a git dependency and by copying with attribution where code is adapted; name the app and CLI `screenfoundry`.

## 6. Verification

`cargo test -p sc-core`, `cargo clippy -p sc-core --all-targets -- -D warnings`,
`cargo xtask layers`; full `cargo xtask ci` before each commit once commands land.

## 7. Revision history

- 2026-10-21: created; `sc-core` separation core landed.
- 2026-10-21: SC-1 landed: four `screen.*` engine commands, PNG films (1-bit TIFF/PDF is SC-2).
- 2026-10-21: Screen Print menu (right of Effect) with two folder-picking items; translations for 23 languages are a machine-written first pass and need native review.
- 2026-10-21: embroidery removed (`docs/plans/remove-embroidery.md`); app renamed to ScreenFoundry (binaries, ids, packaging, catalogs) with the old document extension and settings folder still read.
- 2026-10-21: SC-2 first part: 1-bit PNG and TIFF films with their resolution (`format` option).
- 2026-10-21: SC-5 first part: `screen.simulate` (simulated-process separation); `sc-core::simulated`.
- 2026-10-22: SC-3 first part: Screen Print panel (Window menu); palette finder keeps exact flat colours and drops edge-blend clusters.
