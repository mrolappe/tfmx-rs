//! A bounded linear listing, top to bottom, of one macro's or one pattern's
//! bytecode -- not an execution trace (`tfmx-cli trace` is that). Structured
//! so a future GUI (`docs/gui-plan.md` Phase G1) can render it without going
//! through `tfmx-cli`'s printed text; `tfmx-cli disasm` now just formats
//! these lines.
//!
//! `DisasmLine::Pattern` embeds [`PatternEntry`] directly rather than
//! mirroring it into a local view type (unlike `view.rs`'s `TrackSlotView`):
//! nothing here needs JSON output yet, so there's no `serde` gating to
//! satisfy -- add a mirror type if/when that's needed.

use tfmx::{AccessError, Module, PatternCommand, PatternEntry, decode_pattern_entry};

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
}
