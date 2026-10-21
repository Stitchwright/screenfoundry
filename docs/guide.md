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
each dot cell needs at least four film pixels.

## Mesh and line screen

The mesh limits how fine a halftone can be. As a rule of thumb the line screen is between a fifth
and a quarter of the mesh count, and never more than a third. **Check Mesh and Line Screen**
(`screen.advice`) gives the recommended range and says when a setting is too fine, and when the
angle lines the dots up with the mesh threads (0° or 90°), which invites moiré.

## Registration marks

With `marks`, every film gets a crosshair-in-circle at each corner, outside the art. All films
of a job grow by the same margin, so the marks line up from one film to the next.

## Films and output

Films are 1-bit images, black where ink must reach the garment, with the job's resolution
recorded in the file so they print at the right size. Choose `format`: `png` (default) or `tiff`
(uncompressed baseline, which imagesetters and direct-to-screen machines read). Choose a folder
with `dir`; without one the films come back as base64. PDF output, direct-to-screen and DTF
output are planned.

## Commands and automation

| Command | What it does |
|---|---|
| `screen.palette` | The artwork's dominant colors. |
| `screen.advice` | Whether a line screen suits a mesh. |
| `screen.separate` | One film per screen. |
| `screen.halftone` | One ink's tones as dots. |

The artwork is the document's first artboard (`source: "document"`), an image file
(`source: {path}`) or inline bytes (`source: {dataBase64}`). Run `engine.commands` for every
parameter.

## Known limits

- Separation is one solid film per ink. Simulated process (overlapping halftone channels) and
  index color are not built yet.
- One trap width and one choke width for the whole job.
- Films are PNG or uncompressed TIFF; no PDF, compressed TIFF, or machine-specific output yet.
- The defaults come from published practitioner advice that disagrees in places. Test-print
  before a production run.
