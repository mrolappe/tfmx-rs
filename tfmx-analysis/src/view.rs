//! View-model structs for Phase 5.8 visualization: waveform regions with
//! loop points, the pattern->macro call graph, and the trackstep structure
//! map. Pure data, derived from the existing walker/zones passes -- no HTML
//! here, so any consumer (the `tfmx-cli` renderer, a future GUI, another
//! export) can read the same JSON. `docs/m5-plan.md` Phase 5.8.

use tfmx::{AccessError, LineCommand, Module, TrackSlot, TrackstepLine, decode_line};

use crate::walker::{WalkResult, walk_song};
use crate::zones::resolve_zones;

/// One macro's sample or loop region, as it would be drawn over `smpl`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct WaveformRegion {
    pub macro_number: u8,
    pub start: u32,
    pub len: u32,
    pub looped: bool,
    /// `start + len` reads past `smpl`'s end -- the static equivalent of
    /// `tfmx-cli lint`'s `sample-region-out-of-bounds` finding.
    pub out_of_bounds: bool,
}

/// Every zone-resolved sample region reachable from one song, against
/// `smpl`'s real length.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct WaveformView {
    pub smpl_len: u32,
    pub regions: Vec<WaveformRegion>,
}

/// One decoded trackstep track slot, mirroring [`TrackSlot`] for
/// serialization (the core crate stays `serde`-free).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum TrackSlotView {
    Pattern { number: u8, transpose: i8 },
    Hold { transpose: i8 },
    StopChannel,
    StopVoice { voice: u8 },
}

impl From<TrackSlot> for TrackSlotView {
    fn from(slot: TrackSlot) -> Self {
        match slot {
            TrackSlot::Pattern { number, transpose } => {
                TrackSlotView::Pattern { number, transpose }
            }
            TrackSlot::Hold { transpose } => TrackSlotView::Hold { transpose },
            TrackSlot::StopChannel => TrackSlotView::StopChannel,
            TrackSlot::StopVoice { voice } => TrackSlotView::StopVoice { voice },
        }
    }
}

/// One decoded `$EFFE` trackstep line command, mirroring [`LineCommand`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum LineCommandView {
    Stop,
    PlaySection { position: u16, times: u16 },
    SetTempo { divisor: u16, cia_bpm: u16 },
    MasterVolSlideA { divisor: u16, target: u16 },
    MasterVolSlideB { divisor: u16, target: u16 },
    Unknown { opcode: u16 },
}

impl From<LineCommand> for LineCommandView {
    fn from(cmd: LineCommand) -> Self {
        match cmd {
            LineCommand::Stop => LineCommandView::Stop,
            LineCommand::PlaySection { position, times } => {
                LineCommandView::PlaySection { position, times }
            }
            LineCommand::SetTempo { divisor, cia_bpm } => {
                LineCommandView::SetTempo { divisor, cia_bpm }
            }
            LineCommand::MasterVolSlideA { divisor, target } => {
                LineCommandView::MasterVolSlideA { divisor, target }
            }
            LineCommand::MasterVolSlideB { divisor, target } => {
                LineCommandView::MasterVolSlideB { divisor, target }
            }
            LineCommand::Unknown { opcode } => LineCommandView::Unknown { opcode },
        }
    }
}

/// One trackstep line, decoded: either eight track slots or one `$EFFE`
/// command.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum StepView {
    Tracks([TrackSlotView; 8]),
    Command(LineCommandView),
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct TrackstepStep {
    pub line: u16,
    pub step: StepView,
}

/// The song's trackstep table, decoded line by line, `song_start..=song_end`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct TrackstepMap {
    pub steps: Vec<TrackstepStep>,
}

/// Everything Phase 5.8's renderer needs for one song: pure data, no HTML.
/// `walk`'s `reachable_patterns`/`reachable_macros`/`edges` *are* the
/// pattern->macro call graph -- no separate graph struct duplicates them.
#[derive(Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct SongView {
    pub song: u8,
    pub waveform: WaveformView,
    pub walk: WalkResult,
    pub trackstep: TrackstepMap,
}

