//! Zone resolution: statically partitions one macro's `(note, volume)`
//! input space along its `$1C <Splitkey>` / `$1D <Splitvol>` branches.
//! `docs/m5-plan.md` Phase 5.3.
//!
//! Unlike [`crate::walk_song`], which scans a macro linearly and ignores
//! branches, this pass *does* interpret `$1C`/`$1D` -- but symbolically,
//! over intervals rather than concrete values, so one pass yields every
//! zone at once.

use std::collections::BTreeSet;
use std::ops::RangeInclusive;

use tfmx::{AccessError, Module};

use crate::walker::{MAX_STEPS, SamplePointer, SampleRegion, sext24};

/// Highest note value a pattern can dispatch: the note byte is masked with
/// `$3F` (`tfmx/src/sequencer.rs`'s `note: aa & 0x3F`).
pub const NOTE_MAX: u8 = 0x3F;

/// Highest macro volume, `docs/playback-model.md:85`. The *entry* volume is
/// narrower still -- `MacroInterpreter::trigger` loads `nibble.min(15) * 3`,
/// so only multiples of 3 up to 45 actually occur -- but the zone axis is
/// the full register domain, and callers map their own velocity onto it.
pub const VOLUME_MAX: u8 = 64;

/// The macro volume register as a function of the entry volume, in the exact
/// closed form every chain of `$0D <AddVolume>` (add, then clamp to
/// `0..=64`) and `$0E <SetVolume>` collapses to: `clamp(entry + offset, lo,
/// hi)`.
///
/// The three fields are load-bearing, not redundant: clamping does not
/// compose with addition (`$0D -10` then `$0D +10` leaves entry volume 0 at
/// 10, not 0), but `clamp(clamp(e+k, lo, hi) + b, 0, 64)` is exactly
/// `clamp(e+k+b, clamp(lo+b, 0, 64), clamp(hi+b, 0, 64))`, so the form is
/// closed under the opcodes that touch it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum MacroVolume {
    Entry {
        offset: i32,
        lo: u8,
        hi: u8,
    },
    /// A `$0F <Envelope>` (or anything else time-varying) has taken over --
    /// the register's value at a later `$1D` is no longer a function of the
    /// entry volume alone.
    Unknown,
}

impl MacroVolume {
    fn identity() -> Self {
        MacroVolume::Entry {
            offset: 0,
            lo: 0,
            hi: VOLUME_MAX,
        }
    }

    fn add(self, delta: i8) -> Self {
        match self {
            MacroVolume::Entry { offset, lo, hi } => MacroVolume::Entry {
                offset: offset + delta as i32,
                lo: (lo as i16 + delta as i16).clamp(0, VOLUME_MAX as i16) as u8,
                hi: (hi as i16 + delta as i16).clamp(0, VOLUME_MAX as i16) as u8,
            },
            MacroVolume::Unknown => MacroVolume::Unknown,
        }
    }

    fn set(value: u8) -> Self {
        let v = value.min(VOLUME_MAX);
        MacroVolume::Entry {
            offset: 0,
            lo: v,
            hi: v,
        }
    }

    /// The register's value for a given entry volume, if statically known.
    pub fn eval(self, entry: u8) -> Option<u8> {
        match self {
            MacroVolume::Entry { offset, lo, hi } => {
                Some((entry as i32 + offset).clamp(lo as i32, hi as i32) as u8)
            }
            MacroVolume::Unknown => None,
        }
    }

    /// The value, if it does not depend on the entry volume at all.
    pub fn fixed(self) -> Option<u8> {
        match self {
            MacroVolume::Entry { lo, hi, .. } if lo == hi => Some(lo),
            _ => None,
        }
    }
}

/// A `$0F <Envelope>`: every `jiffies` jiffies, volume moves `step` towards
/// `target` (`docs/playback-model.md:511`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Envelope {
    pub step: u8,
    pub jiffies: u8,
    pub target: u8,
}

