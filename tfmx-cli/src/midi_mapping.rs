//! Hand-editable JSON mapping from 5.3's zone tables to MIDI output --
//! `(macro, note range, velocity range) -> program | drum note | drop`,
//! auto-drafted from `tfmx_analysis::resolve_zones` and meant to be
//! hand-edited afterwards (consolidate zones into fewer programs, route some
//! to drums, drop unwanted ones). `docs/m5-plan.md` Phase 5.5.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use tfmx::Module;
use tfmx_analysis::{WalkResult, ZoneTable};

/// General MIDI's percussion channel, 0-based (channel 10 in 1-based MIDI
/// terms) -- the target for [`ZoneOutput::Drum`].
pub const DRUM_CHANNEL: u8 = 9;

/// What a zone's `(note, volume)` rectangle turns into on export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum ZoneOutput {
    /// A pitched instrument: MIDI note number tracks the TFMX note
    /// (`docs/m5-session-log.md`'s anchor: raw note `0x18` = MIDI 60), the
    /// zone's own `transpose` added on top, `program` sent once per channel.
    Program { program: u8 },
    /// A percussion hit: fixed MIDI note `note` on [`DRUM_CHANNEL`],
    /// regardless of the triggering TFMX note.
    Drum { note: u8 },
    /// Dropped -- no MIDI event emitted for triggers landing in this zone.
    Drop,
}

/// One zone, in the mapping file's own units (plain inclusive ranges, not
/// `tfmx_analysis::Zone`'s richer per-path state -- this file is meant to be
/// hand-edited, so it only keeps what a user needs to see and change).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MappingZone {
    pub notes: (u8, u8),
    pub volumes: (u8, u8),
    pub output: ZoneOutput,
    /// Semitones added on top of the TFMX note, [`ZoneOutput::Program`] only.
    #[serde(default)]
    pub transpose: i8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MacroMapping {
    pub zones: Vec<MappingZone>,
}

impl MacroMapping {
    /// The zone covering `(note, volume)`, if any. Zones drafted from
    /// `resolve_zones` are disjoint and exhaustive over the full `(note,
    /// volume)` rectangle, but a hand-edited file need not preserve that --
    /// the first matching zone wins, and no match means the trigger is
    /// silently dropped.
    pub fn zone_for(&self, note: u8, volume: u8) -> Option<&MappingZone> {
        self.zones.iter().find(|z| {
            z.notes.0 <= note
                && note <= z.notes.1
                && z.volumes.0 <= volume
                && volume <= z.volumes.1
        })
    }
}

/// The full mapping, one [`MacroMapping`] per TFMX macro number.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MidiMapping {
    pub macros: BTreeMap<u8, MacroMapping>,
}

impl MidiMapping {
    pub fn zone_for(&self, macro_number: u8, note: u8, volume: u8) -> Option<&MappingZone> {
        self.macros.get(&macro_number)?.zone_for(note, volume)
    }
}

/// Auto-drafts a mapping from a song's static walk: one entry per reachable
/// macro, `program` defaulted to the macro number itself, zones copied
/// verbatim from [`tfmx_analysis::resolve_zones`]. A starting point, not a
/// final answer.
pub fn draft_mapping(module: &Module, walk: &WalkResult) -> MidiMapping {
    let mut macros = BTreeMap::new();
    for &macro_number in &walk.reachable_macros {
        if let Ok(table) = tfmx_analysis::resolve_zones(module, macro_number) {
            macros.insert(macro_number, draft_macro_mapping(&table));
        }
    }
    MidiMapping { macros }
}

fn draft_macro_mapping(table: &ZoneTable) -> MacroMapping {
    let zones = table
        .zones
        .iter()
        .map(|z| MappingZone {
            notes: (*z.notes.start(), *z.notes.end()),
            volumes: (*z.volumes.start(), *z.volumes.end()),
            output: ZoneOutput::Program {
                program: table.macro_number,
            },
            transpose: 0,
        })
        .collect();
    MacroMapping { zones }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program(n: u8) -> ZoneOutput {
        ZoneOutput::Program { program: n }
    }

    fn zone(notes: (u8, u8), volumes: (u8, u8), output: ZoneOutput) -> MappingZone {
        MappingZone { notes, volumes, output, transpose: 0 }
    }

    #[test]
    fn zone_for_finds_the_matching_rectangle() {
        let mapping = MacroMapping {
            zones: vec![
                zone((0, 31), (0, 64), program(1)),
                zone((32, 63), (0, 64), program(2)),
            ],
        };
        assert_eq!(mapping.zone_for(10, 40).unwrap().output, program(1));
        assert_eq!(mapping.zone_for(40, 40).unwrap().output, program(2));
        assert_eq!(mapping.zone_for(31, 64).unwrap().output, program(1));
    }

    #[test]
    fn zone_for_returns_none_outside_every_zone() {
        let mapping = MacroMapping {
            zones: vec![zone((0, 10), (0, 10), program(1))],
        };
        assert!(mapping.zone_for(20, 5).is_none());
    }

    #[test]
    fn midi_mapping_zone_for_looks_up_the_right_macro() {
        let mut macros = BTreeMap::new();
        macros.insert(
            5,
            MacroMapping { zones: vec![zone((0, 63), (0, 64), program(5))] },
        );
        let mapping = MidiMapping { macros };
        assert!(mapping.zone_for(5, 10, 10).is_some());
        assert!(mapping.zone_for(6, 10, 10).is_none(), "macro 6 not in the mapping");
    }

    #[test]
    fn mapping_round_trips_through_json() {
        let mut macros = BTreeMap::new();
        macros.insert(
            0,
            MacroMapping {
                zones: vec![
                    zone((0, 31), (0, 32), ZoneOutput::Drop),
                    zone((32, 63), (0, 64), ZoneOutput::Drum { note: 36 }),
                ],
            },
        );
        let mapping = MidiMapping { macros };

        let json = serde_json::to_string_pretty(&mapping).unwrap();
        let round_tripped: MidiMapping = serde_json::from_str(&json).unwrap();
        assert_eq!(round_tripped, mapping);
    }

    #[test]
    fn draft_mapping_covers_every_reachable_macro_with_zones_from_resolve_zones() {
        let Some(mdat) = corpus_path("mdat.turrican intro") else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let smpl = corpus_path("smpl.turrican intro").expect("smpl present alongside mdat");
        let mdat_bytes = std::fs::read(mdat).unwrap();
        let smpl_bytes = std::fs::read(smpl).unwrap();
        let module = tfmx::Module::parse(&mdat_bytes, &smpl_bytes).unwrap();
        let walk = tfmx_analysis::walk_song(&module, 0).unwrap();

        let mapping = draft_mapping(&module, &walk);

        assert_eq!(mapping.macros.len(), walk.reachable_macros.len());
        for &macro_number in &walk.reachable_macros {
            let table = tfmx_analysis::resolve_zones(&module, macro_number).unwrap();
            let drafted = &mapping.macros[&macro_number];
            assert_eq!(drafted.zones.len(), table.zones.len());
            for (z, mz) in table.zones.iter().zip(&drafted.zones) {
                assert_eq!(mz.notes, (*z.notes.start(), *z.notes.end()));
                assert_eq!(mz.volumes, (*z.volumes.start(), *z.volumes.end()));
                assert_eq!(mz.output, program(macro_number));
            }
        }
    }

    fn corpus_path(name: &str) -> Option<std::path::PathBuf> {
        let path = std::path::PathBuf::from(format!(
            "{}/../testdata/{}",
            env!("CARGO_MANIFEST_DIR"),
            name
        ));
        path.exists().then_some(path)
    }
}
