#!/usr/bin/env python3
"""Generates a minimal synthetic TFMX module isolating `$0B <Portamento>`'s
sign convention: does a *positive* `bb` bend pitch up or down? Built to
settle a user-reported direction mismatch (r-type pattern 8 / macro 4)
against `uade123` as ground truth, independent of that macro's own
AddNote-then-glide complexity -- see docs/macro-playback-fidelity.md.

One held note, `$0B <Portamento> aa=1 bb=+32` (a large positive rate for an
unambiguous, fast glide), sustained 100 jiffies so `measure-pitch` at
different `--skip-seconds` can trace the direction over time.
"""
import math
import struct

OUT_DIR = "."

# Same sine/anchor convention as gen_minimal_scale.py: note $1E -> period 424
# -> ~261 Hz with L=32.
L = 32
NULL_SAMPLE_LEN = 4
TONE_OFFSET = NULL_SAMPLE_LEN
smpl = bytes(NULL_SAMPLE_LEN) + bytes(
    max(-127, min(127, round(100 * math.sin(2 * math.pi * i / L)))) & 0xFF
    for i in range(L)
)
with open(f"{OUT_DIR}/smpl.portamento-probe", "wb") as f:
    f.write(smpl)

# -- macro 0: hold note $1E, then glide with a large positive portamento rate --
#   0: $00 DMAoff+Reset* (aa=0, mandatory 1-jiffy pause)
#   1: $02 SetBegin +TONE_OFFSET (absolute)
#   2: $03 SetLen 0x10 words (whole sine cycle)
#   3: $09 SetNote* aa=$1E (absolute note 30, finetune 0)
#   4: $01 DMAon
#   5: $0B Portamento aa=1 (every jiffy) bb=+32 (0x0020)
#   6: $04 Wait* bbbb=100 (let the glide run)
#   7: $07 STOP
macro0 = bytes([
    0x00, 0x00, 0x00, 0x00,
    0x02, 0x00, 0x00, TONE_OFFSET,
    0x03, 0x00, 0x00, 0x10,
    0x09, 0x1E, 0x00, 0x00,
    0x01, 0x00, 0x00, 0x00,
    0x0B, 0x01, 0x00, 0x20,
    0x04, 0x00, 0x00, 0x64,
    0x07, 0x00, 0x00, 0x00,
])

# -- pattern 0: one note triggering macro 0, held long enough for the glide,
# then End --
pattern0 = bytes([0x80 | 0x1E, 0x00, 0xF0, 120])  # note (unused, SetNote overrides), macro 0, vol 15 voice 0
pattern0 += bytes([0xF0, 0x00, 0x00, 0x00])  # $F0 End

# -- trackstep: one line, track 0 -> pattern 0, tracks 1-7 stopped --
trackstep_line = struct.pack(">H", 0x0000) + b"".join(
    struct.pack(">H", 0xFF00) for _ in range(7)
)

# -- assemble mdat --
mdat = bytearray(0x800 + len(trackstep_line))
mdat[0:10] = b"TFMX-SONG "
struct.pack_into(">H", mdat, 0x140, 0)  # song_end[0] = line 0
struct.pack_into(">H", mdat, 0x180, 3)  # tempo[0] = 3 -> 12.5 Hz

mdat[0x800:0x800 + len(trackstep_line)] = trackstep_line

pattern0_offset = len(mdat)
struct.pack_into(">I", mdat, 0x400, pattern0_offset)
mdat += pattern0

macro0_offset = len(mdat)
struct.pack_into(">I", mdat, 0x600, macro0_offset)
mdat += macro0

with open(f"{OUT_DIR}/mdat.portamento-probe", "wb") as f:
    f.write(mdat)

print(f"wrote mdat.portamento-probe ({len(mdat)} bytes), smpl.portamento-probe ({len(smpl)} bytes)")
print(f"pattern0 @ 0x{pattern0_offset:x}, macro0 @ 0x{macro0_offset:x}, tone data @ 0x{TONE_OFFSET:x}")
print(f"starting note $1E tone: {3546895/424/L:.1f} Hz")