/// Where a zone's path through the macro ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum ZoneExit {
    /// `$07 <STOP>`.
    Stop,
    /// `$06 <Cont>`: playback continues in another macro.
    Cont { macro_number: u8, step: u16 },
    /// `$15 <Go submacro>`.
    GoSub { macro_number: u8, step: u16 },
    /// A `$1C`/`$1D` whose outcome is not a function of `(note, entry
    /// volume)` -- the volume register was already `Unknown` -- or a branch
    /// back to a step this path already visited. The zone below is what was
    /// resolved up to that point; downstream must not treat it as complete.
    Unresolved { step: u16 },
    /// Ran off the end of the data or past [`MAX_STEPS`] with no terminator.
    NoTerminator,
}

/// One `(note range, volume range)` rectangle and what the macro does for it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Zone {
    /// Inclusive note range, within `0..=`[`NOTE_MAX`]. This is the raw
    /// pattern note: `$1C` compares `self.note` *before* the track transpose
    /// or any `$08 <AddNote>` offset is applied.
    pub notes: RangeInclusive<u8>,
    /// Inclusive *entry* volume range, within `0..=`[`VOLUME_MAX`].
    pub volumes: RangeInclusive<u8>,
    /// The live `smpl` region at the end of the path, if the path touched
    /// any sample-pointer opcode.
    pub sample: Option<SampleRegion>,
    /// The volume register at the end of the path.
    pub volume: MacroVolume,
    /// The last `$0F <Envelope>` armed along the path.
    pub envelope: Option<Envelope>,
    pub exit: ZoneExit,
}

/// Every zone of one macro. The zones are disjoint and, taken together,
/// cover the whole `0..=NOTE_MAX` x `0..=VOLUME_MAX` rectangle.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct ZoneTable {
    pub macro_number: u8,
    pub zones: Vec<Zone>,
}

/// Resolves macro `macro_number`'s `$1C`/`$1D` branches into a zone table.
pub fn resolve_zones(module: &Module, macro_number: u8) -> Result<ZoneTable, AccessError> {
    let bytes = module.macro_(macro_number)?;
    let mut zones = Vec::new();
    walk(
        bytes,
        macro_number,
        Path {
            step: 0,
            notes: (0, NOTE_MAX),
            volumes: (0, VOLUME_MAX),
            volume: MacroVolume::identity(),
            envelope: None,
            sample: None,
            visited: BTreeSet::new(),
            budget: MAX_STEPS,
        },
        &mut zones,
    );
    Ok(ZoneTable {
        macro_number,
        zones,
    })
}

/// One in-flight path: the sub-rectangle of inputs that reach `step`, plus
/// the state accumulated on the way there.
struct Path {
    step: u16,
    notes: (u8, u8),
    volumes: (u8, u8),
    volume: MacroVolume,
    envelope: Option<Envelope>,
    sample: Option<SamplePointer>,
    visited: BTreeSet<u16>,
    budget: usize,
}

impl Path {
    fn fork(&self, step: u16, notes: (u8, u8), volumes: (u8, u8)) -> Path {
        Path {
            step,
            notes,
            volumes,
            volume: self.volume,
            envelope: self.envelope,
            sample: self.sample.clone(),
            visited: self.visited.clone(),
            budget: self.budget,
        }
    }

    fn finish(self, macro_number: u8, exit: ZoneExit, zones: &mut Vec<Zone>) {
        zones.push(Zone {
            notes: self.notes.0..=self.notes.1,
            volumes: self.volumes.0..=self.volumes.1,
            sample: self.sample.as_ref().map(|s| {
                let (start, len) = s.live();
                SampleRegion {
                    macro_number,
                    start,
                    len: len * 2, // word count -> bytes, docs/format.md §8
                    looped: s.is_looped(),
                }
            }),
            volume: self.volume,
            envelope: self.envelope,
            exit,
        });
    }
}

