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
