//! The static walker: song -> reachable trackstep lines -> patterns ->
//! macros -> sample regions, via `Module`'s bounds-checked accessors only.
//! `docs/m5-plan.md` Phase 5.2.

use std::collections::{BTreeSet, VecDeque};

use tfmx::{
    AccessError, Module, PatternCommand, PatternEntry, TrackSlot, TrackstepLine, decode_line,
    decode_pattern_entry,
};

/// Steps read from one pattern's or one macro's data before giving up --
/// patterns/macros have no length field (`Module::pattern`/`macro_` return
/// "to the end of mdat"), so an untrusted or malformed module could
/// otherwise never terminate the walk. Mirrors `tfmx-cli`'s
/// `MAX_DISASM_STEPS`, whose linear-scan-to-terminator shape this walker
/// follows for the same reason: it does not execute `$F1`/`$F2`/`$1C`/`$1D`
/// branches, just records every reference it sees along the one line from
/// entry to terminator.
pub(crate) const MAX_STEPS: usize = 256;

/// What a [`Span`] covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum SpanKind {
    Pattern(u8),
    Macro(u8),
}

/// A claimed `mdat` byte range: from a pattern's or macro's start to the
/// terminator (or step cap) the walk stopped at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Span {
    pub kind: SpanKind,
    pub start: u32,
    pub end: u32,
}

/// A `smpl` byte range touched by a macro's sample-pointer opcodes
/// (`$02`/`$03`/`$11`/`$12`/`$18`/`$19`), snapshotted after each one.
/// Best-effort: this is a linear scan, not real execution, so it cannot
/// know which `$1C`/`$1D` branch a real note would take, or resolve `$11`'s
/// oscillating (`aa != 0`) form to a fixed offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct SampleRegion {
    pub macro_number: u8,
    pub start: u32,
    pub len: u32,
    /// Whether `$18 <Sampleloop>` was armed by the time this region was
    /// snapshotted -- `false` means it's the one-shot `$02`/`$03` region,
    /// `true` means it's the post-`$18` loop region. Export formats need
    /// this to know whether to loop the region indefinitely or play it once.
    pub looped: bool,
}

/// Everything statically reachable from one song.
#[derive(Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct WalkResult {
    pub reachable_patterns: BTreeSet<u8>,
    pub reachable_macros: BTreeSet<u8>,
    pub mdat_spans: Vec<Span>,
    pub sample_regions: Vec<SampleRegion>,
    /// Raw (unmasked) `Note.voice` nibbles seen -- deliberately not folded
    /// through `Player::voice_of`'s `& 0x03` mask, since nibbles 4-7 are the
    /// 7V detector's own signature (`docs/m5-plan.md`'s "7V posture").
    pub voice_nibbles: BTreeSet<u8>,
}

impl WalkResult {
    /// Nibbles 4-7 used with no 3 -- `docs/status.md:1190`'s signature,
    /// already confirmed to identify `apidya (title)` uniquely in the
    /// corpus.
    pub fn is_7v(&self) -> bool {
        self.voice_nibbles.iter().any(|&n| (4..=7).contains(&n)) && !self.voice_nibbles.contains(&3)
    }
}

