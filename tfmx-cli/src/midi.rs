//! Turns a `TraceEvent` trace (the same seam `Command::Trace` renders) into
//! a MIDI note-event stream, then writes it as a Standard MIDI File via
//! `midly`. `docs/m5-plan.md` Phase 5.5.
//!
//! One tick per jiffy: trivially satisfies "PPQ chosen so 1 jiffy is an
//! exact integer tick count" (the plan's own requirement) and keeps the
//! header's ticks-per-quarter-note fixed while `docs/playback-model.md`
//! §3.2's `50/(v+1)` jiffy rate becomes a MIDI tempo meta event -- real
//! wall-clock time is exact regardless of which `PPQ` is picked, since
//! microseconds-per-tick is derived from it, not the other way round.

use midly::num::{u24, u28, u4, u7, u15};
use tfmx::{PatternCommand, PatternEntry, TraceEvent};

/// Pitch bend range set via RPN 0 on every pitched channel, wide enough for
/// a `$0B <Portamento>` glide spanning more than an octave without
/// clipping. `docs/playback-model.md` §6.
const BEND_RANGE_SEMITONES: i16 = 24;

use crate::midi_mapping::{MidiMapping, ZoneOutput, DRUM_CHANNEL};

/// Ticks per MIDI quarter note. Arbitrary (any value keeps 1 jiffy = 1 tick
/// exact) -- chosen high enough for smooth pitch-bend ramps in a DAW grid.
pub const PPQ: u16 = 96;

