# Macro/pattern fidelity: r-type macro 13 stuck on a static loop fragment

**Status: FIXED, TDD'd, ear-confirmed.** One new unconfirmed lead spun off (an onset blip in `render-macro` output not present in the real editor) — see the end of this doc.

[← index](macro-playback-fidelity.md)

---

## 17. FIXED, ear-confirmed: r-type macro 13's sweep was stuck on a static loop fragment — two stacked bugs

New report (2026-08-04, separate session): r-type macro 13 sounded "like a small sample fragment is
looped indefinitely," but the real macro (auditioned in the TFMX editor) sweeps through different
sample areas. `tfmx-cli disasm --macro 13` showed the mechanism: `$11 <AddBegin>` (periodic form,
`half_period=$FF=255`, `step=$FEE0=-288`) is the first instruction inside a `$05 <Loop>` body
repeated 254 times — the loop count almost exactly matches the vibrato's half-period, clear intent
for one continuous ramp across the whole loop, paired with `$12 <AddLen>` growing the region by +4
words every pass.

Two stacked bugs, both in `tfmx/src/macro_interp.rs`:

1. **Phase reset on re-arm** (the `$11` opcode handler): every re-execution of `$11` inside the loop
   rebuilt `PointerVibrato` with `t: 0`, so `PointerVibrato::delta()` always read 0 the instant it
   was called — the pointer never advanced past the very start of its ramp. Fixed: re-arming an
   already-running vibrato now updates `half_period`/`step` in place and preserves `t`.
2. **Deferred writes never reaching the loop target** (`MacroInterpreter::tick`): DMA turns on
   before the loop starts, so `Paula::set_sample_region`'s writes defer to the next natural wrap
   (`docs/playback-model.md` §2.3's double-buffering model, landed the session before this one,
   commit `fe48dc3`) — and `Voice::next_sample`'s wrap always reloads `loop_start`/`loop_len`, not
   whatever was most recently requested. Nothing kept `loop_start`/`loop_len` in sync with `$11`/
   `$12`'s changes absent an explicit `$18 <Sampleloop>`, so even with bug 1 fixed the ramp was still
   silently discarded at every wrap. Fixed: `loop_start`/`loop_len` now continuously mirror the live
   attack pointer (`sample_start`/`sample_len`) whenever `$18` hasn't run yet (`loop_active == false`).

TDD'd with `add_begin_periodic_form_keeps_ramping_when_re_armed_inside_a_loop`
(`tfmx/src/macro_interp.rs`), reproducing macro 13's exact shape — fails before either fix, passes
with both. Full workspace suite green except golden hashes, which moved for 4 modules that exercise
this general pattern (not just r-type): `r-type`, `turrican intro`, `turrican 2 title (st)`,
`x-out (title)`. **User ear-confirmed** on `render-macro --macro 13` before/after
(`testdata/synth/rtype-mac13-{before,after}.wav`) and the full song 0 in context
(`rtype-song0-{before,after}.wav`): macro 13 now sweeps through sample content as expected.

### New, unconfirmed lead: an audible "blip" at the very start of the fixed `render-macro` output

The user also heard a brief blip/click right at the start of `rtype-mac13-after.wav` that is **not**
present when auditioning macro 13 directly in the real TFMX editor. Explicitly flagged as "not sure
whether it is related" and to be recorded for a future investigation, not chased this session.

What's been ruled out already: a sample-by-sample diff of `rtype-mac13-before.wav` vs.
`-after.wav` shows the two renders are **byte-identical for the first 2033 frames** (~46ms, well
past the note's onset at frame 1764) — the fix does not change anything about the attack itself,
so if the blip is new (introduced by this session's fix rather than pre-existing and simply masked
before by the stuck-loop symptom), it isn't at the very first sample-region write. Not yet checked:
whether the blip is a `render-macro` tooling artifact (this tool has a known-real limitation, §10
above, for macros that depend on pattern-level retriggering — macro 13 is triggered once by
`render-macro` with no pattern context, unlike in the real song) vs. a genuine new engine bug from
this session's `loop_start`/`loop_len` sync change, vs. something pre-existing that was simply
harder to notice under the old stuck-loop symptom. First things to try in a fresh session: (a)
`render-pattern` instead of `render-macro` for the same macro, since it preserves real trigger
cadence and would rule out the tooling explanation; (b) locate the blip's exact sample offset (the
diff above only checked the first 2033 frames — extend it) and cross-reference against `tfmx-cli
trace --macro 13` to see which opcode is executing at that jiffy.

