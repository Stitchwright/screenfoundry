# What ScreenFoundry needs from the owner

Status: 2026-10-23. Everything below is blocked on something only a person can supply. Each item
says what is needed, what it unblocks, and what happens meanwhile.

## 1. Real prints (SC-10 calibration)

- **What:** print `screen.testPage` (Print Test Page) on your press with your usual mesh and inks,
  scan or photograph the result, and note which tone patches, line widths and trap overlaps held.
  Two or three shops, if possible, with different meshes (110, 156, 230).
- **Unblocks:** tuning the default dot limits, line screens, dot gain, trap widths and underbase
  choke from data instead of rules of thumb; the research's open conflicts
  (`docs/research/silkscreen-landscape.md`).
- **Meanwhile:** the defaults come from published practitioner advice, not from prints.

## 2. A real DCS 2.0 consumer (SC-8)

- **What:** one file from `screen.separate` with `dcs: single` and one with `dcs: multiple`, opened
  in any DCS-aware RIP or layout tool you own, and a note of what it did (opened, ignored plates,
  errors).
- **Unblocks:** knowing the DCS writer is correct. It was written from the public specification
  and only read back by our own code.

## 3. Machine-specific CTS formats (SC-8)

- **What:** the name of the computer-to-screen machine(s) you target and their published file
  format documentation (or sample jobs you own and may share).
- **Unblocks:** direct output for that machine. Today `screen.separate` writes 1-bit TIFF films at
  the machine's dpi, which most RIPs accept.

## 4. TIFF-with-preview DCS headers (SC-8)

- **What:** the public specification text for the TIFF-preview EPS header variant, or a sample.
- **Unblocks:** DCS files that show a preview in layout tools.

## 5. Mesh-aware minimum FM dot (SC-7)

- **What:** a source (shop guidance or a published table) for the smallest stochastic dot a given
  mesh holds.
- **Unblocks:** the FM dot size defaulting from the mesh instead of a fixed `dotMm`.

## 6. App icon (SC-9)

- **What:** an original icon (or permission to generate a simple one in code), licensed
  MIT OR Apache-2.0, with a row in `ASSETS.md`. The icon in the repo is still the StitchFoundry
  artwork.

## 7. Translation review

- **What:** a native-speaker check of the machine-written first pass in the 23 catalogs
  (`crates/ui-egui/src/i18n/*.tsv`, every "ScreenFoundry:" comment block).
- **Meanwhile:** the strings are understandable but may be stiff or wrong in places.

## 8. Decisions only the owner can make

- **Bug-report hub name:** which hub name to switch to when the bug reporter supports it.
- **PhotoCraft git dependency (SC-0d):** deferred; say when to wire it.
- **FM screening and patents:** error diffusion was chosen so no patent question arises. No legal
  opinion was taken. If FM screening ships commercially, ask someone qualified.
