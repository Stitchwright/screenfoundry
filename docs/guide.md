# ScreenFoundry: screen-printing prepress

ScreenFoundry turns artwork into the films used to expose screens for screen printing
(silkscreen). It is a vector editor with a separation engine built in: draw or import the art,
then separate it into one film per screen, with an underbase, trapping, halftones and
registration marks.

Everything is a command (`screen.*`), so the same features work from the menus, the control
channel, the command-line tool and the MCP server.

- [Quick start](#quick-start)
- [Colors and inks](#colors-and-inks)
- [Underbase](#underbase)
- [Trap and choke](#trap-and-choke)
- [Halftones](#halftones)
- [Simulated process](#simulated-process)
- [FM screening](#fm-screening)
- [Vector art](#vector-art)
- [Index color](#index-color)
- [DTF files](#dtf-files)
- [Mesh and line screen](#mesh-and-line-screen)
- [Registration marks](#registration-marks)
- [Films and output](#films-and-output)
- [Commands and automation](#commands-and-automation)
- [Known limits](#known-limits)

## Quick start

1. Draw the art, or open an SVG, PDF or image. Each ink should be a flat color.
2. Choose **Screen Print › Separate Colors…** and pick a folder.
3. ScreenFoundry finds the colors in the first artboard, makes one film per color, and writes
   them to the folder as PNG files, with registration marks.
4. Print the films on film positive and expose the screens.

The **Screen Print** panel (Window › Screen Print) does the same with live feedback: choose the
method, press **Find Colors**, edit the inks, then **Preview** to see each film and how much of
the art it covers, or **Save Films…**.

For a photo or a tonal image in one ink, choose **Screen Print › Halftone Film…** instead.

## Colors and inks

**Find Ink Colors** (`screen.palette`) lists the dominant colors of the artwork, most used
first. **Separate Colors** (`screen.separate`) gives every opaque pixel to the nearest ink, judged
in CIE Lab so that "nearest" follows how colors look, not their RGB numbers. Transparent pixels
take no ink. The films never overlap: this is spot-color separation, one solid film per ink.

Give the inks yourself (`inks`: name, color, mesh) or let ScreenFoundry find them (`colors`: how
many, up to 12).

## Underbase

On a dark garment, colors print over a white underbase. Ask for one with `underbase`. It is
the footprint of all the colors together, pulled in by a choke (default 0.3 mm) so no white shows
at the edges. White and near-white inks get no underbase of their own. The underbase prints first.

## Trap and choke

Screens never line up perfectly. **Trap** spreads each color that prints before a darker one a
little underneath it (default 0.25 mm), so a hairline gap does not show. The darkest color, which
prints last, is not spread. The colors print from light to dark by default.

These widths are rules of thumb that vary between shops and presses. Treat them as starting
points and adjust them (`trapMm`, `underbase.chokeMm`) after a test print.

## Halftones

**Halftone Film** (`screen.halftone`) turns the tones of one ink into dots. Choose the dot shape
(round, ellipse, line), the lines per inch (`lpi`, default 45), and the screen angle (default
22.5°). Tones lighter than `minDot` (default 8%) print nothing, because tiny dots do not hold on
the screen; tones darker than `maxDot` (default 95%) print solid. The film's resolution is `dpi`;
each dot cell needs at least four film pixels. Ink spreads through the mesh, so a printed dot comes
out bigger than the film's. Set `dotGain` (a fraction: 0.2 means a 50% dot prints as 70%; at most
0.25, default 0 = off) and the film's tones are lightened to cancel it, most at the middle and not
at all at 0% and 100%. Measure the gain on a test print; it depends on mesh, ink and squeegee.

## Simulated process

Spot colours give one solid film per ink. **Simulated Process** (`screen.simulate`) reproduces
full-colour art, photographs and gradients with a handful of inks printed as halftone dots side
by side over a base (a white underbase, or the garment: `base`).

For every pixel ScreenFoundry works out the share of the area each ink should cover. It treats
the dots as mixing in linear light: the pixel looks like the area-weighted average of the inks
and the base. It picks the closest colour that mix can make, and where several mixes give the
same colour it takes the one with the least ink, because fewer dots print cleaner. Each ink is
screened at its own angle (`angles`, default 22.5°, 67.5°, 45°, 15°, 75°, …) so the dot
patterns do not clash; the underbase prints first, then the inks light to dark. Up to eight
inks; give them (`inks`) or let ScreenFoundry find them (`colors`).

`dotGain` works here too (and is the Dot Gain box in the panel). It does not model dots overlapping
or how opaque the ink is. Print a test and adjust the dot limits (`minDot`, `maxDot`), the dot gain
and the line screen before a production run.

## FM screening

Both the halftone film and simulated process can screen with scattered equal dots instead of
angled dots: `screening: "fm"`. The number of dots follows the tone, not their size, so there is
no line screen and no screen angle, and two inks cannot moiré against each other. The size of one
dot is `dotMm` (default 0.2 mm), which should be several mesh threads wide or the dots will not
hold on the screen; every dot is a whole square of that size or a run of touching squares. The
dot limits (`minDot`, `maxDot`) and `dotGain` work as for the angled screens. There is no mesh
advice for FM, because nothing sourced says how small an FM dot may be for a mesh.

The method is Floyd–Steinberg error diffusion, run on cells of the dot size. It is not the
void-and-cluster method; that is a patented method of the 1990s, and error diffusion is older.
Error diffusion can show faint worm-like patterns in some flat tones and a hybrid screen (angled
dots in the middle tones, scattered ones at the ends) is not built; print a test and compare it
with the angled screen before choosing it for a job. In the panel, Simulated Process has an AM
and FM choice with a Dot Size.

## Vector art

**Separate Vector Art** (`screen.separateVector`) separates the open document's first artboard
without drawing it first. Every solid fill and every stroke (turned into its outline, with its
dashes, caps and alignment) goes onto an ink, and the work stays on the paths until the last
step: each finished plate is drawn once at the film resolution (`dpi`) with hard edges, so a
straight edge in the art is a straight edge on the film.

- **Inks.** Give them (`inks`, each object takes the nearest one; a spot swatch takes the ink of
  its own name first) or leave them out and every distinct colour or spot swatch of the art
  becomes an ink, up to 12. `snap` names them from the ink library as usual.
- **One ink a layer.** With `inkBy: "layer"` each layer that has shapes is one ink, named after the
  layer and coloured by its first shape, whatever colours its shapes are (sublayers count as
  their layer). An entry of `inks` with the layer's name sets that ink's colour and mesh. Use it
  when the art is already organised by screen.
- **Knockout and overprint.** A shape knocks out the other inks under it, so no ink prints where
  it will be covered, unless the shape is set to overprint (Attributes, or `object.setOverprint`):
  then the ink below prints whole and the shape prints over it.
- **Trap and underbase.** Both are path operations. Each ink grows by `trapMm` (default 0.25 mm)
  only where a darker ink lies, so light colours tuck under dark ones and free edges stay put.
  The underbase is the art's footprint pulled in by `chokeMm`.
- **Vector plates.** With `vectorPdf` every film also comes as a one-page vector PDF the size of
  the artboard (black on nothing, in the `vector` entry of the film, or `NN-role-name.vector.pdf`
  next to the film in `dir`). It holds the plate's paths as they are, trap and knockout included,
  so a RIP can draw them at any resolution. With `marks` the page grows by the same margin as a
  raster film and carries the same four registration marks (as paths), so a vector plate and a
  raster film of the same job line up.
- **Type** is separated through its outlines, in the colours of its first style; use Create
  Outlines when characters differ in colour. Clip groups and layer clips are honoured.
- **Left out, and said so.** Gradients, patterns, meshes, placed pictures, brush strokes, symbol
  instances and live art that needs expanding are listed in `skipped`. Opacity, blend modes,
  masks and effects are ignored (shapes print solid) and counted in `notes`, as are spot tints
  under 100%. Use the raster commands for those.

## Index color

**Index Color** (`screen.index`) reduces full-colour art to a fixed set of inks and prints each
square pixel of the picture in exactly one of them (or in none, leaving the base). There are no
dots, so nothing overlaps and the screens cannot moiré against each other. Give the inks (`inks`)
or let ScreenFoundry find them (`colors`, default 6, up to 12); the base (`base`, default white)
is the colour that shows where no ink prints.

Each pixel takes the palette colour nearest to its own colour plus the error its neighbours left
behind (Floyd-Steinberg error diffusion, worked in linear light so the average brightness
matches the art). `dither` sets how much error spreads: 1 (default) is full diffusion for
photographs, 0 maps every pixel to its nearest ink for flat art. `cellMm` (default 0.5 mm) is the
side of one printed pixel: keep it several mesh threads wide or the pixels will not hold on the
screen. The default is a starting point, not a standard; print a test. Inks print light to dark
as usual, and `underbase` works as in simulated process.

## DTF files

**DTF Files** (`screen.dtf`) makes the files a direct-to-film printer takes from the artwork: the
colour art on a transparent background, mirrored (`mirror`, default on) because the print is
transferred to the garment face down, and a white layer (`white`, default on) to print under the
colours. The white is white ink where the art is, pulled in from its edges by `whiteChokeMm`
(default 0.3 mm) so no white shows past the colour. Both files are the same size and mirroring, at `dpi` (default 300), with the resolution recorded;
`format` is `png` (default) or `tiff` (uncompressed 8-bit RGBA). They are written as
`dtf-color` and `dtf-white` with that extension in `dir`, or come back as base64.

- **Graded white.** `whiteMode: "graded"` makes the white as opaque as the art is, inside the
  footprint, so soft edges and fades get a matching white; the default `solid` is full white
  wherever the art is at least half opaque.
- **Gang sheet.** `artboards` (`"all"`, or a list of artboard numbers from 1) lays several
  artboards of the open document out on one sheet `sheetWidthMm` wide (required; roll widths
  vary, so there is no default), `gapMm` apart (default 5): in rows, tallest first, left to
  right, a new row when the next does not fit. The sheet is as tall as the rows need. With
  `rotate` ScreenFoundry also tries every design turned to lie wider than tall and every design
  turned to stand taller than wide (where it fits across the sheet) and uses the layout with the
  shortest sheet, so turning never does worse than not turning; a design too wide for the sheet
  can then go turned. This is shelf packing, not a search for the best possible nesting. `placements` says where each artboard went (pixels and millimetres, as they are in the
  files, so after mirroring). The mirroring and the white layer then apply to the whole sheet.

What a shop's RIP wants (white made by the RIP or supplied as a file, mirroring, resolution)
varies, so check it against yours. The choke and the gap are starting points, not standards.
There is no highlight-white tool beyond the graded mode, and no nesting beyond the shelf layouts above.

## Mesh and line screen

The mesh limits how fine a halftone can be. As a rule of thumb the line screen is between a fifth
and a quarter of the mesh count, and never more than a third. **Check Mesh and Line Screen**
(`screen.advice`) gives the recommended range and says when a setting is too fine, and when the
angle lines the dots up with the mesh threads (0° or 90°), which invites moiré.

## Registration marks

With `marks`, every film gets a crosshair-in-circle at each corner, outside the art. All films
of a job grow by the same margin, so the marks line up from one film to the next.

## Ink library

`screen.inkLibrary` lists about 35 generic inks (White, Red, Royal Blue, Metallic Gold…). Their
colours are on-screen starting points, not measurements of any ink and not brand colours. In the
panel, **Snap to Library** replaces each ink by the nearest library ink (by CIE76 ΔE, up to 25) and
gives it the library's name unless you renamed it; two inks never snap to the same library ink, so
no two screens come out identical. The same works in commands: `screen.matchInks` returns the
nearest ink for each colour, and `snap` (`true` or `{maxDeltaE, library}`) on `screen.separate` and
`screen.simulate` does it before separating. Pass your own inks as `library: [{name, color}]`; they
are tried before the built-in ones.

## Films and output

Films are 1-bit images, black where ink must reach the garment, with the job's resolution
recorded in the file so they print at the right size. Choose `format`: `png` (default), `tiff`
(uncompressed baseline), `tiff-g4` (CCITT Group 4, which imagesetters and direct-to-screen
machines read and which makes a flat film a few hundred bytes instead of megabytes) or `pdf` (one page per
film at its physical size, for print shops that take PDF). Choose a folder with `dir`; without one the films come back as base64. Add `jobSheet`
(`true`, or `{title, notes}`) for a one-page PDF that travels with the films: the film size and
resolution, then one row per screen in print order with its colour swatch, role, mesh, coverage
and file name, then the mesh and line-screen advice. It is written as `job-sheet.pdf` in `dir`,
or comes back as base64.

**Mirror.** `mirror` flips every film left to right, for printing the film emulsion side down or
for a machine that wants it that way (registration marks are symmetric, so they stay at the
corners). With `vectorPdf` the vector plates are flipped too.

**Computer-to-screen (CTS).** A CTS machine takes a 1-bit bitmap at its own resolution (600 to
over 2500 dpi according to the sources read). The films already are that: set `dpi` to the
machine's (up to 2400) and take `tiff-g4` or `tiff`, with `mirror` if it asks for it. ScreenFoundry
writes no machine-specific format or driver file; if your machine needs one, say so.

**DCS 2.0.** `dcs` (`single` or `multiple`) also writes the films as a DCS 2.0 file, the format
older separation and RIP software reads: a main EPS holding a composite and a pointer to every
plate, and one EPS per plate (a 1-bit grayscale image, black where the ink prints). `single`
puts everything in one file (`%%PlateFile` entries with byte offsets); `multiple` writes the
plates as files beside a main file and needs `dir`. `dcsName` names the job and the files
(default `separation`). It is written from the public specification (June 1993, revised May
1995) and checked by reading the files back, but no DCS consumer or PostScript interpreter was
available to try it in: test it with your own RIP before relying on it. The composite is drawn at
150 dpi by default to keep the main file small; `dcsCompositeDpi` sets it, and the film's own
resolution (or more) gives the full-resolution composite the specification prefers, in a much
larger file (a composite over 50 megapixels is refused). Each plate is a spot colour, and the
CMYK values written for it come from the document's colour settings (working CMYK space and
rendering intent) applied to the ink's screen colour, since a spot ink's real CMYK is not known.

## Commands and automation

| Command | What it does |
|---|---|
| `screen.palette` | The artwork's dominant colors. |
| `screen.advice` | Whether a line screen suits a mesh. |
| `screen.separate` | One solid film per screen (spot colours). |
| `screen.simulate` | Full colour as halftone dots of a few inks. |
| `screen.index` | Full colour as square pixels of a fixed palette of inks. |
| `screen.dtf` | Direct-to-film files: mirrored colour on transparent and a white layer. |
| `screen.separateVector` | The open document's paths as one film per ink, with knockout, overprint and trap. |
| `screen.halftone` | One ink's tones as dots. |

The artwork is the document's first artboard (`source: "document"`), an image file
(`source: {path}`) or inline bytes (`source: {dataBase64}`). Run `engine.commands` for every
parameter.

## Known limits

- Simulated process treats dots as side by side, not overlapping, and the dot gain is one number
  for every ink. 
- One trap width and one choke width for the whole job.
- Films are PNG, TIFF, CCITT Group 4 TIFF or PDF; no machine-specific (CTS, DTF) output yet.
- The defaults come from published practitioner advice that disagrees in places. Test-print
  before a production run.
