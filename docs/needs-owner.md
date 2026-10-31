# What ScreenFoundry still needs from the owner

Status: 2026-10-24. This used to be a list of questions. Most of them were screen-printing
questions the owner could not be expected to answer, so they were researched instead and settled
(see "Settled by research" below, with sources). What is left needs a real printed sheet or a
business decision.

## 1. One printed test sheet (SC-10): the only thing that really needs a shop

- **What:** run `screen.testPage` (Print Test Page in the Screen Print menu), or ask any local
  screen printer to run it on their press with their usual mesh and ink, then photograph or
  scan the result. Note which tone patches, line widths and trap overlaps held. One shop is
  enough to start; the same sheet on a second mesh (for example 156 and 230) is better.
- **Why nobody can look it up:** dot gain and what holds on a screen change with mesh, tension,
  squeegee, ink and exposure, so published numbers are only anecdotes (one forum thread measured
  5% printing as 17% and 50% as 72%; a trade article gives 20% gain at the 50% mark as an example).
  A printer measures their own setup the same way.
- **What it unlocks:** you (or the printer) enter the measured tones as `dotCurve` and the
  defaults for dot limits, line screen, trap and underbase choke can be set from data.
- **Meanwhile:** the defaults come from published practitioner advice.

## 2. A DCS reader to try the DCS output on (SC-8)

- **What:** one file from `screen.separate` with `dcs: single` and one with `dcs: multiple`, opened
  in any DCS-aware RIP or layout program, and a note of what it did. Wasatch SoftRIP and Fiery XF
  are examples of RIPs that read DCS2.
- **Why:** the writer follows the public DCS 2.0 specification but has only been read back by
  our own code. Most shops now take 1-bit TIFF, so this is low urgency.

## 2b. MakerFoundry (SC-11): your OK and two switches
- **Live check:** say OK and a throwaway Foundry account publishes a tiny SVG. It leaves a test draft on MakerFoundry (no delete endpoint for apps).
- **The web app's address** (`https://stitchwright.github.io/screenfoundry/`) returns 404 until the repository is public and Pages is on (Settings › Pages › Source: GitHub Actions). Then tell the
  MakerFoundry chat to switch the Open in ScreenFoundry button on.
- **Browser check:** the page needs a Chromium run of the real build (Trunk). It was not done yet.

## 3. Translation review

- **What:** a native-speaker check of the machine-written first pass in the 23 catalogs
  (`crates/ui-egui/src/i18n/*.tsv`, every "ScreenFoundry:" comment block).

## 4. Decisions only the owner can make

- **Bug-report hub name:** which name to switch to when the bug reporter supports it.
- **PhotoCraft git dependency (SC-0d):** deferred; say when to wire it.
- **FM screening and patents:** error diffusion was chosen so no patent question arises. No legal
  opinion was taken. If FM screening ships commercially, ask someone qualified.

## Settled by research (2026-10-24)

These were open questions; each now has a default with its source. None is a standard, so
every one is a setting a shop can change.

- **Highlight white (SC-5): built** (`highlightWhite` on `screen.simulate`). Sources agree on
  what it is: a separate opaque white screen, printed last (order: white underbase, colours light
  to dark, black, highlight white) on a 180–230 mesh, which lifts the lightest areas on a dark
  garment ([T-Biznetwork part two](https://t-biznetwork.com/computergraphics/colorseparations/simulatedseps-part2/),
  [Screen Printing magazine](https://screenprintingmag.com/?p=156)). None gives a formula for how
  its dots are sized, so ours is a plain ramp that starts at relative luminance 0.6 and reaches
  full white (`from` changes the start). Treat the ramp as a starting point.
- **Blend modes (SC-4): decided.** Practitioners advise overprint or Darken for spot colours and
  advise against Multiply because it compounds when shapes stack
  ([Adobe community answers](https://community.adobe.com/t5/illustrator-discussions/spot-colour-used-with-transparency/m-p/13073245/highlight/true)).
  Multiply and Darken now print over what is below, like overprint. Other blend modes and masks
  still print as plain solid shapes and are counted in the notes.
- **Minimum FM dot per mesh (SC-7): nothing to look up.** No published table ties a minimum
  stochastic dot to a mesh; vendors say shops test micron and gamma settings against their own
  meshes and exposure ([solutionsforscreenprinters](https://solutionsforscreenprinters.com/?p=24811),
  [Wasatch](https://www.wasatch.com/htmlHelp/en/color_separations/other_screens.html)). Our `dotMm`
  is exactly that setting; the test page shows what a screen holds.
- **Machine-specific CTS formats (SC-8): not needed.** Direct-to-screen systems take 1-bit TIFF
  from a RIP: Exile Screenmaker (CADLink RIP, 1-bit TIFF), Douthitt CTS-30 (1-bit uncompressed
  TIFF at 600 dpi), Fiery XF with the OneBit option, Kodak Matchprint (1-bit TIFF, G4 and LZW),
  Wasatch (one 1-bit TIFF per separation)
  ([Exile](https://anatol.com/presses/screenmaker), [Douthitt](https://spsionline.com/products/douthitt-cts-30-direct-to-screen-unit),
  [Fiery](https://help.fiery.com/cws/FieryXF/9.0_cws_7.2/en-us/GUID-5D22C8CC-313D-4484-88AE-EBF45A043901.html),
  [Matchprint](https://bigpicturemag.com/kpgs-matchprint-color-rip-gets-one-bit-tiff-interface/amp/),
  [Wasatch](https://www.wasatch.com/htmlHelp/en/color_separations/sepTiff.html)). `screen.separate`
  already writes uncompressed and G4 1-bit TIFF at the machine's dpi, mirrored when asked.
- **TIFF-preview DCS headers (SC-8): optional, skipped.** The DCS 2.0 specification itself marks
  the TIFF preview "(optional)" and requires only the composite
  ([DCS 2.0 specification](https://www.printtechnologies.org/standards/files/dcs20spec.pdf)).
- **Dot gain figures:** there is no typical number to build in; see item 1.

## Left out on purpose

- **Ink-overlap model (SC-5):** simulated process treats dots as side by side. Modelling one ink
  printing over another needs measured opacity per ink; the test sheet could be extended to
  measure it if a shop is willing.
- **Ink per tag (SC-4):** groups and layers already give one ink per object set; a tag adds
  nothing a shop asked for.
- **Patterns and meshes (SC-4):** rare in screen-print art; use the raster commands.
- **Outline nesting for gang sheets (SC-8):** DTF gang sheets are packed as rectangles; outline
  nesting only pays off for very irregular shapes.
