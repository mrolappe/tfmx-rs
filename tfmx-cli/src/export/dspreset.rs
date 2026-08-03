//! DecentSampler `.dspreset` export -- plain XML, free cross-platform
//! plugin, keeps key/velocity ranges and loop points. The zero-friction
//! "drop it in and play" path. `docs/m5-plan.md` Phase 5.7.

use std::fmt::Write as _;
use std::io;
use std::path::Path;

use super::wav::{self, write_wav};
use super::{Instrument, InstrumentSerializer, NATIVE_SAMPLE_RATE_HZ};

pub struct DspresetSerializer;

impl InstrumentSerializer for DspresetSerializer {
    fn name(&self) -> &'static str {
        "dspreset"
    }

    fn serialize(&self, instrument: &Instrument, out_dir: &Path) -> io::Result<()> {
        std::fs::create_dir_all(out_dir)?;
        let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<DecentSampler>\n  <groups>\n    <group>\n");
        for zone in &instrument.zones {
            let filename = wav::zone_filename(instrument.macro_number, zone.index);
            write_wav(&out_dir.join(&filename), zone.pcm, NATIVE_SAMPLE_RATE_HZ, zone.looped)?;
            let last_frame = zone.pcm.len().saturating_sub(1);
            let _ = writeln!(
                xml,
                "      <sample path=\"{filename}\" rootNote=\"{}\" loNote=\"{}\" hiNote=\"{}\" \
                 loVel=\"{}\" hiVel=\"{}\" loopEnabled=\"{}\" loopStart=\"0\" loopEnd=\"{last_frame}\"/>",
                zone.pitch_keycenter, zone.lokey, zone.hikey, zone.lovel, zone.hivel, zone.looped,
            );
        }
        xml.push_str("    </group>\n  </groups>\n</DecentSampler>\n");
        std::fs::write(
            out_dir.join(format!("macro{}.dspreset", instrument.macro_number)),
            xml,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::InstrumentZone;

    #[test]
    fn one_sample_element_per_zone_with_matching_key_and_velocity_ranges() {
        let dir = std::env::temp_dir().join("tfmx-export-dspreset-test");
        let pcm = [0i8, 1, 2, 3, 4];
        let instrument = Instrument {
            macro_number: 7,
            zones: vec![InstrumentZone {
                lokey: 36,
                hikey: 99,
                lovel: 1,
                hivel: 127,
                pitch_keycenter: 60,
                looped: true,
                pcm: &pcm,
                index: 0,
            }],
        };

        DspresetSerializer.serialize(&instrument, &dir).unwrap();
        let text = std::fs::read_to_string(dir.join("macro7.dspreset")).unwrap();

        assert_eq!(text.matches("<sample ").count(), 1);
        assert!(text.contains("path=\"macro7_zone0.wav\""));
        assert!(text.contains("rootNote=\"60\""));
        assert!(text.contains("loNote=\"36\" hiNote=\"99\""));
        assert!(text.contains("loVel=\"1\" hiVel=\"127\""));
        assert!(text.contains("loopEnabled=\"true\""));
        assert!(text.contains("loopStart=\"0\" loopEnd=\"4\""));
        assert!(text.starts_with("<?xml"));
        assert!(text.contains("</DecentSampler>"));
        assert!(dir.join("macro7_zone0.wav").exists());
    }
}