/// Base MIDI note for the TFMX raw note byte that is this crate's own
/// middle-C anchor (`tfmx::macro_interp::MIDDLE_C_NOTE = 0x18`,
/// `docs/m5-session-log.md`'s pitch-anchor finding) -- both are semitone-
/// linear, so `midi_note = 60 + (tfmx_note - 0x18)`.
const MIDDLE_C_MIDI: i32 = 60;
const MIDDLE_C_TFMX: i32 = 0x18;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    NoteOn { note: u8, velocity: u8 },
    NoteOff { note: u8 },
    ProgramChange { program: u8 },
    Controller { controller: u8, value: u8 },
    /// Signed 14-bit pitch bend value, `-8192..=8191` (0 = centered).
    PitchBend { bend: i16 },
    /// Track-level meta event: microseconds per quarter note.
    Tempo { microseconds_per_quarter: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MidiEvent {
    pub tick: u32,
    /// 0-based MIDI channel. Meaningless for `EventKind::Tempo`.
    pub channel: u8,
    pub kind: EventKind,
}

/// A stored tempo value `v` (`docs/playback-model.md` §3.2) as one jiffy's
/// duration in microseconds: the jiffy rate is `50/(v+1)` Hz.
fn jiffy_microseconds(tempo: u16) -> u32 {
    (tempo as u32 + 1) * 20_000
}

/// `docs/playback-model.md` §4's raw note byte plus the trace's own
/// `transpose` (both semitone-linear, `tfmx/src/macro_interp.rs:676`) and
/// the mapping zone's hand-authored transpose, anchored at this crate's
/// `MIDDLE_C_NOTE` -> MIDI 60, clamped to a valid MIDI note.
fn midi_note_for(note: u8, transpose: i8, zone_transpose: i8) -> u8 {
    let semitones =
        MIDDLE_C_MIDI + note as i32 + transpose as i32 + zone_transpose as i32 - MIDDLE_C_TFMX;
    semitones.clamp(0, 127) as u8
}

/// The pattern note record's `0-64` volume (`docs/playback-model.md` §4) as
/// a MIDI velocity `1-127` -- never 0, since a `NoteOn` velocity of 0 is a
/// `NoteOff` by MIDI convention.
fn velocity_for(volume: u8) -> u8 {
    ((volume as u32 * 127) / 64).clamp(1, 127) as u8
}

/// `state.period`'s deviation from `base_period` (Paula's period is
/// inversely proportional to frequency, `tfmx/src/macro_interp.rs:26`) in
/// semitones, scaled into a 14-bit bend value by [`BEND_RANGE_SEMITONES`].
fn bend_for(base_period: u16, current_period: u16) -> i16 {
    if current_period == 0 {
        return 0;
    }
    let semitones = 12.0 * (base_period as f64 / current_period as f64).log2();
    ((semitones / BEND_RANGE_SEMITONES as f64) * 8192.0).round().clamp(-8192.0, 8191.0) as i16
}

#[derive(Default)]
struct VoiceState {
    /// The MIDI note + channel currently sounding on this voice, if any --
    /// `None` when nothing has triggered yet or the last trigger's zone
    /// dropped it.
    active: Option<(u8, u8)>,
    /// The period `Voice.period` had the first jiffy it went nonzero after
    /// the current trigger -- the pitch-bend-zero reference. `None` until
    /// captured, and again after every retrigger.
    base_period: Option<u16>,
    last_bend: i16,
}

/// Builds the absolute-tick MIDI event stream for one trace, per `mapping`.
/// `trace` is chronological, the same order `Player::render_traced` emits
/// it in (`tfmx/src/trace.rs`): one `Jiffy` first, then any number of
/// `Pattern`/`Trigger` pairs, per jiffy.
pub fn build_events(trace: &[TraceEvent], mapping: &MidiMapping) -> Vec<MidiEvent> {
    let mut events = Vec::new();
    let mut voices: [VoiceState; 4] = std::array::from_fn(|_| VoiceState::default());
    let mut last_program: [Option<u8>; 16] = [None; 16];
    let mut rpn_sent: [bool; 16] = [false; 16];
    let mut current_tick: u32 = 0;
    let mut last_tick: u32 = 0;
    let mut next_tick: u32 = 0;
    let mut last_tempo: Option<u16> = None;

    for event in trace {
        match *event {
            TraceEvent::Jiffy { tempo, .. } => {
                current_tick = next_tick;
                last_tick = current_tick;
                next_tick += 1;
                if last_tempo != Some(tempo) {
                    events.push(MidiEvent {
                        tick: current_tick,
                        channel: 0,
                        kind: EventKind::Tempo {
                            microseconds_per_quarter: jiffy_microseconds(tempo) * PPQ as u32,
                        },
                    });
                    last_tempo = Some(tempo);
                }
            }
            TraceEvent::Trigger { voice, macro_number, note, volume, transpose } => {
                let voice_idx = (voice & 0x03) as usize;
                if let Some((old_note, old_channel)) = voices[voice_idx].active.take() {
                    events.push(MidiEvent {
                        tick: current_tick,
                        channel: old_channel,
                        kind: EventKind::NoteOff { note: old_note },
                    });
                }
                voices[voice_idx].base_period = None;
                let Some(zone) = mapping.zone_for(macro_number, note, volume) else {
                    continue;
                };
                let (channel, midi_note) = match zone.output {
                    ZoneOutput::Drop => continue,
                    ZoneOutput::Drum { note: drum_note } => (DRUM_CHANNEL, drum_note),
                    ZoneOutput::Program { program } => {
                        let channel = voice_idx as u8;
                        if !rpn_sent[channel as usize] {
                            for (controller, value) in [
                                (101, 0),
                                (100, 0),
                                (6, BEND_RANGE_SEMITONES as u8),
                                (38, 0),
                            ] {
                                events.push(MidiEvent {
                                    tick: current_tick,
                                    channel,
                                    kind: EventKind::Controller { controller, value },
                                });
                            }
                            rpn_sent[channel as usize] = true;
                        }
                        if voices[voice_idx].last_bend != 0 {
                            events.push(MidiEvent {
                                tick: current_tick,
                                channel,
                                kind: EventKind::PitchBend { bend: 0 },
                            });
                            voices[voice_idx].last_bend = 0;
                        }
                        if last_program[channel as usize] != Some(program) {
                            events.push(MidiEvent {
                                tick: current_tick,
                                channel,
                                kind: EventKind::ProgramChange { program },
                            });
                            last_program[channel as usize] = Some(program);
                        }
                        (channel, midi_note_for(note, transpose, zone.transpose))
                    }
                };
                events.push(MidiEvent {
                    tick: current_tick,
                    channel,
                    kind: EventKind::NoteOn { note: midi_note, velocity: velocity_for(volume) },
                });
                voices[voice_idx].active = Some((midi_note, channel));
            }
            TraceEvent::Voice { voice, state } => {
                let voice_idx = (voice & 0x03) as usize;
                let Some((_, channel)) = voices[voice_idx].active else { continue };
                if channel == DRUM_CHANNEL || state.period == 0 {
                    continue;
                }
                match voices[voice_idx].base_period {
                    None => voices[voice_idx].base_period = Some(state.period),
                    Some(base) => {
                        let bend = bend_for(base, state.period);
                        if bend != voices[voice_idx].last_bend {
                            events.push(MidiEvent {
                                tick: current_tick,
                                channel,
                                kind: EventKind::PitchBend { bend },
                            });
                            voices[voice_idx].last_bend = bend;
                        }
                    }
                }
            }
            TraceEvent::Pattern {
                entry: PatternEntry::Command(PatternCommand::KeyUp { voice }),
                ..
            } => {
                let voice_idx = (voice & 0x03) as usize;
                if let Some((note, channel)) = voices[voice_idx].active.take() {
                    events.push(MidiEvent {
                        tick: current_tick,
                        channel,
                        kind: EventKind::NoteOff { note },
                    });
                }
            }
            _ => {}
        }
    }

    for voice in &mut voices {
        if let Some((note, channel)) = voice.active.take() {
            events.push(MidiEvent { tick: last_tick, channel, kind: EventKind::NoteOff { note } });
        }
    }

    events
}

/// One `Vec<MidiEvent>` (absolute ticks) as a `midly::Track` (relative
/// deltas, `midly`'s own on-disk unit) plus the closing `EndOfTrack` meta.
fn build_track(events: &[MidiEvent]) -> Vec<midly::TrackEvent<'static>> {
    let mut track = Vec::with_capacity(events.len() + 1);
    let mut last_tick = 0u32;
    for event in events {
        let delta = event.tick - last_tick;
        last_tick = event.tick;
        let channel = u4::new(event.channel);
        let kind = match event.kind {
            EventKind::NoteOn { note, velocity } => midly::TrackEventKind::Midi {
                channel,
                message: midly::MidiMessage::NoteOn { key: u7::new(note), vel: u7::new(velocity) },
            },
            EventKind::NoteOff { note } => midly::TrackEventKind::Midi {
                channel,
                message: midly::MidiMessage::NoteOff { key: u7::new(note), vel: u7::new(0) },
            },
            EventKind::ProgramChange { program } => midly::TrackEventKind::Midi {
                channel,
                message: midly::MidiMessage::ProgramChange { program: u7::new(program) },
            },
            EventKind::Controller { controller, value } => midly::TrackEventKind::Midi {
                channel,
                message: midly::MidiMessage::Controller {
                    controller: u7::new(controller),
                    value: u7::new(value),
                },
            },
            EventKind::PitchBend { bend } => midly::TrackEventKind::Midi {
                channel,
                message: midly::MidiMessage::PitchBend { bend: midly::PitchBend::from_int(bend) },
            },
            EventKind::Tempo { microseconds_per_quarter } => {
                midly::TrackEventKind::Meta(midly::MetaMessage::Tempo(u24::new(microseconds_per_quarter)))
            }
        };
        track.push(midly::TrackEvent { delta: u28::new(delta), kind });
    }
    track.push(midly::TrackEvent {
        delta: u28::new(0),
        kind: midly::TrackEventKind::Meta(midly::MetaMessage::EndOfTrack),
    });
    track
}

