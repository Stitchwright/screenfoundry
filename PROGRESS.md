# Progress

- 2026-10-30: Dependabot bumps fixed on `fix/dependabot-ci` (fax, jpeg-encoder, moxcms, aes/md-5/sha2, x11rb, skrifa, write-fonts, usvg/resvg, hayro-ccitt, patch group); `cargo xtask ci` passes.
- Left out on purpose (API rewrites), now held by ignore rules in `.github/dependabot.yml`: zbus 6 (blocking API removed), harfrust 0.15, vello_cpu/vello_common 0.3; egui and winit stay on the storytold forks.
- 2026-10-31 nightly maintenance: queue empty; added unit tests for `sc-core` pixel containers (`image.rs`: size caps, overflow, bad buffers, bounds, mirror). Tests only.
- Full `cargo xtask ci` hit "no space left on device" late in the run (after all test suites passed); disk-full, not a code failure.
- 2026-11-01 nightly maintenance: queue empty; full `cargo xtask ci` failed once at `test` on a flaky render test (`a_ready_bitmap_draws_again_where_its_file_shows`, passes alone and 4x in the crate); later CI steps not reached. Added `pdfout.rs` tests (xref offsets, stream length, unreserved ids). Tests only.
- 2026-11-01 nightly sweep: no needs-triage issues; history secret scan clean; cargo audit unchanged (#19). Filed #23 and fixed it here: `screen.dtf` gang sheets and `artboard_art` now refuse over-cap sizes before rendering.
- Main: `cargo xtask ci` all green except wasm (target missing here; passes once installed): 11,037 tests pass.
- 2026-11-02 nightly maintenance: queue empty; `cargo xtask ci` showed no failures (cut off by my 50-min timeout during the slow fuzz tests); sc-core tests stable over 3 runs. Added `pdfout.rs` tests (xref offsets, trailer, bad ids, stream length). Tests only.
- 2026-11-03 nightly sweep: no needs-triage issues; history secret scan clean; cargo audit unchanged (#19). Filed #32 and fixed it here: `screen.*` commands refuse a source image over the 100 MP cap from its header, before decoding it.
- 2026-11-04 nightly maintenance: queue empty; `cargo xtask ci` cut off by the 30-min background limit with no failures seen; sc-core tests stable over 3 runs (130 pass). Added `color.rs` tests (grey chroma, delta-E symmetry, hex edges). Tests only.
- 2026-11-04: flaky render test `a_ready_bitmap_draws_again_where_its_file_shows` fixed: a background worker could make both bitmaps before the first frame finished, so the frame already showed them ("the bitmaps show" failed, e.g. on PR #37). The test now holds the workers (test-only `hold_workers`) while it draws that frame.
- 2026-11-07: issue #43 branding: customer-facing "SiteSentrix" -> "StitchFoundry" (About text + i18n catalogs, Windows/Linux packaging, exe CompanyName). Legal files (LICENSE, NOTICE, copyright strings, ASSETS.md) left for Bruce. Inflected translations (e.g. fi, hu) got a plain word swap; a native check is worthwhile.
