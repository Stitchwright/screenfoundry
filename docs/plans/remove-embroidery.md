# Plan: remove embroidery from ScreenFoundry

Progress: stage A done (UI, plus the tools crate and the apps' AI/secret wiring, which turned out to be small and were pulled in); B, C (document model), D and E remain.

Status: approved by the owner 2026-10-21 ("I want the embroidery pulled out"). Executed in the
stages below, one commit per stage. Task id: **SC-0b**.

## Goal

ScreenFoundry is a screen-printing app. The inherited embroidery product (about 65,000 lines
of dedicated code) goes, and what is left is the VectorCraft vector editor plus `sc-core`. The
app must still build, pass `cargo xtask ci`, run on the web (wasm), and open every document it
could open before.

## What goes

| Part | Size | Notes |
|---|---|---|
| `crates/sf-core` | 19.3k lines | embroidery model, stitch engine, thread/hoop/fabric data |
| `crates/sf-formats` | 4.5k | machine file formats (DST, PES, JEF, ...) |
| `crates/embroidery` | 2.2k | embroidery layer over the vector document |
| `apps/stitchfoundry-cloud` | 3.1k | embroidery cloud service, plus `packaging/cloud` |
| `crates/engine/src/cmd/embroidery*.rs` (54 files) | 21.4k | `embroidery.*` commands |
| `crates/engine/src/tests_embroidery*.rs` | 5.9k | their tests |
| `crates/ui-egui/src/embroidery/` | 9.5k | panels, widget, ask/sew-out/account, prompt |
| UI dialogs | | `my_machine`, `compare`, `font_library`, `design_prompt` |
| `crates/tools/src/embroidery.rs` + catalog entries | | 7 embroidery tools |
| Docs | | `embroidery.md`, `stitchfoundry-spec.md`, `underlay-research.md`, `cloud.md` |

## What stays (and why)

- **`Cmd`s that are not embroidery:** quilting (`cmd/quilting.rs`) and `Longarm` items are
  embroidery-adjacent. They go only if they depend on `sf-core`; decided when the compiler
  says so (stage B), and recorded in the commit.
- **Ink/Stitch SVG namespace** (`crates/svg`, `INKSTITCH`): kept as opaque `inkstitch-*` data
  attributes, so SVGs from other tools round-trip unchanged. Harmless; revisit later.
- **Brush "Stitches" pattern, PDF "stitching functions", three-d "stitch" helpers:** unrelated
  uses of the word. Untouched.
- **The product name "StitchFoundry" in strings, icons, packaging:** that is the *rename* task
  (SC-0c), not this one. This plan leaves names alone so the two changes stay reviewable.

## Compatibility

- **Documents:** `Node.embroidery` and `Document.embroidery` are serialised keys. Neither
  struct uses `deny_unknown_fields`, so files that carry them still load; the keys are
  ignored and dropped on the next save. Stage C adds a test that loads a document with both
  keys and checks it opens and re-saves without them. No format version bump.
- **`.stitchfoundry` files** keep opening (`SAVE_EXTENSION` stays until the rename task).
- **Preferences:** unknown preference keys are ignored; stage B adds a test for a preferences
  file that still has embroidery keys.

## Stages (top-down, so the build stays green after each)

Each stage ends with `cargo check --workspace --all-targets`, the targeted tests named
below, `cargo fmt`, clippy `-D warnings`, and a commit `SC-0b.<stage>: ...`.

**A. UI (`ui-egui`).** Delete `src/embroidery/`, the four dialogs, `tests_embroidery.rs`,
`tests_home.rs` embroidery cases; remove the Embroidery menu, panels, workspaces,
toolbar entries, icons and `UiState` fields that exist only for embroidery; drop the
`vectorcraft-embroidery` dependency. Update `tests_nativemenu.rs` (menu titles). Remove the
embroidery rows' *code* references only: translation rows for strings no longer used are
removed in stage E.
Verify: `cargo test -p vectorcraft-ui-egui --lib` (i18n, menus, native menu).

**B. Engine.** Delete `cmd/embroidery*.rs` and `tests_embroidery*.rs`; remove their
registrations from `cmd/mod.rs`; remove `Session::embroidery_plan`, `DocState` fields,
and the references in the 13 other files (`lib.rs`, `help.rs`, `create.rs`, `textedit.rs`,
`export_history.rs`, `quilting.rs`, `pathops.rs`, `fileio/{mod,save,load}.rs`, `tooling.rs`,
`tests/command_sweep.rs`, `tests/import_fuzz.rs`). File export/import of machine formats goes
(`file.export.embroidery` and the embroidery import path in `fileio`).
Verify: `cargo test -p vectorcraft-engine` including `command_sweep` and `import_fuzz`.

**C. Tools and document model.** Delete `tools/src/embroidery.rs`, its catalog entries,
`cx.embroidery_beads`; remove `Node.embroidery`, `Document.embroidery` and the hit-test
widening in `doc/src/hit.rs`; drop `sf-core` from `doc` and `tools`. Add the old-file test.
Verify: `cargo test -p vectorcraft-doc -p vectorcraft-tools -p vectorcraft-format`.

**D. Crates and workspace.** Delete `sf-core`, `sf-formats`, `embroidery`,
`apps/stitchfoundry-cloud`, `packaging/cloud`; remove them from the workspace members,
`[workspace.dependencies]`, `xtask/src/layers.rs`, `Cargo.lock`, CI workflows that build the
cloud app, and packaging that mentions embroidery file types (MIME, desktop, metainfo, wxs,
nfpm). Fix `apps/vectorcraft` (`main.rs`, `build.rs`, `logging.rs`), `vectorcraft-cli`
(`perf.rs`), `vectorcraft-web` (`web.rs`) and `mcp/src/server.rs`.
Verify: full `cargo xtask ci` (fmt, clippy, tests, layers, assets, brands, wasm).

**E. Docs, catalogs, assets.** Delete the four embroidery docs; fix the links to them in
`README.md`, `ROADMAP.md`, `docs/*.md`, `CLAUDE.md`; remove translation rows used only by
embroidery strings from the 23 catalogs (rows for strings still shown stay);
remove embroidery images from `docs/images` and their `ASSETS.md` rows. Update
`docs/screenfoundry-spec.md` (what exists, decisions) and the revision history.
Verify: `cargo xtask assets brands`, link check by `rg`, full tests.

## Risks and how they are handled

| Risk | Handling |
|---|---|
| Something outside embroidery quietly uses `sf-core` (geometry, `i_overlay`, colour) | The compiler finds every use in stage B/C. A real shared need moves to the crate that wants it, not back into `sf-core`. |
| The i18n tests fail on removed or renamed strings | Stage A runs them; stale rows are harmless to the tests and are removed in E. |
| Document or preference files stop loading | Compatibility tests in stages B and C. |
| wasm breaks | `cargo xtask wasm` runs in stage D. |
| A stage is too big to review | One commit per stage; the diff is almost all deletions. |
| Disk and time (the engine test build alone takes about 10 minutes) | A separate `CARGO_TARGET_DIR`, deleted at the end; long runs go in the background. |

## Definition of done

`rg -i "embroider|sf_core|sf-core|sf-formats|stitch"` over source, docs and packaging finds
only: the product-name strings handled by the rename task, the unrelated uses listed above,
the Ink/Stitch namespace, and this plan. `cargo xtask ci` passes. A document saved by
StitchFoundry with embroidery data opens.

## After this

SC-0c rename to `screenfoundry` (binaries, window title, icons, packaging, CI, docs, MIME and
file extension with a legacy alias), then SC-0d PhotoCraft as a git dependency, then SC-2.