/// Writes `events` as a single-track Standard MIDI File (Format 0 -- every
/// channel's events interleaved in one track, valid and simplest for a
/// programmatic export with no separate per-instrument tracks to name).
pub fn write_smf<W: std::io::Write>(events: &[MidiEvent], out: W) -> std::io::Result<()> {
    let header = midly::Header::new(
        midly::Format::SingleTrack,
        midly::Timing::Metrical(u15::new(PPQ)),
    );
    let smf = midly::Smf { header, tracks: vec![build_track(events)] };
    smf.write_std(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::midi_mapping::{MacroMapping, MappingZone, MidiMapping};
    use std::collections::BTreeMap;

    fn jiffy(tempo: u16) -> TraceEvent {
        TraceEvent::Jiffy { frame: 0, line: 0, tempo, stopped: false }
    }

    fn trigger(voice: u8, macro_number: u8, note: u8, volume: u8) -> TraceEvent {
        TraceEvent::Trigger { voice, macro_number, note, volume, transpose: 0 }
    }

    fn key_up(voice: u8) -> TraceEvent {
        TraceEvent::Pattern {
            track: voice,
            pattern: 0,
            step: 0,
            entry: PatternEntry::Command(PatternCommand::KeyUp { voice }),
        }
    }

    fn program_mapping(macro_number: u8, program: u8) -> MidiMapping {
        let mut macros = BTreeMap::new();
        macros.insert(
            macro_number,
            MacroMapping {
                zones: vec![MappingZone {
                    notes: (0, 63),
                    volumes: (0, 64),
                    output: ZoneOutput::Program { program },
                    transpose: 0,
                }],
            },
        );
        MidiMapping { macros }
    }

    #[test]
    fn a_trigger_emits_rpn_then_program_change_then_note_on_at_the_jiffys_tick() {
        let trace = vec![jiffy(0), trigger(0, 5, 0x18, 64)];
        let events = build_events(&trace, &program_mapping(5, 5));

        assert_eq!(
            events,
            vec![
                MidiEvent {
                    tick: 0,
                    channel: 0,
                    kind: EventKind::Tempo { microseconds_per_quarter: 20_000 * PPQ as u32 }
                },
                MidiEvent {
                    tick: 0,
                    channel: 0,
                    kind: EventKind::Controller { controller: 101, value: 0 }
                },
                MidiEvent {
                    tick: 0,
                    channel: 0,
                    kind: EventKind::Controller { controller: 100, value: 0 }
                },
                MidiEvent {
                    tick: 0,
                    channel: 0,
                    kind: EventKind::Controller { controller: 6, value: BEND_RANGE_SEMITONES as u8 }
                },
                MidiEvent {
                    tick: 0,
                    channel: 0,
                    kind: EventKind::Controller { controller: 38, value: 0 }
                },
                MidiEvent { tick: 0, channel: 0, kind: EventKind::ProgramChange { program: 5 } },
                MidiEvent {
                    tick: 0,
                    channel: 0,
                    kind: EventKind::NoteOn { note: 60, velocity: 127 }
                },
                MidiEvent { tick: 0, channel: 0, kind: EventKind::NoteOff { note: 60 } },
            ]
        );
    }

    #[test]
    fn the_rpn_sequence_is_sent_only_once_per_channel() {
        let trace = vec![jiffy(0), trigger(0, 5, 0x18, 64), jiffy(0), trigger(0, 5, 0x1E, 64)];
        let events = build_events(&trace, &program_mapping(5, 5));
        let rpn_count =
            events.iter().filter(|e| matches!(e.kind, EventKind::Controller { .. })).count();
        assert_eq!(rpn_count, 4, "one RPN sequence, not one per trigger");
    }

    #[test]
    fn a_period_change_after_trigger_emits_a_pitch_bend_and_the_reference_jiffy_does_not() {
        // `Voice` has private fields (`tfmx/src/paula.rs`), so a test builds
        // one the same way the real trace does: through `Paula`'s own API.
        let voice_event = |voice: u8, period: u16| {
            let mut paula = tfmx::Paula::new(100);
            paula.set_period(voice, period);
            TraceEvent::Voice { voice, state: paula.voice(voice) }
        };
        let trace = vec![
            jiffy(0),
            trigger(0, 5, 0x18, 64),
            voice_event(0, 424), // reference: no bend yet
            jiffy(0),
            voice_event(0, 400), // higher pitch -> positive bend
        ];
        let events = build_events(&trace, &program_mapping(5, 5));

        let bends: Vec<_> =
            events.iter().filter(|e| matches!(e.kind, EventKind::PitchBend { .. })).collect();
        assert_eq!(bends.len(), 1, "the reference-capturing jiffy emits no bend");
        assert_eq!(bends[0].tick, 1);
        assert!(
            matches!(bends[0].kind, EventKind::PitchBend { bend } if bend > 0),
            "a shorter period is a higher pitch: positive bend"
        );
    }

    #[test]
    fn an_unchanged_period_emits_no_repeat_pitch_bend() {
        // `Voice` has private fields (`tfmx/src/paula.rs`), so a test builds
        // one the same way the real trace does: through `Paula`'s own API.
        let voice_event = |voice: u8, period: u16| {
            let mut paula = tfmx::Paula::new(100);
            paula.set_period(voice, period);
            TraceEvent::Voice { voice, state: paula.voice(voice) }
        };
        let trace = vec![
            jiffy(0),
            trigger(0, 5, 0x18, 64),
            voice_event(0, 424),
            jiffy(0),
            voice_event(0, 424),
            jiffy(0),
            voice_event(0, 424),
        ];
        let events = build_events(&trace, &program_mapping(5, 5));
        assert!(!events.iter().any(|e| matches!(e.kind, EventKind::PitchBend { .. })));
    }

    #[test]
    fn successive_jiffies_advance_the_tick_by_one() {
        let trace = vec![jiffy(0), trigger(0, 5, 0x18, 64), jiffy(0), jiffy(0), key_up(0)];
        let events = build_events(&trace, &program_mapping(5, 5));

        let note_off = events
            .iter()
            .find(|e| matches!(e.kind, EventKind::NoteOff { .. }))
            .expect("key-up closes the note");
        assert_eq!(note_off.tick, 2, "third jiffy (index 2) is tick 2");
    }

    #[test]
    fn a_retrigger_on_the_same_voice_closes_the_previous_note_first() {
        let trace = vec![jiffy(0), trigger(0, 5, 0x18, 64), jiffy(0), trigger(0, 5, 0x1E, 64)];
        let events = build_events(&trace, &program_mapping(5, 5));

        let ons: Vec<_> = events.iter().filter(|e| matches!(e.kind, EventKind::NoteOn { .. })).collect();
        let offs: Vec<_> = events.iter().filter(|e| matches!(e.kind, EventKind::NoteOff { .. })).collect();
        assert_eq!(ons.len(), 2, "one Trigger = one NoteOn, per the check criterion");
        // one NoteOff for the retrigger closing the first note, one for the
        // trace-end flush closing the second (still active, no trailing jiffy)
        assert_eq!(offs.len(), 2);
        assert_eq!(offs[0].tick, 1, "the retrigger closed the first note at its own jiffy");
    }

    #[test]
    fn a_dropped_zone_emits_no_note_on() {
        let mut macros = BTreeMap::new();
        macros.insert(
            5,
            MacroMapping {
                zones: vec![MappingZone {
                    notes: (0, 63),
                    volumes: (0, 64),
                    output: ZoneOutput::Drop,
                    transpose: 0,
                }],
            },
        );
        let mapping = MidiMapping { macros };
        let trace = vec![jiffy(0), trigger(0, 5, 0x18, 64)];

        let events = build_events(&trace, &mapping);
        assert!(!events.iter().any(|e| matches!(e.kind, EventKind::NoteOn { .. })));
    }

    #[test]
    fn a_drum_zone_uses_the_fixed_note_and_the_drum_channel() {
        let mut macros = BTreeMap::new();
        macros.insert(
            5,
            MacroMapping {
                zones: vec![MappingZone {
                    notes: (0, 63),
                    volumes: (0, 64),
                    output: ZoneOutput::Drum { note: 36 },
                    transpose: 0,
                }],
            },
        );
        let mapping = MidiMapping { macros };
        let trace = vec![jiffy(0), trigger(0, 5, 0x30, 64)];

        let events = build_events(&trace, &mapping);
        let on = events.iter().find(|e| matches!(e.kind, EventKind::NoteOn { .. })).unwrap();
        assert_eq!(on.channel, DRUM_CHANNEL);
        assert_eq!(on.kind, EventKind::NoteOn { note: 36, velocity: 127 });
    }

    #[test]
    fn a_note_still_active_at_the_end_of_the_trace_gets_a_final_note_off() {
        let trace = vec![jiffy(0), trigger(0, 5, 0x18, 64), jiffy(0)];
        let events = build_events(&trace, &program_mapping(5, 5));
        assert!(matches!(events.last().unwrap().kind, EventKind::NoteOff { .. }));
    }

    #[test]
    fn a_tempo_change_emits_a_new_tempo_meta_only_when_it_changes() {
        let trace = vec![jiffy(3), jiffy(3), jiffy(5)];
        let events = build_events(&trace, &program_mapping(5, 5));
        let tempos: Vec<_> = events.iter().filter(|e| matches!(e.kind, EventKind::Tempo { .. })).collect();
        assert_eq!(tempos.len(), 2, "only the actual change re-emits Tempo");
        assert_eq!(tempos[0].tick, 0);
        assert_eq!(tempos[1].tick, 2);
    }

    #[test]
    fn write_smf_round_trips_through_midly_parse() {
        let trace = vec![
            jiffy(0),
            trigger(0, 5, 0x18, 64),
            jiffy(0),
            trigger(1, 5, 0x1E, 32),
            jiffy(0),
            key_up(0),
            key_up(1),
        ];
        let events = build_events(&trace, &program_mapping(5, 5));

        let mut bytes = Vec::new();
        write_smf(&events, &mut bytes).unwrap();

        let smf = midly::Smf::parse(&bytes).unwrap();
        assert_eq!(smf.header.format, midly::Format::SingleTrack);
        assert_eq!(smf.header.timing, midly::Timing::Metrical(midly::num::u15::new(PPQ)));
        assert_eq!(smf.tracks.len(), 1);

        let note_ons = smf.tracks[0]
            .iter()
            .filter(|e| {
                matches!(e.kind, midly::TrackEventKind::Midi { message: midly::MidiMessage::NoteOn { .. }, .. })
            })
            .count();
        assert_eq!(note_ons, 2, "one NoteOn per Trigger, round-tripped intact");

        assert!(matches!(
            smf.tracks[0].last().unwrap().kind,
            midly::TrackEventKind::Meta(midly::MetaMessage::EndOfTrack)
        ));
    }

    #[test]
    fn midi_note_for_maps_the_middle_c_anchor_to_midi_60() {
        assert_eq!(midi_note_for(0x18, 0, 0), 60);
        assert_eq!(midi_note_for(0x18 + 12, 0, 0), 72, "an octave up is 12 semitones up");
        assert_eq!(midi_note_for(0x18, 12, 0), 72, "trace transpose is semitone-linear too");
        assert_eq!(midi_note_for(0x18, 0, -12), 48, "zone transpose shifts down an octave");
    }
}
