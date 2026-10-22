# 2026-10-22: `row_thumbnails_wait_for_the_end_of_a_drag` failed once

Status: **open, not reproduced.**

## Symptom

In one full `cargo xtask ci` run, `panels::layers::tests::row_thumbnails_wait_for_the_end_of_a_drag`
(`crates/ui-egui/src/panels/layers.rs`) failed:

```
assertion `left == right` failed: and stay
  left: 3
 right: 2
```

That is, a second frame re-rendered a row thumbnail that the first frame had already rendered.

## What was tried

- The test alone: 6 passes of 6.
- The whole `vectorcraft-ui-egui` test binary: 15 passes of 15.
- The next full CI run passed.

Nothing in the work being checked touched the Layers panel or thumbnails.

## Hypothesis (not tested)

The render counter (`THUMBS`) is thread-local. Tests share the worker threads of the test
runner, so a thumbnail cached or counted by an earlier test on the same thread could change what
a later test sees. A fix would reset the thread-local state at the start of the test, or key the
count by the test's own document.

## If it comes back

Run the whole binary with `--test-threads=1` and with the default several times, as in
`2026-10-21-portal-save-test-hang.md`, and see which test runs just before it on the failing
thread.
