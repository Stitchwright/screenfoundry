# 2026-10-21: `portal_save` test hangs the desktop app's test binary

Status: **fixed** (test code only; the app is unchanged).

## Symptom

`cargo xtask ci` stopped for over an hour in the desktop app's tests, on
`portal_save::tests::the_answer_is_awaited_on_the_path_the_portal_returns` ("has been running for
over 60 seconds", 0% CPU). Run alone the test always passed. The whole test binary hung in about
a third of runs (4 of 12, 5 of 20) and never when run with one thread (0 of 10); with two threads
1 of 10, with four 2 of 10. Earlier full CI runs had passed, and the committed HEAD hung just as
often with the day's changes stashed, so it was not caused by that work.

## Hypotheses

1. **The client misses the one-shot `Response` signal** (a 300 ms pause in the mock stands in for
   a person answering the dialog; a busy machine might subscribe later). Experiment: repeat the
   signal until the caller has had it. Result: still 5 hangs in 20. **Rejected.**
2. **The mock never sees the client's `SaveFile` call.** Experiment: stage markers with times in
   the mock and the test. Result: in a hung run one mock thread printed "mock thread started"
   but never "mock got SaveFile"; the client's call had arrived 4 ms before that thread began
   listening. **Confirmed.**

## Cause

`zbus`'s `MessageIterator` sees only messages that arrive after it is created. The test made it
inside the spawned thread, so on a busy machine the client's first call could arrive before the
thread started; the call was dropped, nobody replied, and the client's call waits for ever.

## Fix

Make the iterator in `mock_portal` before spawning the thread and move it in. Result: 0 hangs in
40 runs (the code before the fix hung in 4 of 12 and 5 of 20).

## Left behind

The hung runs leak their `dbus-daemon` child processes. Clear them with `pkill -x dbus-daemon`
before measuring.