/// Walks song `song`'s trackstep lines to every pattern, macro and sample
/// region they can statically reach.
pub fn walk_song(module: &Module, song: u8) -> Result<WalkResult, AccessError> {
    let song_start = module.song_start(song);
    let song_end = module.song_end(song);
    let mut result = WalkResult::default();
    let mut pattern_queue: VecDeque<u8> = VecDeque::new();
    let mut macro_queue: VecDeque<u8> = VecDeque::new();
    let mut queued_patterns: BTreeSet<u8> = BTreeSet::new();
    let mut queued_macros: BTreeSet<u8> = BTreeSet::new();

    for line in song_start..=song_end {
        let Ok(bytes) = module.trackstep_line(line) else {
            continue;
        };
        if let TrackstepLine::Tracks(slots) = decode_line(bytes) {
            for slot in slots {
                if let TrackSlot::Pattern { number, .. } = slot {
                    queue(&mut pattern_queue, &mut queued_patterns, number);
                }
            }
        }
    }

    while let Some(n) = pattern_queue.pop_front() {
        if !result.reachable_patterns.insert(n) {
            continue;
        }
        let (Ok(bytes), Ok(offset)) = (module.pattern(n), module.pattern_offset(n)) else {
            continue;
        };

        let mut consumed = 0u32;
        for word in bytes.chunks_exact(4).take(MAX_STEPS) {
            consumed += 4;
            let entry = decode_pattern_entry([word[0], word[1], word[2], word[3]]);
            match entry {
                PatternEntry::Note {
                    macro_number,
                    voice,
                    ..
                } => {
                    result.voice_nibbles.insert(voice);
                    queue(&mut macro_queue, &mut queued_macros, macro_number);
                }
                PatternEntry::Command(cmd) => {
                    match cmd {
                        PatternCommand::Jump { pattern, .. }
                        | PatternCommand::GoSub { pattern, .. }
                        | PatternCommand::PlayPattern { pattern, .. } => {
                            queue(&mut pattern_queue, &mut queued_patterns, pattern);
                        }
                        _ => {}
                    }
                    if matches!(cmd, PatternCommand::End | PatternCommand::Stop) {
                        break;
                    }
                }
            }
        }
        result.mdat_spans.push(Span {
            kind: SpanKind::Pattern(n),
            start: offset,
            end: offset + consumed,
        });
    }

    while let Some(n) = macro_queue.pop_front() {
        if !result.reachable_macros.insert(n) {
            continue;
        }
        let (Ok(bytes), Ok(offset)) = (module.macro_(n), module.macro_offset(n)) else {
            continue;
        };

        let mut consumed = 0u32;
        let mut sample = SamplePointer::default();
        for word in bytes.chunks_exact(4).take(MAX_STEPS) {
            consumed += 4;
            let [op, aa, bb, cc] = [word[0], word[1], word[2], word[3]];
            let word23 = u16::from_be_bytes([bb, cc]);
            let mut touched = false;
            match op {
                0x02 => {
                    sample.set_begin(sext24(aa, bb, cc));
                    touched = true;
                }
                0x03 => {
                    sample.set_len(word23 as u32);
                    touched = true;
                }
                0x06 => queue(&mut macro_queue, &mut queued_macros, aa),
                0x11 if aa == 0 => {
                    sample.add_begin(i16::from_be_bytes([bb, cc]) as i32);
                    touched = true;
                }
                0x12 => {
                    sample.add_len(word23 as u32);
                    touched = true;
                }
                0x15 => queue(&mut macro_queue, &mut queued_macros, aa),
                0x18 => {
                    sample.sampleloop(sext24(aa, bb, cc));
                    touched = true;
                }
                0x19 => {
                    sample = SamplePointer::default();
                    touched = true;
                }
                0x21 => queue(&mut macro_queue, &mut queued_macros, aa),
                0x07 => break,
                _ => {}
            }
            if touched {
                let (start, len) = sample.live();
                result.sample_regions.push(SampleRegion {
                    macro_number: n,
                    start,
                    len: len * 2, // word count -> bytes, docs/format.md §8
                    looped: sample.loop_active,
                });
            }
        }
        result.mdat_spans.push(Span {
            kind: SpanKind::Macro(n),
            start: offset,
            end: offset + consumed,
        });
    }

    Ok(result)
}

fn queue(queue: &mut VecDeque<u8>, queued: &mut BTreeSet<u8>, n: u8) {
    if queued.insert(n) {
        queue.push_back(n);
    }
}

/// Mirrors `tfmx/src/macro_interp.rs`'s sample-pointer bookkeeping for the
/// opcodes this walker statically tracks; see that module's `$02`/`$18`
/// comments for the units this copies.
#[derive(Default, Clone)]
pub(crate) struct SamplePointer {
    sample_start: u32,
    sample_len: u32,
    loop_start: u32,
    loop_len: u32,
    loop_active: bool,
}

impl SamplePointer {
    pub(crate) fn set_begin(&mut self, value: i32) {
        self.sample_start = (value as u32) & 0x00FF_FFFF;
        self.loop_start = self.sample_start;
        self.loop_len = self.sample_len;
    }

    pub(crate) fn set_len(&mut self, len: u32) {
        self.sample_len = len;
        self.loop_start = self.sample_start;
        self.loop_len = self.sample_len;
    }

    pub(crate) fn add_begin(&mut self, step: i32) {
        if self.loop_active {
            self.loop_start = self.loop_start.wrapping_add_signed(step);
        } else {
            self.sample_start = self.sample_start.wrapping_add_signed(step);
        }
    }

    pub(crate) fn add_len(&mut self, delta: u32) {
        self.sample_len = self.sample_len.wrapping_add(delta) & 0xFFFF;
    }

