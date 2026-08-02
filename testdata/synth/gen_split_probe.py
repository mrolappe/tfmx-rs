#!/usr/bin/env python3
"""Generates a minimal synthetic TFMX module whose macro 0 is a single
`$1C <Splitkey>` keysplit -- the probe fixture for zone resolution
(docs/m5-plan.md Phase 5.3).

Deliberately the simplest possible split: one threshold, on one axis, with
each half handing off to its own single-purpose instrument macro. A correct
zone resolver must report exactly two zones, cut at note $20, and name the
right `$06 <Cont>` target for each half.

Layout and conventions follow gen_minimal_scale.py; run from this directory:
    python3 gen_split_probe.py
"""
import math
import struct

OUT_DIR = "."

# -- smpl: 4 bytes of silence (see gen_minimal_scale.py), then two distinct
# single-cycle waves so the two keysplit halves also *sound* different --
NULL_SAMPLE_LEN = 4
L = 32
LOW_OFFSET = NULL_SAMPLE_LEN                 # a sine, for the low half
HIGH_OFFSET = NULL_SAMPLE_LEN + L            # a square, for the high half
sine = bytes(
    max(-127, min(127, round(100 * math.sin(2 * math.pi * i / L)))) & 0xFF
    for i in range(L)
)
square = bytes((100 if i < L // 2 else -100) & 0xFF for i in range(L))
with open(f"{OUT_DIR}/smpl.split-probe", "wb") as f:
    f.write(bytes(NULL_SAMPLE_LEN) + sine + square)

SPLIT_NOTE = 0x20

# -- macro 0: the keysplit itself, nothing else --
#   0: $1C Splitkey aa=$20 bbbb=$0002 -- note < $20 jumps to step 2
#   1: $06 Cont macro 1 step 0        -- note >= $20: the high instrument
#   2: $06 Cont macro 2 step 0        -- note <  $20: the low instrument
macro0 = bytes([
    0x1C, SPLIT_NOTE, 0x00, 0x02,
    0x06, 0x01, 0x00, 0x00,
    0x06, 0x02, 0x00, 0x00,
])


def instrument(sample_offset):
    """A minimal DMAon instrument over one 32-byte single-cycle wave."""
    return bytes([
        0x00, 0x00, 0x00, 0x00,             # DMAoff+Reset
        0x02, 0x00, 0x00, sample_offset,    # SetBegin (absolute)
        0x03, 0x00, 0x00, 0x10,             # SetLen 16 words = 32 bytes
        0x08, 0x00, 0x00, 0x00,             # AddNote (period from the note)
        0x01, 0x00, 0x00, 0x00,             # DMAon
        0x07, 0x00, 0x00, 0x00,             # STOP
    ])


macro1 = instrument(HIGH_OFFSET)
macro2 = instrument(LOW_OFFSET)

# -- pattern 0: four notes straddling the split, all through macro 0 --
NOTES = [SPLIT_NOTE - 2, SPLIT_NOTE - 1, SPLIT_NOTE, SPLIT_NOTE + 1]
WAIT_JIFFIES = 4
pattern0 = b"".join(
    bytes([0x80 | note, 0x00, 0xF0, WAIT_JIFFIES])  # macro 0, volume 15, voice 0
    for note in NOTES
)
pattern0 += bytes([0xF0, 0x00, 0x00, 0x00])  # $F0 End

# -- trackstep: one line, track 0 -> pattern 0, tracks 1-7 stopped --
trackstep_line = struct.pack(">H", 0x0000) + b"".join(
    struct.pack(">H", 0xFF00) for _ in range(7)
)

# -- assemble mdat (fixed layout: patterns $400, macros $600, tracksteps $800) --
mdat = bytearray(0x800 + len(trackstep_line))
mdat[0:10] = b"TFMX-SONG "
struct.pack_into(">H", mdat, 0x140, 0)   # song_end[0] = line 0
struct.pack_into(">H", mdat, 0x180, 3)   # tempo[0] = 3 -> 12.5 Hz
mdat[0x800:0x800 + len(trackstep_line)] = trackstep_line

struct.pack_into(">I", mdat, 0x400, len(mdat))
mdat += pattern0
for n, body in enumerate((macro0, macro1, macro2)):
    struct.pack_into(">I", mdat, 0x600 + n * 4, len(mdat))
    mdat += body

with open(f"{OUT_DIR}/mdat.split-probe", "wb") as f:
    f.write(mdat)

print(f"wrote mdat.split-probe ({len(mdat)} bytes) and smpl.split-probe")
