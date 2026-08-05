//! A bounded linear listing, top to bottom, of one macro's or one pattern's
//! bytecode -- not an execution trace (`tfmx-cli trace` is that). Structured
//! so a future GUI (`docs/gui-plan.md` Phase G1) can render it without going
//! through `tfmx-cli`'s printed text; `tfmx-cli disasm` now just formats
//! these lines.
//!
//! `DisasmLine` itself embeds [`PatternEntry`] directly, unmirrored, since
//! `tfmx-cli`'s text formatting only needs the plain Rust type. [`DisasmLineView`]
//! is the JSON-facing mirror the `/disasm` route (`docs/gui-plan.md` Phase W1)
//! serializes instead, the same way `view.rs`'s `TrackSlotView` mirrors
//! `TrackSlot` -- the core crate stays `serde`-free, so anything embedding one
//! of its types needs a local mirror to gain `Serialize`.

use tfmx::{AccessError, Module, NoteTiming, PatternCommand, PatternEntry, decode_pattern_entry};

/// Stops at the opcode's own natural terminator (`$07 STOP` for a macro,
/// `$F0 End`/`$F4 STOP` for a pattern) or after `MAX_DISASM_STEPS`, whichever
/// comes first -- `pattern()`/`macro_()` return raw bytes "to the end of
/// mdat" with no length field, so an untrusted or malformed module could
/// otherwise never terminate this loop.
const MAX_DISASM_STEPS: usize = 256;

/// One decoded step of a macro's or a pattern's bytecode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisasmLine {
    Macro {
        step: usize,
        opcode: u8,
        aa: u8,
        bb: u8,
        cc: u8,
    },
    Pattern {
        step: usize,
        entry: PatternEntry,
    },
}

/// Disassembles macro `macro_number`'s bytecode into a linear listing.
pub fn disassemble_macro(
    module: &Module,
    macro_number: u8,
) -> Result<Vec<DisasmLine>, AccessError> {
    let bytes = module.macro_(macro_number)?;
    let mut lines = Vec::new();
    for (step, word) in bytes.chunks_exact(4).take(MAX_DISASM_STEPS).enumerate() {
        let [opcode, aa, bb, cc] = [word[0], word[1], word[2], word[3]];
        lines.push(DisasmLine::Macro {
            step,
            opcode,
            aa,
            bb,
            cc,
        });
        if opcode == 0x07 {
            break;
        }
    }
    Ok(lines)
}

/// Disassembles pattern `pattern`'s bytecode into a linear listing.
pub fn disassemble_pattern(module: &Module, pattern: u8) -> Result<Vec<DisasmLine>, AccessError> {
    let bytes = module.pattern(pattern)?;
    let mut lines = Vec::new();
    for (step, word) in bytes.chunks_exact(4).take(MAX_DISASM_STEPS).enumerate() {
        let entry = decode_pattern_entry([word[0], word[1], word[2], word[3]]);
        let stop = matches!(
            entry,
            PatternEntry::Command(PatternCommand::End | PatternCommand::Stop)
        );
        lines.push(DisasmLine::Pattern { step, entry });
        if stop {
            break;
        }
    }
    Ok(lines)
}

/// JSON-serializable mirror of [`DisasmLine`] (`docs/gui-plan.md`'s
/// `/disasm` route).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum DisasmLineView {
    Macro {
        step: usize,
        opcode: u8,
        aa: u8,
        bb: u8,
        cc: u8,
    },
    Pattern {
        step: usize,
        entry: PatternEntryView,
    },
}

impl From<DisasmLine> for DisasmLineView {
    fn from(line: DisasmLine) -> Self {
        match line {
            DisasmLine::Macro {
                step,
                opcode,
                aa,
                bb,
                cc,
            } => DisasmLineView::Macro {
                step,
                opcode,
                aa,
                bb,
                cc,
            },
            DisasmLine::Pattern { step, entry } => DisasmLineView::Pattern {
                step,
                entry: entry.into(),
            },
        }
    }
}

/// Mirrors [`NoteTiming`] for serialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum NoteTimingView {
    Detune(i8),
    Wait(u8),
    Portamento(u8),
}

impl From<NoteTiming> for NoteTimingView {
    fn from(timing: NoteTiming) -> Self {
        match timing {
            NoteTiming::Detune(v) => NoteTimingView::Detune(v),
            NoteTiming::Wait(v) => NoteTimingView::Wait(v),
            NoteTiming::Portamento(v) => NoteTimingView::Portamento(v),
        }
    }
}

/// Mirrors [`PatternEntry`] for serialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum PatternEntryView {
    Note {
        note: u8,
        macro_number: u8,
        volume: u8,
        voice: u8,
        timing: NoteTimingView,
    },
    Command(PatternCommandView),
}

impl From<PatternEntry> for PatternEntryView {
    fn from(entry: PatternEntry) -> Self {
        match entry {
            PatternEntry::Note {
                note,
                macro_number,
                volume,
                voice,
                timing,
            } => PatternEntryView::Note {
                note,
                macro_number,
                volume,
                voice,
                timing: timing.into(),
            },
            PatternEntry::Command(command) => PatternEntryView::Command(command.into()),
        }
    }
}

