# M5 session log

Append-only log for the "Export and static analysis" milestone (see
[`docs/m5-plan.md`](m5-plan.md)). Each entry: what was done, problems hit, **mistakes made and
how they were resolved**, and anything a future session would otherwise have to re-derive.
Write it honestly — wrong turns are the valuable part. One entry per phase, in phase order.

## Phase 5.0 — Idea ledger and session log

Wrote `docs/analysis-tooling-ideas.md` from `m5-plan.md`'s decision table, phase list and
"Deferred to the ledger" list, and this file's skeleton. Both linked from `CLAUDE.md`'s "Where
the knowledge lives". No code changed, no problems hit.

## Phase 5.1 — Reference register-log spike (WinUAE Memwatch)

**Positive result: the mechanism works.** WinUAE 6.1.0's debugger (Shift+F12 to enter) sets a
logonly memory watch with `w <slot> <addr> <len> <R/W/I> <F/C/L/N>`, e.g.
`w 0 dff0a0 40 W L` watches all of `$DFF0A0-$DFF0DF` for writes without breaking. Each hit
prints one line to the debugger's own console: `Memwatch <slot>: break at <addr>.<size> <RWI>
<value> PC=<pc> <accessmask> (<reg>)`. Decodes cleanly onto `AUD0-3 LCH/LCL/LEN/PER/VOL` by
`addr & 0xFF`, base `0xA0`/`0xB0`/`0xC0`/`0xD0` per channel, `+0x0/+0x2/+0x4/+0x6/+0x8` per
register.

**Wrong turn #1: stdout redirection does not capture this.** First attempt launched WinUAE
from a terminal with `> file.log 2>&1`, expecting the debugger console to go through the same
stream (the app's *general* logging does — confirmed separately when an accidental plain
launch printed ROM-scan messages to stdout). It doesn't: the log file only ever received the
one `write_log()`-based "watchpoint set" confirmation line. Read WinUAE's own source
(`debug.cpp` on GitHub, `tonioni/WinUAE`) to find out why: `memwatch_hit_msg()` prints hit
events via `console_out_f()`, a separate stream that only writes to the in-app debugger console
window/buffer — never through `write_log()`. There's no in-debugger toggle to redirect it.
**Workaround: copy the console text out by hand** (select-all + copy in the console window,
paste to a file) after a playback run. Fine for a spike; not something Phase 5.2+ can automate
without further work on capture (out of scope for this milestone as currently planned — noted
here so a future session doesn't have to re-discover the `console_out_f` vs `write_log` split).

**Wrong turn #2 (partial): the hit-message format has no timestamp at all.** Also confirmed by
reading `debug.cpp` — `mwhit`/`memwatch_hit_msg` carry address, size, R/W/I, value and PC only;
no vpos/hpos/frame counter. **Fix: add a second logonly watch on `$DFF09C` (INTREQ)**, e.g.
`w 1 dff09c 2 W L`. AmigaOS clears the VERTB interrupt-request bit (`0x0020`) once per 50 Hz
frame at a fixed PC in this build (`$00FC1354`); counting those hits in the same interleaved
console stream gives jiffy-resolution relative timing for free. Verified on a ~13.5s capture:
675 VERTB markers, spacing consistent with a steady 50 Hz source (line-gap between markers
varies with how many `AUD*` writes fall in that frame, not with the marker itself becoming
irregular).

**Decoded sample** (from a `the_house_of_techno` capture the user confirmed sounded correct by
ear; `jiffy` derived from the VERTB marker count, 0-indexed from the first captured registers):

```
jiffy=0  AUD0LCH  0x0002  PC=00C28FBE
jiffy=0  AUD0LCL  0xEB24  PC=00C28FBE
jiffy=0  AUD0LEN  0x1000  PC=00C28FC8
jiffy=0  AUD0PER  0x0168  PC=00C29162
jiffy=1  AUD0VOL  0x0020  PC=00C28EF0
jiffy=2  AUD0LCH  0x0002  PC=00C29360
...
```
4691 `AUD*` writes decoded across 673 jiffies from one capture. **Caveat, worth carrying
forward**: the value field is sometimes wider than a real 16-bit chip register should allow
(e.g. an `LCL` write showing `0x00023B71`) — appears to be upper garbage bits passed through by
WinUAE's memory-access hook on what is actually a word write; mask to the low 16 bits when
parsing. Also, the first ~16 events on this hardware are a `0xFFFE`/`0x0000` sweep pattern at a
PC outside the player's normal cluster — looks like a POST/init diagnostic, not music; anyone
resuming should skip past it rather than treat it as a decode bug.

**Also surfaced, not investigated further this phase**: the user reports WinUAE 6.1.0's own
playback of the TFMX editor is sometimes audibly wrong compared to fs-uae, and sometimes
correct, on the same module — nondeterministic or state-dependent in a way not yet understood.
The capture above is from a run confirmed correct by ear. **This is a real risk to the oracle's
trustworthiness** if pursued further: a register log captured during one of the "wrong" runs
would encode WinUAE's own bug, not ground truth, with no independent way (yet) to tell which
kind of run produced a given log. Not chased down in this timebox; flagging it for whoever
re-decides how far oracle work goes next.

No `tfmx`/`tfmx-cli`/`tfmx-analysis` code was written or changed this phase — a spike-only
parser (Python, ad hoc) was used to produce the decoded sample above and was not committed, per
the phase's "don't prematurely build into the crate structure" instruction. Raw captured logs
(WinUAE console text) were kept outside the repo (session-local), consistent with this
project's existing practice for other rendered/captured artifacts derived from the copyrighted
test corpus.

## Phase 5.2 — Static walker core (`tfmx-analysis`)

**User's re-decide call (session 16, before this phase started)**: treat the 5.1 spike's
positive result as sufficient and move straight to the static walker, rather than spending more
of this milestone automating Memwatch capture or chasing the WinUAE-vs-fs-uae playback
inconsistency. Both stay open, unblocked, for a future session that wants the register-log
oracle specifically.

**Two small, justified additions to the `tfmx` core** (not scope creep — both are existing
private logic the walker needed exposed, no new behavior):

- `Module::pattern_offset(n)` / `Module::macro_offset(n)`: the absolute `mdat` byte offset a
  pattern/macro's data starts at. `Module::pattern`/`macro_` only ever returned the byte slice,
  not where it began — fine for every existing consumer (they all just read forward from
  offset 0 of the slice), but the provenance map needs the absolute start to report a byte
  span. Refactored `pointer_table_entry` into `pointer_table_offset` (returns the `u32`) so
  both accessors and the new offset methods share one bounds-checked lookup. TDD'd against the
  same known corpus entries the existing `pattern_and_macro_access_known_file` test uses.
- `sequencer::decode_line` made `pub` and re-exported as `tfmx::decode_line`, mirroring
  `decode_pattern_entry`'s existing seam (stateless decode, no execution-state context) — the
  walker needed a way to turn a raw trackstep line's 16 bytes into `TrackstepLine` without
  pulling in `Sequencer`'s stateful trackstep runner.

**Design choice: the walker does not execute control flow, it lists linearly to the
terminator — same shape as `tfmx-cli disasm`.** Patterns are scanned from step 0 to
`$F0 End`/`$F4 Stop` (or a 256-step cap, mirroring `disasm`'s `MAX_DISASM_STEPS`); macros from
step 0 to `$07 STOP`. `$F1 Loop`/`$1C Splitkey`/`$1D Splitvol` branches are not followed —
their operands are read (so a `Jump`/`GoSub`/`PlayPattern` target pattern, or a `$06 Cont`/
`$15 Go submacro`/`$21 Play macro` target macro, is still queued as reachable) but the walk
does not jump to the branch target's *step*; every referenced pattern/macro number gets its
own from-step-0 scan when it's popped off the worklist. This means a pattern only ever reached
via `Jump{step: 40}` still gets scanned from step 0, not step 40 — an approximation, but the
same one `disasm` already makes, and it errs toward *more* provenance coverage, not less. Not
revisited this phase; would need real per-track program-counter simulation (closer to
`PatternRunner`/`MacroInterpreter`) to do exactly, and that is out of Phase 5.2's scope per
`m5-plan.md`.

**Sample-region tracking is best-effort, not zone resolution.** `$02 SetBegin`/`$03 SetLen`/
`$11 AddBegin` (only its `aa == 0` one-shot form — the oscillating `aa != 0` vibrato form isn't
resolved to a static offset)/`$12 AddLen`/`$18 Sampleloop`/`$19 Set one shot sample` update a
small `SamplePointer` struct that mirrors `macro_interp.rs`'s own bookkeeping (same absolute-
`$02`, halved-`$18`-delta units as the two macro-fidelity fixes already landed on `main`), and
every touch snapshots the "live" region (loop region once `$18` has run, else the plain sample
region) into `WalkResult::sample_regions`. This does **not** attempt `$1C`/`$1D` interval
splitting into note/velocity zones — that is Phase 5.3's job, the milestone's stated "spine".

**Corpus result** (`walk_song(module, 0)`, all 10 corpus modules, song 0 only):

```
turrican intro: 53 patterns, 25 macros reachable; provenance 6072/19108 bytes (31.8%)
turrican outside: 29 patterns, 8 macros reachable; provenance 2176/12252 bytes (17.8%)
r-type: 37 patterns, 14 macros reachable; provenance 2432/7432 bytes (32.7%)
x-out (title): 27 patterns, 10 macros reachable; provenance 4004/9116 bytes (43.9%)
turrican 2 title (st): 61 patterns, 39 macros reachable; provenance 8344/20340 bytes (41.0%)
turrican 2 level 1-desert: 48 patterns, 13 macros reachable; provenance 3572/13024 bytes (27.4%)
turrican 2 level 3-flight: 32 patterns, 14 macros reachable; provenance 3524/14328 bytes (24.6%)
turrican 3 level 1: 28 patterns, 11 macros reachable; provenance 6768/16732 bytes (40.4%)
apidya (title): 43 patterns, 22 macros reachable; provenance 4300/7056 bytes (60.9%)
apidya (level 1): 9 patterns, 10 macros reachable; provenance 1364/8148 bytes (16.7%)
```

`apidya (title)` is the only module whose raw voice nibbles include 4-7 with no 3 — the 7V
signature holds across the whole corpus, asserted in
`walker::tests::walks_all_corpus_modules_song_0_without_panic`. Coverage is deliberately far
from 100% and not treated as a bug: only song 0 is walked (most modules carry more than one
song slot, unexplored this phase), and header/pointer tables themselves are structural data,
never claimed by any pattern/macro span — matching the phase's own interpretation note in
`m5-plan.md` ("the signal is the delta across modules, not 100%").

No mistakes hit worth recording as wrong turns this phase — the two `tfmx` accessor additions
and the walker itself passed their tests on the first real corpus run once the one seeded test
bug (a hand-encoded `cv` byte with volume/voice nibbles swapped in
`reachable_patterns_and_macros_from_trackstep`) was caught by its own assertion and fixed.

## Phase 5.3 — Zone resolution (`$1C`/`$1D`)

Delegated to an Opus 5 agent (self-contained brief: opcode semantics from `docs/opcodes.md`,
the runtime `$1C`/`$1D` reference in `tfmx/src/macro_interp.rs`, the existing walker to extend).

New `tfmx-analysis/src/zones.rs`: `resolve_zones(module, macro_number) -> ZoneTable`, a
symbolic pass that interprets a macro's `$1C <Splitkey>`/`$1D <Splitvol>` branches over
intervals rather than concrete values, partitioning the whole `0..=$3F` (note) x `0..=64`
(entry volume) rectangle into disjoint zones, each carrying its live sample region, volume
register and envelope.

**The interval algebra**: DFS over paths, each carrying a `(note interval, entry-volume
interval)` rectangle plus accumulated state; splits cut the rectangle and empty halves are
pruned. `$1C` cuts the note axis directly, since no macro opcode ever writes the note register.
`$1D` compares the volume *register*, already touched by `$0D`/`$0E`/`$1E`, tracked as
`clamp(entry + offset, lo, hi)` -- three fields, not a single accumulated offset, because
clamping does not compose with addition (`$0D -10` then `$0D +10` leaves entry-volume 0 at 10,
not 0). `$0F <Envelope>` (time-varying volume) or a revisited step yields `ZoneExit::Unresolved`
rather than a guess. `walk_song` and its tests are untouched; `SamplePointer`/`sext24` widened to
`pub(crate)` for reuse rather than re-derived.

New fixture: `testdata/synth/gen_split_probe.py` (+ generated `mdat`/`smpl.split-probe`,
`testdata/synth/` un-ignored) -- a from-scratch macro with one `$1C` threshold, for a
known-boundary test independent of the real corpus.

**Check results**: `turrican intro` macro 28 (no `$1C`/`$1D` in its disasm) resolves to exactly
one full-rectangle zone matching that linear structure field-for-field; the probe macro resolves
to exactly two zones split at the right note boundary. Corroborating, not required by the check:
macro 24's real keysplit, macro 5's `$1D` chain, and a coverage test probing every macro of all
10 corpus modules to confirm every point lands in exactly one zone. 240 workspace tests pass
(9 new), clippy clean, `wasm32-unknown-unknown` build for `tfmx` unaffected (only `tfmx-analysis`
touched).

**Open finding, not acted on**: macro 5's `$1D` chain (`$0D +$15` then four `$1D`s at
`$20/$2A/$34/$3C`) reads as dead code (3 of 4 `Cont` targets unreachable) under the documented
"jump if volume < aa" polarity from `docs/opcodes.md:177`, but as a clean 5-way velocity-layered
fan-out under the reverse polarity ("jump if volume >= aa") -- suggestive the documented polarity
may be backwards. Not investigated further this phase since 5.3's check criterion is to match
*current* documented/runtime behavior, not to resolve fidelity questions; a fidelity thread issue
if picked up later. Test `zones::tests::turrican_intro_macro_5_splitvol_chain` documents today's
reading. Also noted: macro 28's `$0E <SetVolume>` has `aa=$00` (resolved volume 0, `cc=$38`
unused) -- mirrors the interpreter, not investigated.

**Follow-up (2026-08-02), while building an ear-check fixture (`testdata/synth/
gen_splitvol_probe.py`) for the polarity question above**: loading a synthetic `mdat` in the TFMX
editor whose trackstep table reserves only *one* real line (all 8 tracks `$FF00`/stopped) still
shows non-empty data in trackstep lines 1-5 after a full editor/emulator restart and reload. The
bytes it displays there are exactly the pattern/macro bytecode that happens to follow the
trackstep table in the file (confirmed by hexdump) -- the editor is not respecting the one-line
table boundary and is reading raw file bytes past it as more trackstep lines, rather than treating
them as absent/default. Not investigated further (out of scope for the polarity question), but
worth remembering when generating any synthetic fixture for editor loading: reserve several real,
explicitly-stopped trackstep lines (not just as many as the song logically uses), or the editor's
view will show misleading "phantom" song data past the last real line. Unclear yet whether this is
purely an editor-display quirk or hints at a real per-file minimum trackstep-table size assumption
worth checking against `docs/format.md`.

**RESOLVED (2026-08-02): the polarity finding above is a confirmed engine bug, now fixed.** The
ear-check fixture settled it against the real TFMX editor: a `$0D +0` primer then one `$1D`
threshold, triggered at two note volumes straddling it, played the *fallthrough* branch for the
*lower* volume and the *jump* branch for the *higher* volume -- the reverse of [S1]'s literal
"jumps if volume is less than `aa`". Also confirmed in the same test: `$1D` only reads a
meaningful volume once an explicit volume-setting opcode (`$0D`/`$0E`) has run since trigger --
a bare `$1D` as a macro's first opcode always took the jump regardless of the note's volume in
the real editor. This crate's `trigger()` seeds the volume register eagerly, so it never needed
that priming, and no real corpus macro observed so far puts `$1D` before an explicit `$0D`/`$0E`,
so that second point is a model-accuracy footnote, not a fix.

Fixed `tfmx/src/macro_interp.rs`'s `$1D` handler (`self.volume < b1` -> `self.volume >= b1`,
TDD'd -- test renamed `splitvol_jumps_only_when_volume_is_at_or_above_the_threshold`).
`tfmx-analysis/src/zones.rs`'s `$1D` branch mirrors the same flip (taken set is now a *suffix* of
the volume axis, not a prefix); `turrican_intro_macro_5_splitvol_chain` rewritten for the now-5-zone
ascending fan-out (quietest -> macro 4 ... loudest -> macro 0), matching the "clean fan-out" reading
the finding predicted. `docs/opcodes.md:177`'s row and a new note below its table record the
corrected polarity and the priming requirement, both with the real-hardware citation. 147 `tfmx` +
19 `tfmx-analysis` + full workspace tests pass, clippy clean, `wasm32-unknown-unknown` build for
`tfmx` unaffected. The `tfmx-cli` golden-hash regression suite is unchanged byte-for-byte -- traced
and confirmed macro 5 is never actually triggered within any corpus module's first 90 s of any
song (it's statically reachable in `turrican intro` song 2 per the static walker, but only via a
`$06 Cont` chain from macros never observed triggered in that window), so this fix has no
corpus-audible effect the existing regression net could catch either way; a real in-song A/B is
still open for whoever finds where macro 5's chain actually plays.

## Phase 5.4 — JSON dump + serialization seam

Wired up the `serde` feature `tfmx-analysis` had carried unused since Phase 5.2, and filled the
`TraceFormat` TODO at `tfmx-cli/src/main.rs:766`.

**`tfmx-analysis`**: `#[cfg_attr(feature = "serde", derive(serde::Serialize))]` added to every
public walker/zone type (`SpanKind`, `Span`, `SampleRegion`, `WalkResult`, `MacroVolume`,
`Envelope`, `ZoneExit`, `Zone`, `ZoneTable`) — feature stays optional and default-off, so this
touches nothing for callers that don't opt in. `RangeInclusive<u8>` (`Zone::notes`/`volumes`)
serializes via serde's built-in `Range`/`RangeInclusive` support, no extra code needed.

