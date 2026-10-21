# ScreenFoundry app icon

**Subject:** machine embroidery: a rectangular machine hoop with the bracket that clips onto the
embroidery arm, the machine's needle and embroidery foot, and a satin-stitched S being sewn.

**Palette:**

| Colour | Hex | Used for |
|---|---|---|
| Navy | `#1e2b4f` | the full-bleed field |
| Coral | `#f2546b` (lighter `#ff8c9b`, darker `#d63d55`, shadow `#b8344b`) | the satin S and the thread |
| Linen | `#f6f1e7` | the fabric in the hoop |
| Greys | `#e4e8ef`, `#c7cedb`, `#b3bccb`, `#8a94a8` | hoop, machine head, needle bar, foot |

**Tile:** `viewBox="0 0 512 512"`, a rounded square with `rx=112`, clipped. The macOS files add Apple's
transparent margin (tile = 824/1024 of the canvas); Windows and Linux use the tile edge to edge.

**Provenance:** original artwork made for ScreenFoundry, written by hand as SVG (shapes and a stripe
pattern, no third-party material). `screenfoundry.svg` is the canonical artwork; `screenfoundry-small.svg`
is the same file for `hicolor/scalable`. Licence: see `LICENSE.txt`.

## Files

| File | What | Used by |
|---|---|---|
| `screenfoundry.svg` | master vector | source for everything below |
| `screenfoundry-small.svg` | the same artwork | `hicolor/scalable` |
| `screenfoundry-1024.png` | full tile, 1024 px | docs, store listings |
| `screenfoundry-macos-512.png` | tile with Apple margin, 512 px | runtime Dock icon (`apps/vectorcraft/src/main.rs`) |
| `screenfoundry.icns` | macOS icon set, 16 to 1024 px | `cargo xtask bundle` (`CFBundleIconFile`) |
| `screenfoundry.ico` | Windows icon, 16 to 256 px | `apps/vectorcraft/build.rs` (embedded in the `.exe`) |
| `hicolor/<size>/apps/com.stitchwright.screenfoundry.png` | Linux theme icons, 16 to 512 px; the 256 one is also the runtime icon on Windows and Linux, the 128 one the brand mark in the app bar and About box (`crates/ui-egui/src/brand.rs`) | `packaging/linux/com.stitchwright.screenfoundry.desktop` |
| `hicolor/scalable/apps/com.stitchwright.screenfoundry.svg` | Linux scalable icon | as above |

## How it reaches each OS

- **macOS:** the app sets the Dock / app-switcher icon at runtime (`ViewportBuilder::with_icon`); the
  `.app` from `cargo xtask bundle` carries `screenfoundry.icns`.
- **Windows:** `build.rs` embeds `screenfoundry.ico` and VERSIONINFO with `winresource` (taskbar, Start
  menu, Explorer, Alt-Tab); the runtime icon covers the title bar.
- **Linux:** install `packaging/linux/com.stitchwright.screenfoundry.desktop` to `share/applications/`
  and `hicolor/` to `share/icons/`. The app sets its Wayland app ID / X11 class to
  `com.stitchwright.screenfoundry`, so the dock matches the window to the launcher.

## Regenerate

Edit `screenfoundry.svg` (and copy it to `screenfoundry-small.svg`), then run `packaging/icons.sh` (needs
`resvg`). It writes every PNG, the `.ico` (`cargo xtask ico`) and the `.icns` (`cargo xtask icns`).
