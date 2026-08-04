# Macro/pattern playback fidelity: index

Started 2026-07-31, with the trackstep-timing fix (`docs/trackstep-timing-bug.md`) code-complete
but not yet confirmed by ear. While preparing for that listen, the user cross-checked individual
macros against the TFMX editor's own macro-audition feature and found real discrepancies,
independent of the trackstep-timing work. What started as one document grew to 18 sections
spanning many distinct bugs across several corpus modules; it was split 2026-08-04 into one
document per distinct issue, linked below, so a fresh session can load only the thread it needs
instead of the whole history.

**▶ START HERE (2026-08-04): next session's task.** Both open theories in
[macro-fidelity-08](macro-fidelity-08-pattern52-note-durations.md) are now settled (editor ground
truth + a docs re-derivation) — the `dma_on`-based sustain heuristic in `note_on` is confirmed the
wrong invariant. Next: redesign it without reopening the three other cases it's load-bearing for.
See that doc's own "START HERE" for the first concrete step (`docs/status.md`'s original rationale).

## Open threads (need work)

- [**macro-fidelity-01**: portamento-to-note pattern records silently dropped](macro-fidelity-01-portamento-drop.md) — OPEN, design question unresolved.
- [**macro-fidelity-02**: pattern 0x54/macros 0x30-0x31 (voice 2) chorus effect](macro-fidelity-02-pattern54-chorus.md) — tempo report resolved; macro-internal pulse rate ~3.5x too slow is a real bug, **PAUSED** pending a clock-domain investigation.
- [**macro-fidelity-06**: `$0B <Portamento>` direction bug (r-type, macro 4)](macro-fidelity-06-portamento-direction.md) — OPEN, root cause NOT found; the documented model itself is contradicted by direct editor evidence.
- [**macro-fidelity-08**: pattern 0x52/macro 0x1c (macro 28) wrong note durations](macro-fidelity-08-pattern52-note-durations.md) — root cause found, ear-confirmed, fix NOT chosen. **Current chosen-next-step target.**

## Resolved / fixed threads (reference only)

- [**macro-fidelity-03**: pattern 0x6b/macros 0x0a, 0x27 — `$04 Wait` zero-jiffies bug](macro-fidelity-03-macro0a-wait-bug.md) — FIXED, TDD'd.
- [**macro-fidelity-04**: Paula attack→loop handoff undone every jiffy, + differential-render method](macro-fidelity-04-paula-handoff.md) — FIXED, TDD'd; also documents the three-scope differential-render recipe reused throughout this investigation.
- [**macro-fidelity-05**: the macro-28 pitch/out-of-bounds saga — `MIDDLE_C_NOTE` root cause](macro-fidelity-05-macro28-pitch-saga.md) — RESOLVED, ear-confirmed. The long multi-session thread (original §5–§14) that chased "pitch off, playhead wanders" through several real but audibly-inert fixes before finding the actual cause.
- [**macro-fidelity-07**: r-type macro 13 stuck on a static loop fragment](macro-fidelity-07-rtype-macro13-loop.md) — FIXED, TDD'd, ear-confirmed. One unconfirmed onset-blip lead spun off, not chased yet.

## Shared tooling

- [**macro-fidelity-tooling**: `render-macro`, `render-pattern`, and their gotchas](macro-fidelity-tooling.md) — referenced by nearly every doc above; read the gotchas before re-reporting a "render-macro produces silence" symptom.

## Cross-cutting notes

- None of this thread blocks or is blocked by the trackstep-timing golden-hash work
  (`docs/trackstep-timing-bug.md`) — independent bugs found via the same editor cross-check session.
- Every audio-affecting fix in this investigation follows the project's standing rule: not done
  until TDD'd *and* the user's ears have confirmed it, per [[feedback_verify_audio_before_claiming_done]]
  and [[feedback_tdd_required]] (see `CLAUDE.md`).
