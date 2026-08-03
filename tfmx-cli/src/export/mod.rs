//! Sample and sampler-instrument export: one `InstrumentSerializer` per
//! output format, all built over 5.3's zone table (`tfmx_analysis::Zone`).
//! `docs/m5-plan.md` Phase 5.7.
//!
//! Adding a format is one new file implementing [`InstrumentSerializer`]
//! plus one line in [`by_name`] -- nothing else in this crate changes.

mod dspreset;
mod sfz;
mod wav;

use std::io;
use std::path::Path;

use tfmx::Module;

/// TFMX's own pitch anchor (`crate::midi::MIDDLE_C_TFMX`/`MIDDLE_C_MIDI`,
/// originally `tfmx/src/macro_interp.rs`'s `MIDDLE_C_NOTE`): raw note
/// `0x18` plays a zone's sample at its native rate, `8363` Hz. Every
/// exported sample is written at that rate, so MIDI note 60 is always the
/// sampler's pitch-keycenter, regardless of which zone it came from -- the
/// same anchor `tfmx-cli/src/midi.rs`'s export already uses.
pub const NATIVE_SAMPLE_RATE_HZ: u32 = 8363;

fn midi_note(tfmx_note: u8) -> u8 {
    (crate::midi::MIDDLE_C_MIDI + tfmx_note as i32 - crate::midi::MIDDLE_C_TFMX).clamp(0, 127) as u8
}

/// One playable region of one instrument (macro), resolved to raw PCM plus
/// the key/velocity range and loop state a sampler format needs. Built by
/// [`build_instrument`] from a [`tfmx_analysis::Zone`]; zones with no
/// sample region (`Zone::sample == None`, e.g. a keysplit target that is
/// itself another macro) contribute nothing and are skipped.
pub struct InstrumentZone<'a> {
    pub lokey: u8,
    pub hikey: u8,
    pub lovel: u8,
    pub hivel: u8,
    pub pitch_keycenter: u8,
    /// Whether the region should loop indefinitely (`$18 <Sampleloop>` was
    /// armed) or play once.
    pub looped: bool,
    pub pcm: &'a [i8],
    /// Unique within one [`Instrument`], used to name the per-zone sample
    /// file.
    pub index: usize,
}

/// One macro's zone table, resolved to exportable sample data.
pub struct Instrument<'a> {
    pub macro_number: u8,
    pub zones: Vec<InstrumentZone<'a>>,
}

/// Resolves `macro_number`'s zone table ([`tfmx_analysis::resolve_zones`])
/// into an [`Instrument`], mapping each zone's note/volume rectangle onto
/// MIDI key/velocity ranges and fetching its live sample bytes from
/// `module`. A zone whose sample region does not fit `module`'s `smpl`
/// buffer is skipped rather than failing the whole instrument -- the same
/// resilience `tfmx-cli lint`'s `sample-region-out-of-bounds` finding
/// already treats as a per-zone, not per-module, problem.
pub fn build_instrument<'a>(
    module: &'a Module<'a>,
    macro_number: u8,
) -> Result<Instrument<'a>, tfmx::AccessError> {
    let table = tfmx_analysis::resolve_zones(module, macro_number)?;
    let mut zones = Vec::new();
    for (index, zone) in table.zones.iter().enumerate() {
        let Some(region) = &zone.sample else {
            continue;
        };
        let Ok(pcm) = module.sample(region.start, region.len) else {
            continue;
        };
        zones.push(InstrumentZone {
            lokey: midi_note(*zone.notes.start()),
            hikey: midi_note(*zone.notes.end()),
            lovel: crate::midi::velocity_for(*zone.volumes.start()),
            hivel: crate::midi::velocity_for(*zone.volumes.end()),
            pitch_keycenter: crate::midi::MIDDLE_C_MIDI as u8,
            looped: region.looped,
            pcm,
            index,
        });
    }
    Ok(Instrument { macro_number, zones })
}

/// Writes `instrument`'s export in one format to `out_dir`.
pub trait InstrumentSerializer {
    fn name(&self) -> &'static str;
    fn serialize(&self, instrument: &Instrument, out_dir: &Path) -> io::Result<()>;
}

/// The format registry: every [`InstrumentSerializer`] this crate ships,
/// looked up by its `--format` name.
pub fn by_name(name: &str) -> Option<Box<dyn InstrumentSerializer>> {
    match name {
        "wav" => Some(Box::new(wav::WavSerializer)),
        "sfz" => Some(Box::new(sfz::SfzSerializer)),
        "dspreset" => Some(Box::new(dspreset::DspresetSerializer)),
        _ => None,
    }
}

pub const FORMAT_NAMES: &[&str] = &["wav", "sfz", "dspreset"];

#[cfg(test)]
mod tests {
    use super::*;

    fn read_corpus(name: &str) -> Option<Vec<u8>> {
        let path = format!("{}/../testdata/{}", env!("CARGO_MANIFEST_DIR"), name);
        std::fs::read(path).ok()
    }

    #[test]
    fn midi_note_maps_the_middle_c_anchor_and_clamps() {
        assert_eq!(midi_note(0x18), 60);
        assert_eq!(midi_note(0), 36);
        assert_eq!(midi_note(0x3F), 99);
    }

    /// Macro 28 of `turrican intro` is a single unsplit zone whose chain
    /// ends on `$18 Sampleloop` (`tfmx-analysis`'s own
    /// `turrican_intro_macro_28_is_a_single_unsplit_zone` pins the exact
    /// region: start `0x7F04`, len `0x100` bytes, looped).
    #[test]
    fn build_instrument_resolves_macro_28_to_one_looped_zone() {
        let Some(mdat) = read_corpus("mdat.turrican intro") else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let smpl = read_corpus("smpl.turrican intro").expect("smpl alongside mdat");
        let module = Module::parse(&mdat, &smpl).expect("valid module");

        let instrument = build_instrument(&module, 28).expect("macro 28 in range");

        assert_eq!(instrument.macro_number, 28);
        assert_eq!(instrument.zones.len(), 1);
        let zone = &instrument.zones[0];
        assert_eq!(zone.lokey, 36, "raw note 0 -> MIDI 36");
        assert_eq!(zone.hikey, 99, "raw note 0x3F -> MIDI 99");
        assert_eq!(zone.lovel, 1, "raw volume 0 clamps to the MIDI-velocity floor");
        assert_eq!(zone.hivel, 127);
        assert_eq!(zone.pitch_keycenter, 60);
        assert!(zone.looped, "macro 28's chain ends on $18 Sampleloop");
        assert_eq!(zone.pcm.len(), 0x100);
    }

    #[test]
    fn by_name_covers_every_advertised_format() {
        for name in FORMAT_NAMES {
            assert!(by_name(name).is_some(), "{name} missing from the registry");
        }
        assert!(by_name("nki").is_none(), "Kontakt .nki is explicitly out of scope");
    }
}
