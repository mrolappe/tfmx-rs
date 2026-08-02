#!/usr/bin/env python3
"""Generates a minimal synthetic TFMX module whose macro 0 is a single
`$1D <Splitvol>` velocity split -- an ear-check fixture for the polarity
question raised in ROADMAP.md's Phase 5.3 finding (`turrican intro` macro
5's `$1D` chain reads as dead code under the documented "jump if volume <
aa" polarity, but as a clean fan-out under the opposite polarity).

Both branches play the same sine sample but a full octave apart (via
`$08 <AddNote>` transpose, not sample length -- a sample-length trick made
`measure-pitch` lock onto a harmonic instead of the fundamental on an
earlier attempt at this fixture; `measure-pitch` also mismeasures *this*
version, apparently confused by the DMAon attack transient -- verify by ear
or a zero-crossing count over frames [10000:30000], not `measure-pitch`).
Trigger macro 0 directly with `tfmx-cli render-macro` at a volume below and
above `$20` and listen for which octave plays each time; then load this
same mdat/smpl pair in the TFMX editor (ground truth,
docs/tfmx-editor-as-ground-truth.md) and audition macro 0 at the same two
volumes. If the editor swaps which octave plays relative to this crate's
render, the doc's `$1D` polarity is indeed backwards and
`tfmx/src/macro_interp.rs`'s `self.volume < b1` should flip.

Layout and conventions follow gen_split_probe.py; run from this directory:
    python3 gen_splitvol_probe.py
"""
import math
import os
import struct

# Anchored to the script's own directory, not the invocation cwd -- running
# this as `python3 testdata/synth/gen_splitvol_probe.py` from elsewhere in
# the repo silently wrote stray output into the caller's cwd twice before.
OUT_DIR = os.path.dirname(os.path.abspath(__file__))

# -- smpl: 4 bytes of silence (see gen_minimal_scale.py), then one 32-sample
# sine cycle -- both branches play this same sample, only the transpose
# differs, so the pitch difference comes from note_period() alone --
NULL_SAMPLE_LEN = 4
L = 32
TONE_OFFSET = NULL_SAMPLE_LEN
sine = bytes(
    max(-127, min(127, round(100 * math.sin(2 * math.pi * i / L)))) & 0xFF
    for i in range(L)
)
with open(f"{OUT_DIR}/smpl.splitvol-probe", "wb") as f:
    f.write(bytes(NULL_SAMPLE_LEN) + sine)

SPLIT_VOL = 0x20

# -- macro 0: a priming $0D +0 ahead of the splitvol itself --
#   0: $0D AddVolume aa=$00           -- no-op arithmetically, but per
#      docs/opcodes.md:161 this is the opcode that "loads the result into
#      the volume register" -- an editor ear-check surfaced that a bare
#      $1D as macro 0's first opcode (no preceding $0D/$0E) always takes
#      the jump regardless of the note's actual volume, unlike this
#      crate's own model (which seeds the volume register at trigger
#      time). Real turrican intro macro 5 always opens with a real $0D
#      before its own $1D chain -- this primer tests whether that's why.
#   1: $1D Splitvol aa=$20 bbbb=$0003 -- volume < $20 jumps to step 3
#   2: $06 Cont macro 1 step 0        -- volume >= $20 (documented polarity): no transpose
#   3: $06 Cont macro 2 step 0        -- volume <  $20 (documented polarity): -12 semitones
macro0 = bytes([
    0x0D, 0x00, 0x00, 0x00,
    0x1D, SPLIT_VOL, 0x00, 0x03,
    0x06, 0x01, 0x00, 0x00,
    0x06, 0x02, 0x00, 0x00,
])


def instrument(transpose):
    """A minimal DMAon instrument over the sine cycle, transposed by `aa`
    semitones ($08 <AddNote>, signed byte)."""
    return bytes([
        0x00, 0x00, 0x00, 0x00,                  # DMAoff+Reset
        0x02, 0x00, 0x00, TONE_OFFSET,            # SetBegin (absolute)
        0x03, 0x00, 0x00, L // 2,                 # SetLen (words)
        0x08, transpose & 0xFF, 0x00, 0x00,       # AddNote (period from note+transpose)
        0x01, 0x00, 0x00, 0x00,                   # DMAon
        0x07, 0x00, 0x00, 0x00,                   # STOP
    ])


macro1 = instrument(0)     # no transpose: the reference octave
macro2 = instrument(-12)   # one octave down

# -- trackstep: one line, every track stopped -- render-macro never reads
# it, but the fixed layout still puts it at $800 right where the macro
# bodies would otherwise land, and the TFMX editor *does* read it. Reserve
# it for real, all tracks stopped, so the editor shows an empty song
# instead of misdecoded macro bytecode. --
trackstep_line = b"".join(struct.pack(">H", 0xFF00) for _ in range(8))

# -- pattern 0: a genuinely empty pattern ($F0 End immediately). A zero
# pattern-pointer table entry does NOT mean "no pattern" -- it means
# "pattern 0 starts at mdat byte 0", i.e. the file header/magic string,
# which the editor happily (mis)decodes as pattern data. Give pattern 0 a
# real body and a real pointer so the editor shows an actually-empty
# pattern instead. Patterns 1-127 are unused by this fixture and keep the
# same zero-pointer wart -- harmless unless the editor's pattern browser is
# paged all the way there. --
pattern0 = bytes([0xF0, 0x00, 0x00, 0x00])

# -- assemble mdat (fixed layout: pattern pointer table $400-$600, macro
# pointer table $600-$800, trackstep table starts at $800) --
mdat = bytearray(0x800 + len(trackstep_line))
mdat[0:10] = b"TFMX-SONG "
struct.pack_into(">H", mdat, 0x180, 3)   # tempo[0] = 3 -> 12.5 Hz
mdat[0x800:0x800 + len(trackstep_line)] = trackstep_line

struct.pack_into(">I", mdat, 0x400, len(mdat))
mdat += pattern0

for n, body in enumerate((macro0, macro1, macro2)):
    struct.pack_into(">I", mdat, 0x600 + n * 4, len(mdat))
    mdat += body

with open(f"{OUT_DIR}/mdat.splitvol-probe", "wb") as f:
    f.write(mdat)

print(f"wrote mdat.splitvol-probe ({len(mdat)} bytes) and smpl.splitvol-probe")