**`tfmx-cli`**: new `dump` subcommand (`--format json`, mirroring `trace`'s own
value-enum-with-one-variant-for-now seam) runs `walk_song` + `resolve_zones` for every reachable
macro and serializes the result. New `trace --format json`: `TraceEvent` lives in the
dependency-free `tfmx` core crate (hard rule, no `serde`), so its JSON encoding is hand-written in
a new `tfmx-cli/src/serialize.rs` (`write_json_event`, one `serde_json::json!` arm per variant,
ndjson — one object per line, mirroring `write_text_event`'s one-line-per-event shape) rather than
derived. `write_trace` gained a `format: TraceFormat` parameter and now switches on it internally
instead of `run_trace` doing the format dispatch, one function per format as the existing doc
comment already called for. `CliError` gained a `Json(serde_json::Error)` variant for
`dump`'s `serde_json::to_writer_pretty` call; `trace --format json`'s per-line writes can't
actually fail with a JSON error (the `Value` is already built), so it stays a plain
`std::io::Result` like `write_text_event`.

**Check results**: all 10 corpus modules' `dump --format json` output is valid, re-parseable JSON
with a non-empty `zones` array (new `dump_json_is_valid_and_has_zone_tables_across_full_corpus`
test, mirroring the existing `lint_runs_across_full_corpus_without_error` corpus-loop shape);
`cargo build -p tfmx-analysis --no-default-features` still compiles and exposes the same structs
(feature gate is additive-only). New tests, TDD-adjacent (written alongside implementation, not
strictly red-green-refactor since Sonnet did the wiring directly): `walk_result_serializes_to_valid_json`
and `zone_table_serializes_to_valid_json` in `tfmx-analysis` (gated `#[cfg(feature = "serde")]`,
needed a new `serde_json` dev-dependency), `write_trace_json_emits_one_valid_json_object_per_line`
and the corpus-wide dump test in `tfmx-cli`. 65 relevant tests pass (61 `tfmx-cli` + new
`tfmx-analysis` serde tests), full workspace suite green, clippy clean (one pre-existing unrelated
warning in `tfmx/tests/mutation_robustness.rs`, not touched), `wasm32-unknown-unknown` build for
`tfmx` unaffected (only `tfmx-analysis`/`tfmx-cli` touched).

Not done, out of scope for this phase: `dump`'s output is one song's walk plus that song's
reachable macros' zone tables — dumping every song, or every macro regardless of reachability,
was not asked for by the phase brief and would be speculative scope.

## Phase 5.5 — MIDI export

`tfmx-cli export-midi`, per the phase brief in `docs/m5-plan.md`: a JSON mapping keyed on
`(macro, note range, velocity range) → program | drum note | drop`, auto-drafted from 5.3's zone
tables and hand-editable, driving a note event stream built from an actual song trace (the same
`Player::render_traced` seam `trace` uses), written out as a Standard MIDI File via `midly`.

**New `tfmx-cli/src/midi_mapping.rs`**: `MidiMapping`/`MacroMapping`/`MappingZone`/`ZoneOutput`
(serde `Serialize`+`Deserialize`, unlike `tfmx-analysis`'s serialize-only types — this file is
meant to be loaded back after a hand-edit). `draft_mapping(module, &WalkResult)` calls
`tfmx_analysis::resolve_zones` for every reachable macro and defaults each zone's output to
`Program { program: macro_number }`, transpose 0 — a starting point, not a final answer, per the
plan's own framing.

**New `tfmx-cli/src/midi.rs`**: `build_events(trace: &[TraceEvent], &MidiMapping) -> Vec<MidiEvent>`
walks a trace chronologically, one absolute MIDI tick per `Jiffy` event — trivially satisfies the
plan's "PPQ chosen so 1 jiffy = an exact integer tick count" (it's exactly 1), with real
wall-clock accuracy coming from a MIDI tempo meta event emitted whenever the trace's own `Jiffy.
tempo` changes (`docs/playback-model.md` §3.2's `50/(v+1)` jiffy rate → microseconds/quarter,
scaled by the fixed header `PPQ`). Each `Trigger` becomes one `NoteOn` (looked up through the
mapping's zone for `(macro_number, note, volume)`; `Drop` emits nothing, `Drum` fixes the MIDI
note and routes to the GM percussion channel, `Program` computes the MIDI note from this crate's
own `MIDDLE_C_NOTE = 0x18 → MIDI 60` anchor plus the trace's `transpose` plus the zone's own
hand-authored transpose); a voice's previous note is explicitly closed first on retrigger, and any
still-sounding note gets a final `NoteOff` at the trace's end. Channel = voice number for pitched
output, drums share the GM channel 10. `write_smf` converts the absolute-tick list to `midly`'s
own relative-delta `Track` and writes a Format-0 (single-track) SMF.

**Vibrato/portamento → pitch bend**: rather than decoding `$0B`/`$0C` specifically, every voice's
`Voice.period` (already traced every jiffy) is compared against a per-trigger reference (the
period the first jiffy it goes nonzero after a `Trigger`) — any deviation, from whichever opcode
caused it, becomes a 14-bit pitch bend (`period` is inversely proportional to frequency, same
relationship as `tfmx/src/macro_interp.rs`'s `note_period`). A wide ±24-semitone bend range is set
once per channel via RPN 0 so portamento glides spanning more than an octave don't clip; bend
resets to center on every new trigger.

**`tfmx-cli` CLI wiring**: `render_trace` extracted out of `run_trace` (previously inlined) so
`export-midi` can share the exact same trace-collection loop rather than duplicating it —
`ExportMidiArgs` mirrors `TraceArgs`' `song`/`seconds`/`gate` shape, plus `-o`/`--output` and an
optional `--mapping <path>`: if the path doesn't exist yet it's auto-drafted and written there for
hand-editing on the next run; omitted entirely, the mapping is auto-drafted in memory only.

**Check results**: `export_midi_produces_valid_midi_matching_trigger_count_across_full_corpus`
(new, mirrors `dump_json...`'s corpus-loop shape) confirms all 10 corpus modules export MIDI that
re-parses via `midly::Smf::parse`, with note count matching the trace's own `Trigger` count exactly
both before and after the tick-delta round trip through `midly`.
`editing_the_mapping_changes_the_exported_notes` confirms dropping a triggered macro's zones
removes notes from the export. 19 new tests total, written test-first per this project's TDD rule
(5 in `midi_mapping.rs`, 12 in `midi.rs`, the 2 corpus-level ones above in `main.rs`), full
workspace suite green (80 `tfmx-cli` tests total, up from 61), clippy clean (the same one
pre-existing unrelated warning as Phase 5.4, not touched),
`wasm32-unknown-unknown` build for `tfmx` unaffected (only `tfmx-cli` touched; `midly` added as a
`tfmx-cli`-only dependency with `rayon`/`parallel` off).

Not done, out of scope for this phase: the actual DAW-open/listen half of the check criterion (a
corpus module opens in a real cross-platform DAW/player) — structurally verified via `midly`'s own
parser, but per this project's own standing rule, structural validation is not the same as ears on
it; flagged here for whoever picks up the ear-check. `$08`/`$09`/`$1E`/`$1F` macro-internal note
changes are not decoded into their own `NoteOn`s — only the pattern-level `Trigger`'s note is
mapped, and any pitch movement from those opcodes shows up as pitch bend instead (same mechanism as
vibrato/portamento, since it's driven off the observed period, not the opcode).

## Phase 5.6 — Fidelity scoreboard

New `tfmx-cli fidelity-scoreboard` subcommand, per `docs/m5-plan.md`'s Phase 5.6 brief: batch-render
the corpus, score it against reference material, write a tracked metrics file. Phase 5.1's WinUAE
register-log spike was ruled unusable as an automated oracle (manual copy-paste capture, unresolved
trust risk — see that phase's own entry above), so this uses the "audio metrics otherwise" branch
the plan called for.

**Design**: for each of the 10 corpus modules, render this crate's own output via the existing
`render_to_wav` (same function `render` uses) and a reference render via `uade123` (a GPL reference
player *executed*, not read from — the hard rule in `CLAUDE.md` explicitly permits this), then feed
both WAVs through the *existing* `detect_onsets`/`inter_onset_intervals`/`pearson_correlation` and
`measure_pitch_hz` functions `onset-diff`/`measure-pitch` already had — extracted into a new pure
`compute_module_fidelity` function (no file I/O) so the metric math is unit-testable without a
reference player or the corpus on disk. `render_reference_wav` shells out to `uade123 -1 -s <song>
-t <seconds> -f <wav> <mdat>`, with its child stdout/stderr discarded — `-f` still emulates in real
time and prints a continuous "Playing time position" progress line regardless of file output, which
would otherwise flood this tool's own stdout.

**Tracked file**: `docs/fidelity-scoreboard.json`, one `ModuleFidelity` object per module
(`onset_correlation`, `our_pitch_hz`, `reference_pitch_hz`) plus a `honesty_note` field carrying the
plan's own honesty requirement verbatim into the artifact itself, not just this log.

**Finding, documented in the honesty note rather than silently reported**: `our_pitch_hz`/
`reference_pitch_hz` came back a constant, degenerate ~8820Hz (`= sample_rate / 5`, the detector's
own minimum allowed lag) on 9 of the 10 modules, for *both* sides — autocorrelation over a whole
30-second dense polyphonic mix collapses toward the shortest allowed lag rather than tracking a real
note, the same failure mode this project's fidelity-thread history already flagged for `measure-
pitch` at full-mix/voice-solo scope (§11 of `docs/macro-playback-fidelity.md`: "not trusted as a
real single-note pitch measurement at that scope"). Rather than quietly shipping a meaningless
number, the scoreboard's `honesty_note` says so explicitly. `onset_correlation` does not share this
problem — it ranges informatively from -0.37 to 0.99 across the corpus, `turrican 2 level 3-flight`
the only module currently near 1.0.

**Check results**: `fidelity_scoreboard_runs_across_full_corpus_without_error` (new, mirrors the
existing corpus-loop tests, skips CI-safely if `uade123` or the corpus is missing) actually ran the
full batch in this session (both `uade123` and the corpus were present) and confirmed all 10 modules
produce valid JSON. `compute_module_fidelity_mutation_moves_onset_correlation` is the plan's own
"a deliberate known-bad mutation moves the metric" check: an identical-rhythm comparison correlates
at ~1.0, a comparison against a deliberately re-clustered onset rhythm drops by >0.5 — both written
test-first per this project's TDD rule. 3 new tests total, full workspace suite green (83
`tfmx-cli` tests, up from 80), clippy clean (the same one pre-existing unrelated warning as prior
phases, not touched), `wasm32-unknown-unknown` build for `tfmx` unaffected (only `tfmx-cli`
touched, no new dependency — `uade123` is invoked as an external process, already a corpus-fetch/
A/B tool this project relies on, not a crate dependency).

Not done, out of scope for this phase: no attempt to make `onset_correlation` itself more
informative (e.g. per-voice comparison) — the plan scoped this phase to reusing the existing
metrics, not improving them; per-voice onset detection is already recorded as a known ceiling on
`detect_onsets` itself, not reopened here.

## Phase 5.7 — Sample and sampler-instrument export

New `tfmx-cli export-instruments` subcommand and a new `tfmx-cli/src/export/` module, per
`docs/m5-plan.md`'s Phase 5.7 brief: one `InstrumentSerializer` trait over 5.3's zone table, three
formats registered by name (`export::by_name`, `export::FORMAT_NAMES`) — `wav`, `sfz`, `dspreset` —
each its own file (`wav.rs`, `sfz.rs`, `dspreset.rs`) so a fourth format is one new file plus one
registry line, per the plan's own structure-first instruction. Kontakt `.nki` and Ableton `.adg`
stayed ruled out, per the plan (encrypted/undocumented, and both covered by SFZ-via-sfizz already).

**Design**: `export::build_instrument(module, macro_number)` resolves the macro's `ZoneTable`
(`tfmx_analysis::resolve_zones`) and, for every zone carrying a sample region, fetches its live PCM
via `Module::sample` — zones with no sample region (a keysplit handing off to another macro) or an
out-of-bounds region are skipped per-zone rather than failing the whole instrument, mirroring `lint`'s
existing `sample-region-out-of-bounds` finding being a per-zone, not per-module, problem. Each zone's
note/volume rectangle maps onto MIDI key/velocity ranges via the same anchor/formula `midi.rs`'s MIDI
export already established (`MIDDLE_C_TFMX`/`MIDDLE_C_MIDI`/`velocity_for`, promoted to `pub(crate)`
so `export` reuses them instead of re-deriving the pitch anchor a second place) — every exported
sample is written at TFMX's own native rate (8363 Hz, the same `MIDDLE_C_HZ` `macro_interp.rs` plays
raw note `0x18` at), so MIDI note 60 is always the pitch-keycenter regardless of which zone it came
from, and a sampler resamples the rest of the key range from there.

**`SampleRegion` extended with a `looped: bool` field** (`tfmx-analysis/src/walker.rs`), threaded
from `SamplePointer`'s existing (previously module-private) `loop_active` flag via a new
`is_looped()` accessor. Without it, every exported zone would have to guess one-shot vs.
indefinite-sustain-loop — a real correctness gap, not a speculative one, since a wrong guess either
silences a one-shot's tail (loop mode where none was armed) or leaves a sustained pad decaying to
silence (no loop where `$18 <Sampleloop>` really was armed). Contained to `tfmx-analysis`: nothing
in `tfmx-cli` outside the new `export` module read `SampleRegion`'s fields, so the six call sites
(two production, four test) were the whole blast radius. TDD'd against real corpus data: `zones.rs`'s
existing `turrican_intro_macro_28_is_a_single_unsplit_zone` (has `$18` in its chain) now pins
`looped: true`, and `a_macro_without_splits_is_one_full_range_zone` (no `$18`) pins `looped: false`.

**WAV writer is hand-rolled RIFF**, not layered on `hound` (this crate's other WAV writer, used for
rendered audio): `hound` finalizes a plain `fmt `/`data` file with no support for extra chunks, and
patching one on after the fact would mean hand-fixing its RIFF size fields anyway — no simpler than
writing the ~90 lines directly. `fmt `/`data` always; a `smpl` chunk (one loop record, whole clip)
only when the zone's `looped` flag is set, so a one-shot zone's WAV carries no loop metadata at all
rather than a vacuous full-clip loop. 8-bit PCM's unsigned/128-bias convention (WAV's own quirk for
that bit depth) is applied by hand the same way `hound`'s own `Sample for i8` impl does.

**Check results — structural and independently-verified, not tool-in-the-loop** (no sfizz, Kontakt,
or DecentSampler installed in this environment; the plan's own check criterion needs those to fully
close, so this is disclosed as a gap rather than claimed done). What *was* verified: 10 new unit
tests (TDD'd) covering `build_instrument` against real corpus data (macro 28 of `turrican intro`,
matching `tfmx-analysis`'s own pinned zone), the WAV writer's loop points via a hand-rolled parser
round-trip, and the SFZ/`.dspreset` text output's region/sample count and attribute values. Beyond
this crate's own tests: `export-instruments` was run end-to-end against `turrican intro` song 0 for
all three formats (24 instruments each), the resulting WAV was checked with macOS's own `afinfo`
(confirms valid 8-bit/8363 Hz PCM independently of this crate's parser) and a `.dspreset` file was
checked with `xmllint --noout` (well-formed XML) — both external tools, not this crate re-checking
its own output. Full workspace suite green (90 `tfmx-cli` tests, up from 83), clippy clean (the same
one pre-existing unrelated warning as prior phases, not touched — two new clippy findings in the new
code, `write!`-ending-in-`\n` and a manual `% 2` check, were fixed rather than left), `wasm32-
unknown-unknown` build for `tfmx` unaffected, golden hashes unchanged (this phase adds export and
touches no playback path).

Not done, explicitly out of scope: nobody has loaded the exported files in real sfizz, Kontakt, or
DecentSampler — the phase's own check criterion needs that and it is not something this environment
can automate; whoever has those tools available should do that pass before treating 5.7 as fully
closed in the tool-verification sense, not just the structural sense recorded here.
