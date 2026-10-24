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
  simulated-process (overlapping halftone channels), index colour is separate (`screen.index`).
- Trap and choke use one global radius; no per-pair or per-edge control.
- Film files are 1-bit PNG, uncompressed TIFF or CCITT Group 4 TIFF or one-page PDF (Group 4 image), plus a PDF job sheet (`jobSheet` option).
- The app icon artwork is still StitchFoundry's (renamed files, old picture): a new original icon is part of SC-9.
- The bug-report page of the StitchHub still knows this app as `stitchfoundry` (`help.rs`); switch it when the hub has `screenfoundry`.

## 4. Roadmap

| Id | Task | Notes |
|---|---|---|
| SC-1 (done: commands `screen.palette`, `screen.advice`, `screen.separate`, `screen.halftone`; Screen Print menu with Separate Colors… and Halftone Film…; the panel comes with SC-3) | Engine commands: `screen.palette`, `screen.separate`, `screen.underbase`, `screen.halftone`, `screen.marks` | Wrap `sc-core`; params documented; tests per command; reachable from MCP and CLI. |
| SC-2 (done: 1-bit PNG, uncompressed TIFF, CCITT G4 TIFF and PDF films with the resolution recorded, and the PDF job sheet) | Film export: 1-bit TIFF (CCITT G4) and PDF per screen, at job dpi, with job-sheet page | Left: nothing planned; DTF and direct-to-screen output are SC-8. Use permissive crates only. |
| SC-3 (done: Screen Print panel with method, ink list, find colors, snap to library, underbase, trap, marks, per-film preview and save; `sc-core::inklib` generic library) | Separations panel: palette editor, per-plate preview, ink library, mesh and lpi advice | Panels read engine state only. |
| SC-4 (first part done: `screen.separateVector`; Vector Art in the panel) | Vector-first spot separation: each ink = a layer or object set; overprint/knockout, trap on paths | Done: solid fills and strokes (outlined) and type (first style) onto inks by colour or spot swatch; knockout up the stacking order and overprint; clip groups; trap and underbase as path offsets and booleans; one hard-edged draw per plate. Vector plates as one-page PDFs (`vectorPdf`, with registration marks as paths). Ink per layer or per outermost group (`inkBy`). Per-pair trap (`pairs`). Plates as SVG (`vectorSvg`). Spot tints and gradients print as dots (`tones`, raster films only). Plates as document layers (`intoLayers`). Opacity of solid fills prints as dots and overprints. Symbol instances are separated through their art. Multiply and Darken print as overprint. Left: other blend modes, masks, patterns and meshes (see `docs/needs-owner.md`). |
| SC-5 (partly done: `screen.simulate`, dot gain) | Simulated process: spot-channel halftones over an underbase, highlight white | Done: per-pixel ink shares by optical mixing in linear light, least-ink tie-break, per-ink angles. Dot-gain compensation done (`dotGain`; `inkGains` per ink, one number each). `inkLimit` caps total coverage. Measured dot-gain curve (`dotCurve`, `sc-core::curve`). Highlight white (`highlightWhite`; role and order sourced, ramp is a starting point). Left: ink-opacity/overlap model (needs print data). Calibrate with real shops. |
| SC-6 (done: `screen.index`, `sc-core::index`, Index Color in the panel) | Index colour with a fixed ink palette and non-overlapping patterns | Palette from the existing Lab k-means (`colors`) or the user's inks; Floyd-Steinberg error diffusion in linear light on square cells; one ink a pixel. Diffusion kernels floyd/jarvis/stucki (`kernel`). Ordered (Bayer) dither (`pattern`). Left: other patterned fills, palette optimisation against the dithered result. |
| SC-7 (done: FM, hybrid and three diffusion kernels) | Stochastic (FM) and hybrid screening | `screening: am|fm|hybrid` on `screen.halftone` and `screen.simulate`: error diffusion on cells of `dotMm` (`sc-core::fm`) with Floyd-Steinberg, Jarvis-Judice-Ninke or Stucki kernels, the dot limits and gain; a hybrid angles the middle tones and scatters cells under `hybridBelow` and over `hybridAbove`. Error diffusion was chosen over void-and-cluster so no patent question arises. Left (nothing sourced): a mesh-aware minimum dot size; blue-noise textures other than diffusion. |
| SC-8 (done: `screen.dtf`, `mirror`, `dcs`) | CTS / DTF / DCS 2.0 output; job-sheet export | DTF: mirrored colour on transparent plus a choked white layer (solid or graded), PNG or TIFF, a gang sheet of artboards in shelf-packed rows with optional quarter turns (`sc-core::dtf`). Films: `mirror` (also on the vector PDFs). CTS: the 1-bit TIFF films at the machine's dpi, mirrored when asked; no machine-specific formats (none sourced). DCS 2.0 single- and multiple-file (`sc-core::dcs`), written from the public specification and checked only by reading it back: no DCS consumer was available. The DCS composite resolution is chosen (`dcsCompositeDpi`; full resolution allowed) and its spot CMYK goes through the colour settings. Gang sheets also try free-rectangle packing. Left: shape (non-rectangular) nesting, machine-specific CTS formats (none sourced), TIFF-with-preview DCS headers. The job sheet is done (SC-2). |
| SC-9 | Rename to `screenfoundry` | Done 2026-10-21 except the icon artwork (needs a new original icon) and the hub name above. Binaries `screenfoundry` and `screenfoundry-cli`; documents save as `.screenfoundry`; `.stitchfoundry`, `.vectorcraft` and `.drawcraft` still open; settings come from the old StitchFoundry and VectorCraft folders when there are none yet. |
| SC-10 | Calibration: print-test pages and shop feedback loop to tune the default profiles | First part done 2026-10-23: `screen.testPage` makes the sheets. Tuning the defaults needs real prints from a shop. The only way to settle the research's open conflicts. |

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
- 2026-10-22: SC-2: CCITT Group 4 TIFF films (`format: tiff-g4`, `fax` crate, MIT).
- 2026-10-22: SC-8 first part: `screen.dtf` (DTF colour and white files).
- 2026-10-23: SC-7 finished: hybrid AM/FM (`screening: "hybrid"`) and `fmKernel`.
- 2026-10-23: SC-7: FM screening (`screening: "fm"`, `dotMm`; AM/FM choice in the panel).
- 2026-10-23: SC-8 leftovers: gang-sheet rotation, DCS composite resolution, colour-managed DCS CMYK.
- 2026-10-23: SC-8 finished: DTF TIFF, graded white and gang sheets; `mirror` on films; DCS 2.0 (`dcs`); CTS note.
- 2026-10-22: SC-4: ink per layer (`inkBy: "layer"`).
- 2026-10-22: SC-4: vector plates as PDFs (`vectorPdf` on `screen.separateVector`).
- 2026-10-22: SC-4 first part: `screen.separateVector` (separation on paths; Vector Art in the panel).
- 2026-10-22: SC-6: index colour (`screen.index`; Index Color in the panel).
- 2026-10-22: SC-5: dot-gain compensation (`dotGain` on `screen.halftone` and `screen.simulate`; Dot Gain in the panel).
- 2026-10-22: SC-3: ink library (`sc-core::inklib`, `screen.inkLibrary`, `screen.matchInks`, `snap` option, Snap to Library button).
- 2026-10-22: SC-2: PDF job sheet (`jobSheet` option; `sc-core::jobsheet`).
- 2026-10-22: SC-2: PDF films (`format: pdf`): one page per film at physical size.
- 2026-10-22: SC-3 first part: Screen Print panel (Window menu); palette finder keeps exact flat colours and drops edge-blend clusters.
- 2026-10-23: SC-10 first part: print-test page (`sc-core::testpage`, `screen.testPage`).
- 2026-10-23: SC-5: per-ink dot gain (`inkGains` on `screen.simulate`).
- 2026-10-23: SC-5: ink limit (`inkLimit` on `screen.simulate`).
- 2026-10-23: SC-6: diffusion kernels for index colour (`kernel` on `screen.index`).
- 2026-10-23: SC-4: ink per group (`inkBy: "group"`), per-pair trap (`pairs`), SVG plates (`vectorSvg`).
- 2026-10-23: SC-4: spot tints and gradients as halftone dots (`tones`).
- 2026-10-23: SC-5: measured dot-gain curve (`dotCurve`).
- 2026-10-23: SC-6: ordered dither for index colour (`pattern: ordered`).
- 2026-10-23: SC-4: plates as document layers (`intoLayers`). SC-8: free-rectangle packing for gang sheets.
- 2026-10-23: SC-4: opacity of solid fills and strokes prints as dots that let the inks below show.
- 2026-10-24: SC-4: symbol instances separate through their art.
- 2026-10-24: SC-5: highlight white. SC-4: Multiply and Darken print as overprint. Open questions researched and recorded in `docs/needs-owner.md`.
