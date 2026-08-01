#!/usr/bin/env python3
"""Generates a minimal synthetic TFMX module: one trackstep line, one
pattern playing a chromatic scale, through the simplest possible
DMAon-only macro, over a synthesized single-cycle sine wave sample.

Built to isolate pitch handling from every real-corpus confound (macro
retrigger timing, multi-stage envelopes, the AnyTrack gating cascade) --
see docs/macro-playback-fidelity.md session 11.
"""
import math
import struct

OUT_DIR = "."

# -- smpl: 4 bytes of silence (docs/format.md §8: both real smpl.* files
# begin with 4 zero bytes, "suggesting offset $0 conventionally holds a
# short silent null sample" -- reserve it here too in case the editor
# treats offset 0 specially), then one 32-sample sine cycle, signed 8-bit
# PCM, starting at the next word-aligned offset. --
# note $1E (30) -> period 424 -> freq_hz = 3546895/424 ~= 8365 Hz
# (docs/playback-model.md's own worked example). tone_freq = freq_hz / L,
# so L=32 puts note 30 at ~261.4 Hz, i.e. real middle C -- chosen so the
# scale lands in an easily-judged-by-ear octave.
L = 32
NULL_SAMPLE_LEN = 4
TONE_OFFSET = NULL_SAMPLE_LEN
smpl = bytes(NULL_SAMPLE_LEN) + bytes(
    max(-127, min(127, round(100 * math.sin(2 * math.pi * i / L))))
    & 0xFF
    for i in range(L)
)
with open(f"{OUT_DIR}/smpl.minimal-scale", "wb") as f:
    f.write(smpl)

# -- macro 0: the simplest DMA-on instrument --
# $00 DMAoff+Reset (aa=0, mandatory 1-jiffy pause)
# $02 SetBegin +TONE_OFFSET (absolute, per the session-11 fix) -- past the
#     reserved null sample at offset 0
# $03 SetLen 0x10 words (16 words = 32 bytes = the whole sine cycle;
#     Paula loops the SetBegin/SetLen region on its own, no $18 needed --
#     confirmed by the TFMX editor, docs/macro-playback-fidelity.md §12)
# $08 AddNote* (transpose 0, finetune 0 -- period comes from the note the
#     pattern dispatches)
# $01 DMAon
# $07 STOP (macro program halts; DMA keeps looping the region regardless)
macro0 = bytes([
    0x00, 0x00, 0x00, 0x00,
    0x02, 0x00, 0x00, TONE_OFFSET,
    0x03, 0x00, 0x00, 0x10,
    0x08, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x00, 0x00,
    0x07, 0x00, 0x00, 0x00,
])

# -- pattern 0: a chromatic scale, notes 0x1E..0x2A (one octave), macro 0,
# voice 0, max volume, 5 jiffies each (0.4s at the song's tempo-3, 12.5Hz
# rate -- tempo 0 has never actually been validated in the editor, tempo 3
# has, docs/trackstep-timing-bug.md §3) --
NOTES = list(range(0x1E, 0x2B))  # 30..42 inclusive, 13 notes
WAIT_JIFFIES = 4  # note occupies WAIT_JIFFIES + 1 = 5 jiffies
pattern0 = b"".join(
    bytes([0x80 | note, 0x00, 0xF0, WAIT_JIFFIES])  # macro 0, cv=volume15<<4|voice0
    for note in NOTES
)
pattern0 += bytes([0xF0, 0x00, 0x00, 0x00])  # $F0 End

# -- trackstep: one line, track 0 -> pattern 0 transpose 0, tracks 1-7 stopped --
trackstep_line = struct.pack(">H", 0x0000) + b"".join(
    struct.pack(">H", 0xFF00) for _ in range(7)
)

# -- assemble mdat --
mdat = bytearray(0x800 + len(trackstep_line))
mdat[0:10] = b"TFMX-SONG "
# song_start[0] defaults to 0 (line 0); song_end[0] = num_lines - 1 = 0
struct.pack_into(">H", mdat, 0x140, 0)
# tempo[0] = 3 -> 50/(3+1) = 12.5 Hz -- an editor-validated tempo, unlike 0
struct.pack_into(">H", mdat, 0x180, 3)
# layout table at 0x1D0 stays zero -> Fixed layout, tables at 0x400/0x600/0x800

mdat[0x800:0x800 + len(trackstep_line)] = trackstep_line

pattern0_offset = len(mdat)
struct.pack_into(">I", mdat, 0x400, pattern0_offset)
mdat += pattern0

macro0_offset = len(mdat)
struct.pack_into(">I", mdat, 0x600, macro0_offset)
mdat += macro0

with open(f"{OUT_DIR}/mdat.minimal-scale", "wb") as f:
    f.write(mdat)

print(f"wrote mdat.minimal-scale ({len(mdat)} bytes), smpl.minimal-scale ({len(smpl)} bytes)")
print(f"pattern0 @ 0x{pattern0_offset:x}, macro0 @ 0x{macro0_offset:x}, tone data @ 0x{TONE_OFFSET:x}")
print(f"expected note-30 tone: {3546895/424/L:.1f} Hz, note-42 (octave up): {2*3546895/424/L:.1f} Hz")