/// Builds every Phase 5.8 view model for song `song`: the waveform regions
/// (from zone resolution over every reachable macro), the pattern->macro
/// call graph (the walk's own `edges`/reachable sets, unchanged), and the
/// trackstep structure map (`song_start..=song_end`, decoded line by line).
pub fn build_song_view(module: &Module, song: u8) -> Result<SongView, AccessError> {
    let walk = walk_song(module, song)?;
    let smpl_len = module.smpl().len() as u32;

    let mut regions = Vec::new();
    for &m in &walk.reachable_macros {
        let table = resolve_zones(module, m)?;
        for zone in &table.zones {
            if let Some(region) = zone.sample {
                regions.push(WaveformRegion {
                    macro_number: m,
                    start: region.start,
                    len: region.len,
                    looped: region.looped,
                    out_of_bounds: region.start + region.len > smpl_len,
                });
            }
        }
    }
    regions.sort_by_key(|r| (r.macro_number, r.start, r.len));
    regions.dedup();

    let mut steps = Vec::new();
    let song_start = module.song_start(song);
    let song_end = module.song_end(song);
    for line in song_start..=song_end {
        let Ok(bytes) = module.trackstep_line(line) else {
            continue;
        };
        let step = match decode_line(bytes) {
            TrackstepLine::Tracks(slots) => StepView::Tracks(slots.map(TrackSlotView::from)),
            TrackstepLine::Command(cmd) => StepView::Command(cmd.into()),
        };
        steps.push(TrackstepStep { line, step });
    }

    Ok(SongView {
        song,
        waveform: WaveformView { smpl_len, regions },
        walk,
        trackstep: TrackstepMap { steps },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::walker::SpanKind;
    use std::collections::BTreeSet;
    use std::fs;

    fn read_corpus(name: &str) -> Option<Vec<u8>> {
        let path = format!("{}/../testdata/{}", env!("CARGO_MANIFEST_DIR"), name);
        fs::read(path).ok()
    }

    /// Same fixed-layout `mdat` shape as `walker::tests::minimal_module`,
    /// plus a `smpl` buffer so out-of-bounds regions are checkable.
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

    fn one_track_line(track0: [u8; 2]) -> [u8; 16] {
        line([
            track0, STOP_TRACK, STOP_TRACK, STOP_TRACK, STOP_TRACK, STOP_TRACK, STOP_TRACK,
            STOP_TRACK,
        ])
    }

    #[test]
    fn waveform_view_flags_a_region_that_reads_past_smpl() {
        let lines = [one_track_line(track_word(1, 0))];
        let pattern1: &[[u8; 4]] = &[[0x00, 0x00, 0x00, 0x00], [0xF0, 0x00, 0x00, 0x00]];
        // Macro 0: SetBegin $0000, SetLen 0x10 words (32 bytes) -- past an
        // 8-byte smpl -- then STOP.
        let macro0: &[[u8; 4]] = &[
            [0x02, 0x00, 0x00, 0x00],
            [0x03, 0x00, 0x00, 0x10],
            [0x07, 0x00, 0x00, 0x00],
        ];
        let mdat = minimal_module(&lines, &[(1, pattern1)], &[(0, macro0)]);
        let module = Module::parse(&mdat, &[0u8; 8]).expect("valid header");

        let view = build_song_view(&module, 0).expect("song 0 in range");

        assert_eq!(view.waveform.smpl_len, 8);
        assert_eq!(view.waveform.regions.len(), 1);
        let region = &view.waveform.regions[0];
        assert_eq!(region.macro_number, 0);
        assert_eq!(region.start, 0);
        assert_eq!(region.len, 32);
        assert!(
            region.out_of_bounds,
            "32 bytes from offset 0 reads past 8-byte smpl"
        );
    }

    #[test]
    fn waveform_view_does_not_flag_an_in_bounds_region() {
        let lines = [one_track_line(track_word(1, 0))];
        let pattern1: &[[u8; 4]] = &[[0x00, 0x00, 0x00, 0x00], [0xF0, 0x00, 0x00, 0x00]];
        let macro0: &[[u8; 4]] = &[
            [0x02, 0x00, 0x00, 0x00],
            [0x03, 0x00, 0x00, 0x04],
            [0x07, 0x00, 0x00, 0x00],
        ];
        let mdat = minimal_module(&lines, &[(1, pattern1)], &[(0, macro0)]);
        let module = Module::parse(&mdat, &[0u8; 8]).expect("valid header");

        let view = build_song_view(&module, 0).expect("song 0 in range");

        assert_eq!(view.waveform.regions.len(), 1);
        assert!(!view.waveform.regions[0].out_of_bounds);
    }

    #[test]
    fn call_graph_carries_the_walk_edges_and_reachable_sets() {
        let lines = [one_track_line(track_word(1, 0))];
        let pattern1: &[[u8; 4]] = &[[0x00, 0x09, 0x00, 0x00], [0xF2, 0x02, 0x00, 0x00]];
        let pattern2: &[[u8; 4]] = &[[0xF0, 0x00, 0x00, 0x00]];
        let macro9: &[[u8; 4]] = &[[0x07, 0x00, 0x00, 0x00]];
        let mdat = minimal_module(&lines, &[(1, pattern1), (2, pattern2)], &[(9, macro9)]);
        let module = Module::parse(&mdat, &[]).expect("valid header");

        let view = build_song_view(&module, 0).expect("song 0 in range");

        assert_eq!(view.walk.reachable_patterns, BTreeSet::from([1, 2]));
        assert_eq!(view.walk.reachable_macros, BTreeSet::from([9]));
        assert!(view.walk.edges.contains(&crate::walker::Edge {
            from: SpanKind::Pattern(1),
            to: SpanKind::Macro(9),
        }));
        assert!(view.walk.edges.contains(&crate::walker::Edge {
            from: SpanKind::Pattern(1),
            to: SpanKind::Pattern(2),
        }));
    }

    #[test]
    fn trackstep_map_decodes_one_step_per_line() {
        let lines = [
            one_track_line(track_word(5, 2)),
            one_track_line([0x80, 0x00]), // Hold, transpose 0
        ];
        let pattern5: &[[u8; 4]] = &[[0xF0, 0x00, 0x00, 0x00]];
        let mdat = minimal_module(&lines, &[(5, pattern5)], &[]);
        let module = Module::parse(&mdat, &[]).expect("valid header");

        let view = build_song_view(&module, 0).expect("song 0 in range");

        assert_eq!(view.trackstep.steps.len(), 2);
        assert_eq!(view.trackstep.steps[0].line, 0);
        match &view.trackstep.steps[0].step {
            StepView::Tracks(slots) => {
                assert_eq!(
                    slots[0],
                    TrackSlotView::Pattern {
                        number: 5,
                        transpose: 2
                    }
                );
                assert_eq!(slots[1], TrackSlotView::StopChannel);
            }
            other => panic!("expected Tracks, got {other:?}"),
        }
        match &view.trackstep.steps[1].step {
            StepView::Tracks(slots) => {
                assert_eq!(slots[0], TrackSlotView::Hold { transpose: 0 });
            }
            other => panic!("expected Tracks, got {other:?}"),
        }
    }

    #[test]
    fn trackstep_map_decodes_effe_command_lines() {
        let mut stop_line = [0u8; 16];
        stop_line[0..2].copy_from_slice(&0xEFFEu16.to_be_bytes());
        stop_line[2..4].copy_from_slice(&0x0000u16.to_be_bytes()); // Stop
        let lines = [stop_line];
        let mdat = minimal_module(&lines, &[], &[]);
        let module = Module::parse(&mdat, &[]).expect("valid header");

        let view = build_song_view(&module, 0).expect("song 0 in range");

        assert_eq!(view.trackstep.steps.len(), 1);
        assert_eq!(
            view.trackstep.steps[0].step,
            StepView::Command(LineCommandView::Stop)
        );
    }

    #[cfg(feature = "serde")]
    #[test]
    fn song_view_serializes_to_valid_json() {
        let lines = [one_track_line(track_word(1, 0))];
        let pattern1: &[[u8; 4]] = &[[0xF0, 0x00, 0x00, 0x00]];
        let mdat = minimal_module(&lines, &[(1, pattern1)], &[]);
        let module = Module::parse(&mdat, &[]).expect("valid header");

        let view = build_song_view(&module, 0).expect("song 0 in range");
        let json = serde_json::to_string(&view).expect("SongView serializes");
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(value["song"], 0);
        assert!(value["trackstep"]["steps"].is_array());
    }

    #[test]
    fn build_song_view_runs_across_full_corpus_without_error() {
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
            let view = build_song_view(&module, 0).unwrap_or_else(|e| panic!("{name}: {e:?}"));
            ran_any = true;
            assert!(
                !view.trackstep.steps.is_empty(),
                "{name}: no trackstep steps"
            );
        }
        if !ran_any {
            eprintln!("no corpus modules found -- run `sh testdata/fetch.sh`");
        }
    }
}
