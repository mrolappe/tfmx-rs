//! SFZ export -- one `.sfz` file per macro, one `<region>` per zone,
//! `lokey`/`hikey`/`lovel`/`hivel` mapped one-to-one from the zone table.
//! Imported natively by Kontakt; playable in sfizz (free VST3/AU/LV2), the
//! route into Ableton Live. `docs/m5-plan.md` Phase 5.7.

use std::fmt::Write as _;
use std::io;
use std::path::Path;

use super::wav::{self, write_wav};
use super::{Instrument, InstrumentSerializer, NATIVE_SAMPLE_RATE_HZ};

pub struct SfzSerializer;

impl InstrumentSerializer for SfzSerializer {
    fn name(&self) -> &'static str {
        "sfz"
    }

    fn serialize(&self, instrument: &Instrument, out_dir: &Path) -> io::Result<()> {
        std::fs::create_dir_all(out_dir)?;
        let mut sfz = String::from("<group>\n");
        for zone in &instrument.zones {
            let filename = wav::zone_filename(instrument.macro_number, zone.index);
            write_wav(&out_dir.join(&filename), zone.pcm, NATIVE_SAMPLE_RATE_HZ, zone.looped)?;
            let loop_mode = if zone.looped { "loop_continuous" } else { "no_loop" };
            let _ = writeln!(
                sfz,
                "<region> sample={filename} lokey={} hikey={} lovel={} hivel={} \
                 pitch_keycenter={} loop_mode={loop_mode}",
                zone.lokey, zone.hikey, zone.lovel, zone.hivel, zone.pitch_keycenter,
            );
        }
        std::fs::write(out_dir.join(format!("macro{}.sfz", instrument.macro_number)), sfz)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::InstrumentZone;

    #[test]
    fn one_region_per_zone_with_matching_key_and_velocity_ranges() {
        let dir = std::env::temp_dir().join("tfmx-export-sfz-test");
        let pcm = [0i8, 1, 2, 3];
        let instrument = Instrument {
            macro_number: 5,
            zones: vec![
                InstrumentZone {
                    lokey: 36,
                    hikey: 59,
                    lovel: 1,
                    hivel: 63,
                    pitch_keycenter: 60,
                    looped: false,
                    pcm: &pcm,
                    index: 0,
                },
                InstrumentZone {
                    lokey: 60,
                    hikey: 99,
                    lovel: 64,
                    hivel: 127,
                    pitch_keycenter: 60,
                    looped: true,
                    pcm: &pcm,
                    index: 1,
                },
            ],
        };

        SfzSerializer.serialize(&instrument, &dir).unwrap();
        let text = std::fs::read_to_string(dir.join("macro5.sfz")).unwrap();

        assert_eq!(text.matches("<region>").count(), 2);
        assert!(text.contains("lokey=36 hikey=59 lovel=1 hivel=63"));
        assert!(text.contains("loop_mode=no_loop"));
        assert!(text.contains("lokey=60 hikey=99 lovel=64 hivel=127"));
        assert!(text.contains("loop_mode=loop_continuous"));
        assert!(dir.join("macro5_zone0.wav").exists());
        assert!(dir.join("macro5_zone1.wav").exists());
    }
}