fn walk(bytes: &[u8], macro_number: u8, mut path: Path, zones: &mut Vec<Zone>) {
    loop {
        if path.budget == 0 || !path.visited.insert(path.step) {
            let step = path.step;
            return path.finish(macro_number, ZoneExit::Unresolved { step }, zones);
        }
        path.budget -= 1;

        let at = path.step as usize * 4;
        let Some(word) = bytes.get(at..at + 4) else {
            return path.finish(macro_number, ZoneExit::NoTerminator, zones);
        };
        let [op, b1, b2, b3] = [word[0], word[1], word[2], word[3]];
        let word23 = u16::from_be_bytes([b2, b3]);
        path.step += 1;

        match op {
            0x07 => return path.finish(macro_number, ZoneExit::Stop, zones),
            0x06 => {
                return path.finish(
                    macro_number,
                    ZoneExit::Cont {
                        macro_number: b1,
                        step: word23,
                    },
                    zones,
                );
            }
            0x15 => {
                return path.finish(
                    macro_number,
                    ZoneExit::GoSub {
                        macro_number: b1,
                        step: word23,
                    },
                    zones,
                );
            }
            0x1C => {
                // Jumps if `note < b1`; the note register is never written
                // by any macro opcode, so the split is a plain cut of the
                // note axis at `b1`.
                let cut = b1.min(NOTE_MAX.saturating_add(1));
                let (lo, hi) = path.notes;
                if cut > lo {
                    let taken = path.fork(word23, (lo, hi.min(cut - 1)), path.volumes);
                    walk(bytes, macro_number, taken, zones);
                }
                if cut > hi {
                    return; // whole rectangle branched away
                }
                path.notes = (lo.max(cut), hi);
            }
            0x1D => {
                // Jumps if `volume >= b1` (confirmed against the real TFMX
                // editor, 2026-08-02 -- see docs/opcodes.md's note on
                // `$1D`; the reverse of [S1]'s literal "less than" wording).
                // The register is a monotone non-decreasing function of the
                // entry volume, so the taken set is always a suffix of the
                // volume axis -- found by scanning the 65 possible entry
                // values rather than by inverting the clamp.
                if path.volume == MacroVolume::Unknown {
                    let step = path.step - 1;
                    return path.finish(macro_number, ZoneExit::Unresolved { step }, zones);
                }
                let (lo, hi) = path.volumes;
                let cut = (lo..=hi)
                    .find(|&e| path.volume.eval(e).is_some_and(|v| v >= b1))
                    .unwrap_or(hi.saturating_add(1));
                if cut <= hi {
                    let taken = path.fork(word23, path.notes, (cut, hi));
                    walk(bytes, macro_number, taken, zones);
                }
                if cut <= lo {
                    return; // whole rectangle branched away
                }
                path.volumes = (lo, cut - 1);
            }
            0x0D => path.volume = path.volume.add(b3 as i8),
            0x1E => path.volume = path.volume.add(b3 as i8),
            0x0E => path.volume = MacroVolume::set(b1),
            0x0F => {
                path.envelope = Some(Envelope {
                    step: b1,
                    jiffies: b2,
                    target: b3,
                });
                path.volume = MacroVolume::Unknown;
            }
            0x02 => path
                .sample
                .get_or_insert_default()
                .set_begin(sext24(b1, b2, b3)),
            0x03 => path.sample.get_or_insert_default().set_len(word23 as u32),
            0x11 if b1 == 0 => path
                .sample
                .get_or_insert_default()
                .add_begin(i16::from_be_bytes([b2, b3]) as i32),
            0x12 => path.sample.get_or_insert_default().add_len(word23 as u32),
            0x18 => path
                .sample
                .get_or_insert_default()
                .sampleloop(sext24(b1, b2, b3)),
            0x19 => path.sample = Some(SamplePointer::default()),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn corpus(name: &str) -> Option<(Vec<u8>, Vec<u8>)> {
        let dir = format!("{}/../testdata", env!("CARGO_MANIFEST_DIR"));
        let mdat = fs::read(format!("{dir}/mdat.{name}")).ok()?;
        let smpl = fs::read(format!("{dir}/smpl.{name}")).ok()?;
        Some((mdat, smpl))
    }

    fn synth(name: &str) -> (Vec<u8>, Vec<u8>) {
        let dir = format!("{}/../testdata/synth", env!("CARGO_MANIFEST_DIR"));
        (
            fs::read(format!("{dir}/mdat.{name}")).expect("tracked synth fixture"),
            fs::read(format!("{dir}/smpl.{name}")).expect("tracked synth fixture"),
        )
    }

    /// A module whose macro 0 is exactly `words`. Same fixed layout as
    /// `walker.rs`'s `minimal_module`, trimmed to what zone resolution needs
    /// (no trackstep/pattern data: `resolve_zones` starts at a macro).
    fn macro_module(words: &[[u8; 4]]) -> Vec<u8> {
        const MACRO_PTR_OFFSET: usize = 0x600;
        let mut mdat = vec![0u8; 0x800];
        mdat[0..10].copy_from_slice(b"TFMX-SONG ");
        mdat[0x140..0x142].copy_from_slice(&0u16.to_be_bytes());
        mdat[0x180..0x182].copy_from_slice(&3u16.to_be_bytes());
        let offset = mdat.len() as u32;
        mdat[MACRO_PTR_OFFSET..MACRO_PTR_OFFSET + 4].copy_from_slice(&offset.to_be_bytes());
        for w in words {
            mdat.extend_from_slice(w);
        }
        mdat
    }

    fn zones_of(words: &[[u8; 4]]) -> Vec<Zone> {
        let mdat = macro_module(words);
        let module = Module::parse(&mdat, &[]).expect("valid header");
        resolve_zones(&module, 0).expect("macro 0 in range").zones
    }

    #[test]
    fn a_macro_without_splits_is_one_full_range_zone() {
        let zones = zones_of(&[
            [0x02, 0x00, 0x01, 0x00], // SetBegin $000100
            [0x03, 0x00, 0x00, 0x10], // SetLen 16 words
            [0x07, 0x00, 0x00, 0x00], // STOP
        ]);
        assert_eq!(zones.len(), 1);
        assert_eq!(zones[0].notes, 0..=NOTE_MAX);
        assert_eq!(zones[0].volumes, 0..=VOLUME_MAX);
        assert_eq!(zones[0].exit, ZoneExit::Stop);
        assert_eq!(
            zones[0].sample,
            Some(SampleRegion {
                macro_number: 0,
                start: 0x100,
                len: 32,
                looped: false,
            })
        );
    }

    #[test]
    fn splitkey_cuts_the_note_axis_at_the_threshold() {
        // $1C $20 -> step 2. Below the split: step 2 (Cont macro 9).
        // At or above: step 1 (Cont macro 8).
        let zones = zones_of(&[
            [0x1C, 0x20, 0x00, 0x02],
            [0x06, 0x08, 0x00, 0x00],
            [0x06, 0x09, 0x00, 0x00],
        ]);
        assert_eq!(zones.len(), 2);
        let low = zones.iter().find(|z| *z.notes.start() == 0).expect("low");
        let high = zones.iter().find(|z| *z.notes.start() == 0x20).expect("hi");
        assert_eq!(low.notes, 0..=0x1F);
        assert_eq!(
            low.exit,
            ZoneExit::Cont {
                macro_number: 9,
                step: 0
            }
        );
        assert_eq!(high.notes, 0x20..=NOTE_MAX);
        assert_eq!(
            high.exit,
            ZoneExit::Cont {
                macro_number: 8,
                step: 0
            }
        );
        // Both keep the full volume axis: a keysplit says nothing about it.
        assert!(zones.iter().all(|z| z.volumes == (0..=VOLUME_MAX)));
    }

    #[test]
    fn splitvol_boundary_accounts_for_a_preceding_addvolume() {
        // $0D +$15 then $1D $20: the register is entry+21, so (jump if
        // volume >= aa, confirmed against real hardware -- see
        // docs/opcodes.md's note on $1D) the branch is taken for entry
        // volumes at or above 32-21 = 11.
        let zones = zones_of(&[
            [0x0D, 0x00, 0x00, 0x15],
            [0x1D, 0x20, 0x00, 0x03],
            [0x06, 0x08, 0x00, 0x00],
            [0x06, 0x09, 0x00, 0x00],
        ]);
        assert_eq!(zones.len(), 2);
        let quiet = zones.iter().find(|z| *z.volumes.start() == 0).expect("q");
        let loud = zones.iter().find(|z| *z.volumes.start() == 11).expect("l");
        assert_eq!(quiet.volumes, 0..=10);
        assert_eq!(
            quiet.exit,
            ZoneExit::Cont {
                macro_number: 8,
                step: 0
            }
        );
        assert_eq!(loud.volumes, 11..=VOLUME_MAX);
        assert_eq!(
            loud.exit,
            ZoneExit::Cont {
                macro_number: 9,
                step: 0
            }
        );
        assert!(zones.iter().all(|z| z.notes == (0..=NOTE_MAX)));
    }

    #[test]
    fn a_vacuous_split_does_not_produce_an_empty_zone() {
        // Two keysplits at the same threshold: the second cannot cut
        // anything, so the table must still hold exactly two zones.
        let zones = zones_of(&[
            [0x1C, 0x20, 0x00, 0x02],
            [0x1C, 0x20, 0x00, 0x04], // note >= $20 here: never taken
            [0x07, 0x00, 0x00, 0x00],
            [0x07, 0x00, 0x00, 0x00],
            [0x07, 0x00, 0x00, 0x00],
        ]);
        assert_eq!(zones.len(), 2);
        assert!(zones.iter().all(|z| z.notes.start() <= z.notes.end()));
    }

    #[test]
    fn note_and_volume_splits_partition_the_rectangle() {
        let zones = zones_of(&[
            [0x1C, 0x20, 0x00, 0x03], // note < $20 -> step 3
            [0x1D, 0x18, 0x00, 0x05], // (note >= $20) vol < $18 -> step 5
            [0x07, 0x00, 0x00, 0x00],
            [0x07, 0x00, 0x00, 0x00],
            [0x07, 0x00, 0x00, 0x00],
            [0x07, 0x00, 0x00, 0x00],
        ]);
        assert_eq!(zones.len(), 3);
        // Disjoint and complete: every point of the rectangle in exactly one.
        for note in 0..=NOTE_MAX {
            for vol in 0..=VOLUME_MAX {
                let hits = zones
                    .iter()
                    .filter(|z| z.notes.contains(&note) && z.volumes.contains(&vol))
                    .count();
                assert_eq!(hits, 1, "note {note} vol {vol} covered {hits} times");
            }
        }
    }

    #[test]
    fn clamping_does_not_compose_away() {
        // The interpreter clamps after *each* $0D, so -10 then +10 leaves a
        // silent note at 10, not at 0. A single accumulated offset would
        // wrongly report 0 and put the $1D boundary in the wrong place.
        let v = MacroVolume::identity().add(-10).add(10);
        assert_eq!(v.eval(0), Some(10));
        assert_eq!(v.eval(20), Some(20));
        assert_eq!(MacroVolume::set(0x38).fixed(), Some(0x38));
        assert_eq!(MacroVolume::identity().fixed(), None);
    }

    #[test]
    fn an_envelope_makes_a_later_splitvol_unresolved() {
        let zones = zones_of(&[
            [0x0F, 0x05, 0x03, 0x28], // Envelope
            [0x1D, 0x20, 0x00, 0x03],
            [0x07, 0x00, 0x00, 0x00],
            [0x07, 0x00, 0x00, 0x00],
        ]);
        assert_eq!(zones.len(), 1);
        assert_eq!(zones[0].exit, ZoneExit::Unresolved { step: 1 });
        assert_eq!(
            zones[0].envelope,
            Some(Envelope {
                step: 5,
                jiffies: 3,
                target: 0x28
            })
        );
        assert_eq!(zones[0].volume, MacroVolume::Unknown);
    }

    // -- check criterion, half 1: the real corpus --

    /// `tfmx-cli disasm --macro 28` shows macro 28 of `turrican intro` with
    /// no `$1C`/`$1D` at all -- one linear line from `$00 DMAoff+Reset` to
    /// `$07 STOP` -- so its whole input rectangle is a single zone, ending
    /// on the sample region its `$02/$03/$18/$11` chain leaves live.
    #[test]
    fn turrican_intro_macro_28_is_a_single_unsplit_zone() {
        let Some((mdat, smpl)) = corpus("turrican intro") else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let module = Module::parse(&mdat, &smpl).expect("valid module");
        let table = resolve_zones(&module, 28).expect("macro 28 in range");

        assert_eq!(table.zones.len(), 1, "macro 28 has no $1C/$1D");
        let z = &table.zones[0];
        assert_eq!(z.notes, 0..=NOTE_MAX);
        assert_eq!(z.volumes, 0..=VOLUME_MAX);
        assert_eq!(z.exit, ZoneExit::Stop, "the listing ends on $07 STOP");
        assert_eq!(z.envelope, None, "no $0F in the listing");
        // $02 $007804, $03 $0400, $18 $000700 -> loop_start $7F04,
        // loop_len $0400 - $0380 = $0080 words; the two $11 AddBegin steps
        // (-$0100 then +$0100) cancel. $0080 words = $0100 bytes.
        assert_eq!(
            z.sample,
            Some(SampleRegion {
                macro_number: 28,
                start: 0x7F04,
                len: 0x100,
                looped: true,
            }),
            "macro 28's chain ends on $18 Sampleloop"
        );
        // $0D +$14 then $0E SetVolume aa=$00 (docs/opcodes.md:162 -- the
        // operand is `aa`, and macro 28's is zero).
        assert_eq!(z.volume.fixed(), Some(0));
    }

    /// Macro 24 is `turrican intro`'s plain keysplit: `$1C $20 -> 2`, with
    /// `$06 Cont $16` on fall-through and `$06 Cont $17` at the target.
    #[test]
    fn turrican_intro_macro_24_splits_at_note_0x20() {
        let Some((mdat, smpl)) = corpus("turrican intro") else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let module = Module::parse(&mdat, &smpl).expect("valid module");
        let table = resolve_zones(&module, 24).expect("macro 24 in range");

        assert_eq!(table.zones.len(), 2);
        let low = table.zones.iter().find(|z| *z.notes.start() == 0).unwrap();
        let high = table
            .zones
            .iter()
            .find(|z| *z.notes.start() == 0x20)
            .unwrap();
        assert_eq!(low.notes, 0..=0x1F);
        assert_eq!(
            low.exit,
            ZoneExit::Cont {
                macro_number: 0x17,
                step: 0
            }
        );
        assert_eq!(high.notes, 0x20..=NOTE_MAX);
        assert_eq!(
            high.exit,
            ZoneExit::Cont {
                macro_number: 0x16,
                step: 0
            }
        );
    }

    /// Macro 5 is the corpus's only `$1D` chain: `$0D +$15` then four
    /// `$1D`s at $20/$2A/$34/$3C, each jumping to the *next* `$1D` and
    /// falling through to `$06 Cont 4/3/2/1`, with the final jump landing on
    /// `$06 Cont 0`.
    ///
    /// `$1D`'s polarity was confirmed against the real TFMX editor
    /// (2026-08-02, see docs/opcodes.md's note on `$1D`) to be "jump if
    /// volume >= aa", the reverse of [S1]'s literal wording. Under that
    /// polarity this chain is a clean 5-way ascending velocity fan-out
    /// (quietest -> macro 4, loudest -> macro 0), not the 2-zone degenerate
    /// reading the old (wrong) polarity produced.
    #[test]
    fn turrican_intro_macro_5_splitvol_chain() {
        let Some((mdat, smpl)) = corpus("turrican intro") else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let module = Module::parse(&mdat, &smpl).expect("valid module");
        let table = resolve_zones(&module, 5).expect("macro 5 in range");

        assert_eq!(table.zones.len(), 5);
        let layer = |start: u8| {
            table
                .zones
                .iter()
                .find(|z| *z.volumes.start() == start)
                .unwrap_or_else(|| panic!("no zone starting at entry volume {start}"))
        };
        let expect = |start: u8, end: u8, macro_number: u8| {
            let z = layer(start);
            assert_eq!(z.volumes, start..=end, "layer starting at {start}");
            assert_eq!(
                z.exit,
                ZoneExit::Cont {
                    macro_number,
                    step: 0
                },
                "layer starting at {start}"
            );
        };
        expect(0, 10, 4); // entry + $15 < $20
        expect(11, 20, 3); // $20 <= entry + $15 < $2A
        expect(21, 30, 2); // $2A <= entry + $15 < $34
        expect(31, 38, 1); // $34 <= entry + $15 < $3C
        expect(39, VOLUME_MAX, 0); // entry + $15 >= $3C
    }

    #[test]
    fn every_corpus_macro_zone_table_covers_the_rectangle_exactly_once() {
        let files = [
            "turrican intro",
            "turrican outside",
            "r-type",
            "x-out (title)",
            "turrican 2 title (st)",
            "turrican 2 level 1-desert",
            "turrican 2 level 3-flight",
            "turrican 3 level 1",
            "apidya (title)",
            "apidya (level 1)",
        ];
        for name in files {
            let Some((mdat, smpl)) = corpus(name) else {
                eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
                return;
            };
            let module = Module::parse(&mdat, &smpl).expect("valid module");
            for n in 0..128u8 {
                let Ok(table) = resolve_zones(&module, n) else {
                    continue;
                };
                assert!(!table.zones.is_empty(), "{name} macro {n}: no zones");
                // Spot-check the corners plus the axis boundaries rather
                // than all 65*64 points for all 128 macros of 10 modules.
                for note in [0u8, 1, 0x1F, 0x20, NOTE_MAX] {
                    for vol in [0u8, 1, 10, 11, 32, VOLUME_MAX] {
                        let hits = table
                            .zones
                            .iter()
                            .filter(|z| z.notes.contains(&note) && z.volumes.contains(&vol))
                            .count();
                        assert_eq!(hits, 1, "{name} macro {n}: ({note},{vol}) hit {hits} times");
                    }
                }
            }
        }
    }

    // -- check criterion, half 2: the synthetic probe --

    /// `testdata/synth/gen_split_probe.py` builds a module whose macro 0 is
    /// a single `$1C $20` keysplit into two one-instruction branches.
    #[test]
    fn probe_macro_resolves_to_exactly_two_zones_at_note_0x20() {
        let (mdat, smpl) = synth("split-probe");
        let module = Module::parse(&mdat, &smpl).expect("valid module");
        let table = resolve_zones(&module, 0).expect("macro 0 in range");

        assert_eq!(table.zones.len(), 2, "one split -> exactly two zones");
        let low = table.zones.iter().find(|z| *z.notes.start() == 0).unwrap();
        let high = table
            .zones
            .iter()
            .find(|z| *z.notes.start() == 0x20)
            .unwrap();
        assert_eq!(low.notes, 0..=0x1F, "below the split");
        assert_eq!(high.notes, 0x20..=NOTE_MAX, "at or above the split");
        assert_eq!(
            low.exit,
            ZoneExit::Cont {
                macro_number: 2,
                step: 0
            },
            "the low half hands off to the low instrument"
        );
        assert_eq!(
            high.exit,
            ZoneExit::Cont {
                macro_number: 1,
                step: 0
            },
            "the high half hands off to the high instrument"
        );
        assert!(table.zones.iter().all(|z| z.volumes == (0..=VOLUME_MAX)));
    }

    #[cfg(feature = "serde")]
    #[test]
    fn zone_table_serializes_to_valid_json() {
        let zones = zones_of(&[
            [0x02, 0x00, 0x01, 0x00], // SetBegin $000100
            [0x03, 0x00, 0x00, 0x10], // SetLen 16 words
            [0x07, 0x00, 0x00, 0x00], // STOP
        ]);
        let table = ZoneTable {
            macro_number: 0,
            zones,
        };

        let json = serde_json::to_string(&table).expect("ZoneTable serializes");
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(value["macro_number"], 0);
        assert_eq!(value["zones"][0]["exit"], serde_json::json!("Stop"));
        assert_eq!(value["zones"][0]["sample"]["start"], 0x100);
    }
}
