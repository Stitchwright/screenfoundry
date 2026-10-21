# ScreenFoundry

**Open-source screen-printing (silkscreen) prepress: artwork in, one film per screen out.**

ScreenFoundry separates artwork into spot-colour plates, builds the underbase, traps the colours,
screens tones into halftone dots at the right angle for your mesh, and adds registration marks.
It is written in pure Rust, MIT OR Apache-2.0, and built on the
[StitchFoundry](https://github.com/Stitchwright/StitchFoundry) / VectorCraft vector editor so art,
separations and films live in one document, with PhotoCraft-style raster tools to follow.

> [!NOTE]
> **Pre-release, early.** The separation core (`crates/sc-core`) exists and is tested. The app
> shell is still the inherited StitchFoundry editor; ScreenFoundry commands and panels are the next
> milestone. Nothing has been exposed on a real screen yet: test-expose before production runs.

## What works today (`sc-core`)

| Stage | What it does |
|---|---|
| Palette | Dominant colours of the artwork (deterministic k-means in CIE Lab). |
| Spot separation | One solid plate per ink, nearest in Lab; transparent pixels take no ink. |
| Tone plate | One ink's coverage from white to the ink, for single-ink halftones. |
| Choke / spread | Exact Euclidean (disc) morphology in linear time. |
| Underbase | Union of the colour plates, choked in; white inks get none. |
| Trapping | Each colour that prints before a darker one is spread under it. |
| Halftone | Round, elliptical and line dots at any angle and lpi, with dot-percentage limits; area coverage matches tone. |
| Registration marks | Crosshair-in-circle at the four corners, outside the artwork. |
| Mesh check | Advice on lpi against mesh count and on angles that line up with the threads. |
| Jobs | A serialisable job (inks, mesh, underbase, trap) run into films in print order. |

Every default number is a practitioner rule of thumb gathered from public sources and is an input
you can change, not a fact: see [`docs/research/silkscreen-landscape.md`](docs/research/silkscreen-landscape.md)
for what is known, what conflicts and what is still unverified.

## Build

```sh
cargo test -p sc-core
cargo clippy -p sc-core --all-targets -- -D warnings
```

The full app build is the inherited one; see [`docs/development.md`](docs/development.md).

## Where to read next

- [`docs/screenfoundry-spec.md`](docs/screenfoundry-spec.md): product, rules, what exists, roadmap.
- [`docs/research/silkscreen-landscape.md`](docs/research/silkscreen-landscape.md): the market and technique research.
- [`CLAUDE.md`](CLAUDE.md): rules for agents (clean-room, no panics, everything is a command).

## Licence

MIT OR Apache-2.0 ([`LICENSE-MIT`](LICENSE-MIT), [`LICENSE-APACHE`](LICENSE-APACHE)). Based on
VectorCraft by the ArtCraft team, through StitchFoundry. See [`NOTICE`](NOTICE).
