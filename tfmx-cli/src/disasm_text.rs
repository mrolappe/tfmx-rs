//! Renders a [`tfmx_analysis::DisasmLine`] listing to `disasm`'s plain-text
//! format (`docs/gui-plan.md` Phase W3: shared by the CLI's own `disasm`
//! subcommand and the GUI's `/disasm-text` route, so the opcode mnemonic
//! table lives in exactly one place).

/// `docs/opcodes.md` §3's macro opcode mnemonics, `$00`-`$21`. Name only --
/// operand semantics vary per opcode (signed/unsigned, 8/16/24-bit) and are
/// already implemented once in `MacroInterpreter::execute`; re-deriving them
/// here would duplicate that match arm-for-arm. A step's raw `aa bb cc`
/// bytes are printed alongside the name instead, matching what the docs
/// table itself shows.
///
/// ponytail: name-only, not a decoded operand enum like `PatternEntry` --
/// upgrade to one (mirroring `sequencer::decode_pattern_entry`) if a
/// consumer ever needs structured macro operands, not just a printable
/// listing.
fn macro_opcode_name(op: u8) -> &'static str {
    match op {
        0x00 => "DMAoff+Reset*",
        0x01 => "DMAon",
        0x02 => "SetBegin",
        0x03 => "SetLen",
        0x04 => "Wait*",
        0x05 => "Loop",
        0x06 => "Cont",
        0x07 => "STOP*",
        0x08 => "AddNote*",
        0x09 => "SetNote*",
        0x0A => "Reset",
        0x0B => "Portamento",
        0x0C => "Vibrato",
        0x0D => "AddVolume",
        0x0E => "SetVolume",
        0x0F => "Envelope",
        0x10 => "Loop key up",
        0x11 => "AddBegin",
        0x12 => "AddLen",
        0x13 => "DMAoff*",
        0x14 => "Wait key up*",
        0x15 => "Go submacro",
        0x16 => "Return to old macro",
        0x17 => "Set period*",
        0x18 => "Sampleloop",
        0x19 => "Set one shot sample",
        0x1A => "Wait on DMA*",
        0x1B => "Random play",
        0x1C => "Splitkey",
        0x1D => "Splitvol",
        0x1E => "AddVol+Note*",
        0x1F => "SetPrevNote*",
        0x20 => "Signal",
        0x21 => "Play macro",
        _ => "?",
    }
}

/// Formats one structured disassembly line back to `disasm`'s exact text
/// (macro-opcode name lookup stays here since it's a display concern, not
/// decoded data).
pub fn format_disasm_line(line: &tfmx_analysis::DisasmLine) -> String {
    match line {
        tfmx_analysis::DisasmLine::Macro {
            step,
            opcode,
            aa,
            bb,
            cc,
        } => format!(
            "{step:4}: ${opcode:02X} <{}> aa=${aa:02X} bb=${bb:02X} cc=${cc:02X}",
            macro_opcode_name(*opcode)
        ),
        tfmx_analysis::DisasmLine::Pattern { step, entry } => format!("{step:4}: {entry:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_disasm_line_renders_macro_and_pattern_steps() {
        let macro_line = tfmx_analysis::DisasmLine::Macro {
            step: 0,
            opcode: 0x1C,
            aa: 0x05,
            bb: 0x00,
            cc: 0x00,
        };
        assert_eq!(
            format_disasm_line(&macro_line),
            "   0: $1C <Splitkey> aa=$05 bb=$00 cc=$00"
        );

        let pattern_line = tfmx_analysis::DisasmLine::Pattern {
            step: 0,
            entry: tfmx::PatternEntry::Note {
                note: 33,
                macro_number: 48,
                volume: 12,
                voice: 2,
                timing: tfmx::NoteTiming::Wait(31),
            },
        };
        assert_eq!(
            format_disasm_line(&pattern_line),
            "   0: Note { note: 33, macro_number: 48, volume: 12, voice: 2, timing: Wait(31) }"
        );
    }
}