/// Mirrors [`PatternCommand`] for serialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum PatternCommandView {
    End,
    Loop {
        times: u8,
        target: u16,
    },
    Jump {
        pattern: u8,
        step: u16,
    },
    Wait {
        jiffies: u8,
    },
    Stop,
    KeyUp {
        voice: u8,
    },
    Vibrato {
        speed: u8,
        voice: u8,
        depth: u8,
    },
    Envelope {
        amount: u8,
        speed: u8,
        voice: u8,
        target: u8,
    },
    GoSub {
        pattern: u8,
        step: u16,
    },
    Return,
    Fade {
        speed: u8,
        target: u8,
    },
    PlayPattern {
        pattern: u8,
        track: u8,
        transpose: i8,
    },
    Portamento {
        speed: u8,
        voice: u8,
        rate: u8,
    },
    Lock {
        channel: u8,
        ticks: u16,
    },
    StopCustom,
    Nop,
}

impl From<PatternCommand> for PatternCommandView {
    fn from(command: PatternCommand) -> Self {
        match command {
            PatternCommand::End => PatternCommandView::End,
            PatternCommand::Loop { times, target } => PatternCommandView::Loop { times, target },
            PatternCommand::Jump { pattern, step } => PatternCommandView::Jump { pattern, step },
            PatternCommand::Wait { jiffies } => PatternCommandView::Wait { jiffies },
            PatternCommand::Stop => PatternCommandView::Stop,
            PatternCommand::KeyUp { voice } => PatternCommandView::KeyUp { voice },
            PatternCommand::Vibrato {
                speed,
                voice,
                depth,
            } => PatternCommandView::Vibrato {
                speed,
                voice,
                depth,
            },
            PatternCommand::Envelope {
                amount,
                speed,
                voice,
                target,
            } => PatternCommandView::Envelope {
                amount,
                speed,
                voice,
                target,
            },
            PatternCommand::GoSub { pattern, step } => PatternCommandView::GoSub { pattern, step },
            PatternCommand::Return => PatternCommandView::Return,
            PatternCommand::Fade { speed, target } => PatternCommandView::Fade { speed, target },
            PatternCommand::PlayPattern {
                pattern,
                track,
                transpose,
            } => PatternCommandView::PlayPattern {
                pattern,
                track,
                transpose,
            },
            PatternCommand::Portamento { speed, voice, rate } => {
                PatternCommandView::Portamento { speed, voice, rate }
            }
            PatternCommand::Lock { channel, ticks } => PatternCommandView::Lock { channel, ticks },
            PatternCommand::StopCustom => PatternCommandView::StopCustom,
            PatternCommand::Nop => PatternCommandView::Nop,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tfmx::NoteTiming;

    fn read_corpus(name: &str) -> Option<Vec<u8>> {
        let path = format!("{}/../testdata/{}", env!("CARGO_MANIFEST_DIR"), name);
        fs::read(path).ok()
    }

    /// `turrican intro`'s macro 24 -- the keysplit/`Cont` instrument at the
    /// centre of an earlier session's retrigger fix (`MacroInterpreter::instrument`).
    /// Fixes this exact bytecode as a regression check: a Splitkey into two
    /// `Cont`s, terminated by `STOP`.
    #[test]
    fn disassemble_macro_lists_a_splitkey_cont_chain_and_stops_at_stop() {
        let Some(mdat) = read_corpus("mdat.turrican intro") else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let smpl = read_corpus("smpl.turrican intro").expect("smpl present alongside mdat");
        let module = Module::parse(&mdat, &smpl).expect("valid corpus file");

        let lines = disassemble_macro(&module, 24).expect("macro 24 exists");

        assert_eq!(lines.len(), 4, "must stop right after the STOP at step 3");
        assert!(matches!(
            lines[0],
            DisasmLine::Macro {
                step: 0,
                opcode: 0x1C,
                ..
            }
        ));
        assert!(matches!(lines[1], DisasmLine::Macro { opcode: 0x06, .. }));
        assert!(matches!(lines[2], DisasmLine::Macro { opcode: 0x06, .. }));
        assert!(matches!(lines[3], DisasmLine::Macro { opcode: 0x07, .. }));
    }

    /// Cross-checked against `tfmx-cli trace`'s own repeated decode of this
    /// step across many sessions: pattern 84 step 0 is always
    /// `Note{note:33, macro:48, volume:12, voice:2, Wait(31)}`.
    #[test]
    fn disassemble_pattern_matches_the_known_decode_of_pattern_84_step_0() {
        let Some(mdat) = read_corpus("mdat.turrican intro") else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let smpl = read_corpus("smpl.turrican intro").expect("smpl present alongside mdat");
        let module = Module::parse(&mdat, &smpl).expect("valid corpus file");

        let lines = disassemble_pattern(&module, 84).expect("pattern 84 exists");

        assert_eq!(
            lines[0],
            DisasmLine::Pattern {
                step: 0,
                entry: PatternEntry::Note {
                    note: 33,
                    macro_number: 48,
                    volume: 12,
                    voice: 2,
                    timing: NoteTiming::Wait(31),
                },
            }
        );
    }

    #[cfg(feature = "serde")]
    #[test]
    fn disasm_line_view_serializes_a_pattern_note_to_the_expected_json_shape() {
        let line = DisasmLine::Pattern {
            step: 0,
            entry: PatternEntry::Note {
                note: 33,
                macro_number: 48,
                volume: 12,
                voice: 2,
                timing: NoteTiming::Wait(31),
            },
        };
        let json = serde_json::to_string(&DisasmLineView::from(line)).expect("serializes");
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(
            value["Pattern"]["entry"]["Note"]["timing"],
            serde_json::json!({ "Wait": 31 })
        );
    }
}
