# 2026-10-21: `filled_faces_are_the_pathfinder_regions` fails on a random input

Status: **open, not fixed.** Found by the randomised test during a full `cargo xtask ci`; not
caused by the SC-2 work (nothing in `crates/pathops` has changed since the seed commit).

## Symptom

`planar::tests::filled_faces_are_the_pathfinder_regions` (`crates/pathops/src/planar.rs`, a
`proptest` property) failed once in about a dozen full runs:

```
Test failed: 1599.9999999999993 vs 1757.1236166328254 at crates/pathops/src/planar.rs:1783.
```

That is, for closed shapes alone the total area of the filled faces differed from the area of
the Pathfinder regions by about 157 square points (around 10%), far above rounding error.

## Why it looked intermittent

proptest draws fresh random shapes each run, so most runs never hit the bad case. When it does,
proptest saves the failing case to `crates/pathops/proptest-regressions/planar.txt` and replays
it on every later run, so the test then fails 6 of 6 times. The saved file below reproduces it
(copy it back to that path and run
`cargo test -p vectorcraft-pathops --lib filled_faces_are_the_pathfinder_regions`). The file was
**not** committed: committing it would make CI fail until the bug is fixed.

## Saved case

```
# Seeds for failure cases proptest has generated in the past. It is
# automatically read and these particular cases re-run before any
# novel cases are generated.
#
# It is recommended to check this file in to source control so that
# everyone who runs the test benefits from these saved cases.
cc 7344c041c55995ae3d001bd113661b3cfed3486e5f3670df586615af627c1c39 # shrinks to shapes = [Shape { path: PathData { subpaths: [SubPath { anchors: [Anchor { p: (20.0, 55.0), h_in: (20.0, 57.76142374915397), h_out: (20.0, 52.23857625084603), kind: Smooth }, Anchor { p: (25.0, 50.0), h_in: (22.23857625084603, 50.0), h_out: (27.76142374915397, 50.0), kind: Smooth }, Anchor { p: (30.0, 55.0), h_in: (30.0, 52.23857625084603), h_out: (30.0, 57.76142374915397), kind: Smooth }, Anchor { p: (25.0, 60.0), h_in: (27.76142374915397, 60.0), h_out: (22.23857625084603, 60.0), kind: Smooth }], closed: true }] }, rule: EvenOdd, key: 0 }, Shape { path: PathData { subpaths: [SubPath { anchors: [Anchor { p: (10.0, 30.0), h_in: (10.0, 30.0), h_out: (10.0, 30.0), kind: Corner }, Anchor { p: (50.0, 30.0), h_in: (50.0, 30.0), h_out: (50.0, 30.0), kind: Corner }, Anchor { p: (50.0, 70.0), h_in: (50.0, 70.0), h_out: (50.0, 70.0), kind: Corner }, Anchor { p: (10.0, 70.0), h_in: (10.0, 70.0), h_out: (10.0, 70.0), kind: Corner }], closed: true }] }, rule: NonZero, key: 0 }, Shape { path: PathData { subpaths: [SubPath { anchors: [Anchor { p: (30.0, 55.0), h_in: (30.0, 57.76142374915397), h_out: (30.0, 52.23857625084603), kind: Smooth }, Anchor { p: (35.0, 50.0), h_in: (32.23857625084603, 50.0), h_out: (37.76142374915397, 50.0), kind: Smooth }, Anchor { p: (40.0, 55.0), h_in: (40.0, 52.23857625084603), h_out: (40.0, 57.76142374915397), kind: Smooth }, Anchor { p: (35.0, 60.0), h_in: (37.76142374915397, 60.0), h_out: (32.23857625084603, 60.0), kind: Smooth }], closed: true }] }, rule: EvenOdd, key: 0 }]
```

## Next steps

1. Reproduce with the file above and read the shrunk input.
2. Decide whether `live_paint` (faces) or the Pathfinder regions is wrong for that input
   (self-touching or overlapping closed shapes are the likely cause).
3. Fix, keep the case as a regression test, and commit the proptest regression file with it.