    pub(crate) fn sampleloop(&mut self, delta: i32) {
        self.loop_start = self.loop_start.wrapping_add_signed(delta);
        self.loop_len = self.loop_len.wrapping_sub_signed(delta >> 1) & 0xFFFF;
        self.loop_active = true;
    }

    pub(crate) fn live(&self) -> (u32, u32) {
        if self.loop_active {
            (self.loop_start, self.loop_len)
        } else {
            (self.sample_start, self.sample_len)
        }
    }

    pub(crate) fn is_looped(&self) -> bool {
        self.loop_active
    }
}

pub(crate) fn sext24(hi: u8, mid: u8, lo: u8) -> i32 {
    let raw = ((hi as u32) << 16) | ((mid as u32) << 8) | (lo as u32);
    if raw & 0x0080_0000 != 0 {
        (raw | 0xFF00_0000) as i32
    } else {
        raw as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn read_corpus(name: &str) -> Option<Vec<u8>> {
        let path = format!("{}/../testdata/{}", env!("CARGO_MANIFEST_DIR"), name);
        fs::read(path).ok()
    }

    /// A minimal fixed-layout `mdat` with one song's trackstep lines and the
    /// given patterns/macros placed after them. Same fixed offsets as
    /// `tfmx/src/module.rs`'s own tests use.
    fn minimal_module(
        trackstep_lines: &[[u8; 16]],
        patterns: &[(u8, &[[u8; 4]])],
        macros: &[(u8, &[[u8; 4]])],
    ) -> Vec<u8> {
        const PATTERN_PTR_OFFSET: usize = 0x400;
        const MACRO_PTR_OFFSET: usize = 0x600;
        const TRACKSTEP_OFFSET: usize = 0x800;

        let mut mdat = vec![0u8; TRACKSTEP_OFFSET + trackstep_lines.len() * 16];
        mdat[0..10].copy_from_slice(b"TFMX-SONG ");
        mdat[0x100..0x102].copy_from_slice(&0u16.to_be_bytes());
        mdat[0x140..0x142].copy_from_slice(&(trackstep_lines.len() as u16 - 1).to_be_bytes());
        mdat[0x180..0x182].copy_from_slice(&3u16.to_be_bytes());

        for (i, line) in trackstep_lines.iter().enumerate() {
            mdat[TRACKSTEP_OFFSET + i * 16..TRACKSTEP_OFFSET + i * 16 + 16].copy_from_slice(line);
        }

        for (n, words) in patterns {
            let offset = mdat.len() as u32;
            mdat[PATTERN_PTR_OFFSET + *n as usize * 4..PATTERN_PTR_OFFSET + *n as usize * 4 + 4]
                .copy_from_slice(&offset.to_be_bytes());
            for w in *words {
                mdat.extend_from_slice(w);
            }
        }
        for (n, words) in macros {
            let offset = mdat.len() as u32;
            mdat[MACRO_PTR_OFFSET + *n as usize * 4..MACRO_PTR_OFFSET + *n as usize * 4 + 4]
                .copy_from_slice(&offset.to_be_bytes());
            for w in *words {
                mdat.extend_from_slice(w);
            }
        }
        mdat
    }

    fn track_word(number: u8, transpose: i8) -> [u8; 2] {
        [number, transpose as u8]
    }

    const STOP_TRACK: [u8; 2] = [0xFF, 0x00];

    fn line(words: [[u8; 2]; 8]) -> [u8; 16] {
        let mut bytes = [0u8; 16];
        for (i, w) in words.iter().enumerate() {
            bytes[i * 2] = w[0];
            bytes[i * 2 + 1] = w[1];
        }
        bytes
    }

    #[test]
    fn reachable_patterns_and_macros_from_trackstep() {
        let lines = [line([
            track_word(5, 0),
            STOP_TRACK,
            STOP_TRACK,
            STOP_TRACK,
            STOP_TRACK,
            STOP_TRACK,
            STOP_TRACK,
            STOP_TRACK,
        ])];
        // Pattern 5: one note on macro 9, voice 2 (cv = volume<<4 | voice), then End.
        let pattern5: &[[u8; 4]] = &[[0x00, 0x09, 0x02, 0x00], [0xF0, 0x00, 0x00, 0x00]];
        // Macro 9: immediate STOP.
        let macro9: &[[u8; 4]] = &[[0x07, 0x00, 0x00, 0x00]];
        let mdat = minimal_module(&lines, &[(5, pattern5)], &[(9, macro9)]);
        let module = Module::parse(&mdat, &[]).expect("valid header");

        let result = walk_song(&module, 0).expect("song 0 in range");

        assert_eq!(
            result.reachable_patterns,
            BTreeSet::from([5]),
            "only the trackstep-referenced pattern is reachable"
        );
        assert_eq!(
            result.reachable_macros,
            BTreeSet::from([9]),
            "the macro referenced by pattern 5's only note"
        );
        assert_eq!(result.voice_nibbles, BTreeSet::from([2]));
        assert!(!result.is_7v());
    }

    #[test]
    fn pattern_flow_commands_queue_their_target_patterns() {
        let lines = [line([
            track_word(1, 0),
            STOP_TRACK,
            STOP_TRACK,
            STOP_TRACK,
            STOP_TRACK,
            STOP_TRACK,
            STOP_TRACK,
            STOP_TRACK,
        ])];
        // Pattern 1: $F2 Jump to pattern 2, step 0 (never actually executed
        // by this linear walker, but pattern 2 must still be queued).
        let pattern1: &[[u8; 4]] = &[[0xF2, 0x02, 0x00, 0x00]];
        let pattern2: &[[u8; 4]] = &[[0xF0, 0x00, 0x00, 0x00]];
        let mdat = minimal_module(&lines, &[(1, pattern1), (2, pattern2)], &[]);
        let module = Module::parse(&mdat, &[]).expect("valid header");

        let result = walk_song(&module, 0).expect("song 0 in range");

        assert_eq!(result.reachable_patterns, BTreeSet::from([1, 2]));
    }

    #[test]
    fn macro_cross_references_are_queued() {
        let lines = [line([
            track_word(1, 0),
            STOP_TRACK,
            STOP_TRACK,
            STOP_TRACK,
            STOP_TRACK,
            STOP_TRACK,
            STOP_TRACK,
            STOP_TRACK,
        ])];
        let pattern1: &[[u8; 4]] = &[
            [0x00, 0x00, 0x00, 0x00], // note, macro 0
            [0xF0, 0x00, 0x00, 0x00],
        ];
        // Macro 0: $21 Play macro 3, then STOP.
        let macro0: &[[u8; 4]] = &[[0x21, 0x03, 0x00, 0x00], [0x07, 0x00, 0x00, 0x00]];
        let macro3: &[[u8; 4]] = &[[0x07, 0x00, 0x00, 0x00]];
        let mdat = minimal_module(&lines, &[(1, pattern1)], &[(0, macro0), (3, macro3)]);
        let module = Module::parse(&mdat, &[]).expect("valid header");

        let result = walk_song(&module, 0).expect("song 0 in range");

        assert_eq!(result.reachable_macros, BTreeSet::from([0, 3]));
    }

    #[test]
    fn is_7v_true_for_nibble_without_3_false_when_3_present() {
        let lines = [line([
            track_word(1, 0),
            STOP_TRACK,
            STOP_TRACK,
            STOP_TRACK,
            STOP_TRACK,
            STOP_TRACK,
            STOP_TRACK,
            STOP_TRACK,
        ])];
        // Two notes on the same pattern: voice nibble 4, then nibble 0.
        let pattern1: &[[u8; 4]] = &[
            [0x00, 0x00, 0x04, 0x00],
            [0x00, 0x00, 0x00, 0x00],
            [0xF0, 0x00, 0x00, 0x00],
        ];
        let macro0: &[[u8; 4]] = &[[0x07, 0x00, 0x00, 0x00]];
        let mdat = minimal_module(&lines, &[(1, pattern1)], &[(0, macro0)]);
        let module = Module::parse(&mdat, &[]).expect("valid header");
        let result = walk_song(&module, 0).expect("song 0 in range");
        assert_eq!(result.voice_nibbles, BTreeSet::from([0, 4]));
        assert!(
            result.is_7v(),
            "nibble 4 with no nibble 3 is the 7V signature"
        );

        // Same, but with a nibble-3 note added: no longer flagged.
        let pattern1_with_3: &[[u8; 4]] = &[
            [0x00, 0x00, 0x04, 0x00],
            [0x00, 0x00, 0x03, 0x00],
            [0xF0, 0x00, 0x00, 0x00],
        ];
        let mdat = minimal_module(&lines, &[(1, pattern1_with_3)], &[(0, macro0)]);
        let module = Module::parse(&mdat, &[]).expect("valid header");
        let result = walk_song(&module, 0).expect("song 0 in range");
        assert!(
            !result.is_7v(),
            "nibble 3 present rules out the 7V signature"
        );
    }

    #[test]
    fn mdat_span_covers_offset_to_terminator() {
        let lines = [line([
            track_word(1, 0),
            STOP_TRACK,
            STOP_TRACK,
            STOP_TRACK,
            STOP_TRACK,
            STOP_TRACK,
            STOP_TRACK,
            STOP_TRACK,
        ])];
        let pattern1: &[[u8; 4]] = &[[0x00, 0x00, 0x00, 0x00], [0xF0, 0x00, 0x00, 0x00]];
        let macro0: &[[u8; 4]] = &[[0x07, 0x00, 0x00, 0x00]];
        let mdat = minimal_module(&lines, &[(1, pattern1)], &[(0, macro0)]);
        let module = Module::parse(&mdat, &[]).expect("valid header");
        let offset = module.pattern_offset(1).expect("pattern 1 in range");

        let result = walk_song(&module, 0).expect("song 0 in range");

        let span = result
            .mdat_spans
            .iter()
            .find(|s| s.kind == SpanKind::Pattern(1))
            .expect("pattern 1's span recorded");
        assert_eq!(span.start, offset);
        assert_eq!(span.end, offset + 8, "two longwords consumed up to End");
    }

    #[test]
    fn sample_region_recorded_from_setbegin_setlen() {
        let lines = [line([
            track_word(1, 0),
            STOP_TRACK,
            STOP_TRACK,
            STOP_TRACK,
            STOP_TRACK,
            STOP_TRACK,
            STOP_TRACK,
            STOP_TRACK,
        ])];
        let pattern1: &[[u8; 4]] = &[[0x00, 0x00, 0x00, 0x00], [0xF0, 0x00, 0x00, 0x00]];
        // Macro 0: SetBegin $000100, SetLen $0010 (16 words = 32 bytes), STOP.
        let macro0: &[[u8; 4]] = &[
            [0x02, 0x00, 0x01, 0x00],
            [0x03, 0x00, 0x00, 0x10],
            [0x07, 0x00, 0x00, 0x00],
        ];
        let mdat = minimal_module(&lines, &[(1, pattern1)], &[(0, macro0)]);
        let module = Module::parse(&mdat, &[]).expect("valid header");

        let result = walk_song(&module, 0).expect("song 0 in range");

        let last = result
            .sample_regions
            .last()
            .expect("at least one region recorded");
        assert_eq!(last.macro_number, 0);
        assert_eq!(last.start, 0x100);
        assert_eq!(last.len, 32, "16 words = 32 bytes, docs/format.md §8");
    }

    #[test]
    fn walks_all_corpus_modules_song_0_without_panic() {
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
        let mut ran_any = false;
        for name in files {
            let Some(mdat) = read_corpus(&format!("mdat.{name}")) else {
                eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
                continue;
            };
            let smpl = read_corpus(&format!("smpl.{name}")).expect("smpl alongside mdat");
            let module = Module::parse(&mdat, &smpl).unwrap_or_else(|e| panic!("{name}: {e:?}"));
            let result = walk_song(&module, 0).unwrap_or_else(|e| panic!("{name}: {e:?}"));
            ran_any = true;
            let is_7v = result.is_7v();
            if name == "apidya (title)" {
                assert!(is_7v, "{name} is the corpus's known 7V module");
            } else {
                assert!(!is_7v, "{name} is not a 7V module");
            }

            let claimed: u32 = result.mdat_spans.iter().map(|s| s.end - s.start).sum();
            let coverage = 100.0 * claimed as f64 / mdat.len() as f64;
            eprintln!(
                "{name}: {} patterns, {} macros reachable; provenance {claimed}/{} bytes ({coverage:.1}%)",
                result.reachable_patterns.len(),
                result.reachable_macros.len(),
                mdat.len(),
            );
        }
        if !ran_any {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn walk_result_serializes_to_valid_json() {
        let mut result = WalkResult::default();
        result.reachable_patterns.insert(1);
        result.reachable_macros.insert(2);
        result.mdat_spans.push(Span {
            kind: SpanKind::Pattern(1),
            start: 0,
            end: 10,
        });
        result.sample_regions.push(SampleRegion {
            macro_number: 2,
            start: 4,
            len: 8,
            looped: false,
        });
        result.voice_nibbles.insert(0);

        let json = serde_json::to_string(&result).expect("WalkResult serializes");
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(value["reachable_patterns"], serde_json::json!([1]));
        assert_eq!(value["mdat_spans"][0]["start"], 0);
        assert_eq!(value["mdat_spans"][0]["end"], 10);
        assert_eq!(value["sample_regions"][0]["len"], 8);
    }
}
