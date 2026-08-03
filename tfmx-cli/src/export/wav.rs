//! WAV writer with a `smpl` loop chunk -- loop points survive into
//! hardware/software samplers. `hound` (this crate's other WAV writer, used
//! for rendered audio) writes plain `fmt`/`data` only and has no support for
//! extra chunks, so this is a small hand-rolled RIFF writer.
//! `docs/m5-plan.md` Phase 5.7.

use std::io;
use std::path::Path;

use super::{Instrument, InstrumentSerializer, NATIVE_SAMPLE_RATE_HZ};

pub struct WavSerializer;

impl InstrumentSerializer for WavSerializer {
    fn name(&self) -> &'static str {
        "wav"
    }

    fn serialize(&self, instrument: &Instrument, out_dir: &Path) -> io::Result<()> {
        std::fs::create_dir_all(out_dir)?;
        for zone in &instrument.zones {
            let path = out_dir.join(zone_filename(instrument.macro_number, zone.index));
            write_wav(&path, zone.pcm, NATIVE_SAMPLE_RATE_HZ, zone.looped)?;
        }
        Ok(())
    }
}

/// The per-zone sample file name every serializer that references WAVs
/// (SFZ, DecentSampler) writes into the same `out_dir` as its own
/// instrument-definition file.
pub(crate) fn zone_filename(macro_number: u8, index: usize) -> String {
    format!("macro{macro_number}_zone{index}.wav")
}

/// Writes signed 8-bit mono PCM as a WAV file: `fmt `/`data`, plus a `smpl`
/// chunk looping the whole clip when `looped` (TFMX's `$18 <Sampleloop>`
/// was armed for this region -- otherwise the clip is meant to play once
/// and no loop chunk is written).
pub fn write_wav(path: &Path, pcm: &[i8], sample_rate: u32, looped: bool) -> io::Result<()> {
    // 8-bit WAV PCM is unsigned with a 128 bias -- the same reinterpret
    // `hound`'s own `Sample for i8` impl performs.
    let data: Vec<u8> = pcm.iter().map(|&s| (s as u8).wrapping_add(0x80)).collect();
    let data_padded_len = data.len() + (data.len() % 2);

    const FMT_LEN: u32 = 16;
    const SMPL_LOOPS: u32 = 1;
    let smpl_len: u32 = if looped { 9 * 4 + SMPL_LOOPS * 6 * 4 } else { 0 };
    let smpl_chunk_len = if looped { 8 + smpl_len } else { 0 };

    let riff_len = 4 // "WAVE"
        + 8 + FMT_LEN
        + 8 + data_padded_len as u32
        + smpl_chunk_len;

    let mut out = Vec::with_capacity(8 + riff_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&riff_len.to_le_bytes());
    out.extend_from_slice(b"WAVE");

    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&FMT_LEN.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes()); // byte rate: rate * 1 channel * 1 byte/sample
    out.extend_from_slice(&1u16.to_le_bytes()); // block align: 1 channel * 1 byte/sample
    out.extend_from_slice(&8u16.to_le_bytes()); // bits per sample

    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&data);
    if !data.len().is_multiple_of(2) {
        out.push(0);
    }

    if looped {
        out.extend_from_slice(b"smpl");
        out.extend_from_slice(&smpl_len.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // manufacturer
        out.extend_from_slice(&0u32.to_le_bytes()); // product
        let sample_period_ns = (1_000_000_000f64 / sample_rate as f64).round() as u32;
        out.extend_from_slice(&sample_period_ns.to_le_bytes());
        out.extend_from_slice(&60u32.to_le_bytes()); // MIDI unity note, this crate's pitch anchor
        out.extend_from_slice(&0u32.to_le_bytes()); // MIDI pitch fraction
        out.extend_from_slice(&0u32.to_le_bytes()); // SMPTE format
        out.extend_from_slice(&0u32.to_le_bytes()); // SMPTE offset
        out.extend_from_slice(&SMPL_LOOPS.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // sampler data
        out.extend_from_slice(&0u32.to_le_bytes()); // cue point id
        out.extend_from_slice(&0u32.to_le_bytes()); // loop type: forward
        out.extend_from_slice(&0u32.to_le_bytes()); // loop start frame
        let last_frame = pcm.len().saturating_sub(1) as u32;
        out.extend_from_slice(&last_frame.to_le_bytes()); // loop end frame
        out.extend_from_slice(&0u32.to_le_bytes()); // fraction
        out.extend_from_slice(&0u32.to_le_bytes()); // play count: infinite
    }

    std::fs::write(path, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hand-parses the fields this module wrote, so the test doesn't depend
    /// on any WAV-reading crate -- a minimal, purpose-built round trip.
    struct ParsedWav {
        sample_rate: u32,
        bits_per_sample: u16,
        data: Vec<u8>,
        loop_start: Option<u32>,
        loop_end: Option<u32>,
    }

    fn parse_wav(bytes: &[u8]) -> ParsedWav {
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        let mut pos = 12;
        let mut sample_rate = 0;
        let mut bits_per_sample = 0;
        let mut data = Vec::new();
        let mut loop_start = None;
        let mut loop_end = None;
        while pos + 8 <= bytes.len() {
            let id = &bytes[pos..pos + 4];
            let len = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
            let body = &bytes[pos + 8..pos + 8 + len];
            match id {
                b"fmt " => {
                    sample_rate = u32::from_le_bytes(body[4..8].try_into().unwrap());
                    bits_per_sample = u16::from_le_bytes(body[14..16].try_into().unwrap());
                }
                b"data" => data = body.to_vec(),
                b"smpl" => {
                    // manufacturer, product, period, unity note, pitch
                    // fraction, smpte format, smpte offset, num loops,
                    // sampler data = 9 u32 fields before the loop record.
                    let loop_off = 9 * 4;
                    loop_start = Some(u32::from_le_bytes(
                        body[loop_off + 8..loop_off + 12].try_into().unwrap(),
                    ));
                    loop_end = Some(u32::from_le_bytes(
                        body[loop_off + 12..loop_off + 16].try_into().unwrap(),
                    ));
                }
                _ => {}
            }
            pos += 8 + len + (len % 2);
        }
        ParsedWav {
            sample_rate,
            bits_per_sample,
            data,
            loop_start,
            loop_end,
        }
    }

    #[test]
    fn looped_wav_round_trips_sample_data_and_loop_points() {
        let dir = std::env::temp_dir().join("tfmx-export-wav-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("looped.wav");
        let pcm: Vec<i8> = vec![-128, -1, 0, 1, 127];

        write_wav(&path, &pcm, 8363, true).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let parsed = parse_wav(&bytes);

        assert_eq!(parsed.sample_rate, 8363);
        assert_eq!(parsed.bits_per_sample, 8);
        assert_eq!(parsed.data, vec![0x00, 0x7F, 0x80, 0x81, 0xFF]);
        assert_eq!(parsed.loop_start, Some(0));
        assert_eq!(parsed.loop_end, Some(4), "loops the whole clip, last frame index");
    }

    #[test]
    fn one_shot_wav_has_no_smpl_chunk() {
        let dir = std::env::temp_dir().join("tfmx-export-wav-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("oneshot.wav");
        let pcm: Vec<i8> = vec![0, 1, 2];

        write_wav(&path, &pcm, 8363, false).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let parsed = parse_wav(&bytes);

        assert_eq!(parsed.loop_start, None);
        assert_eq!(parsed.loop_end, None);
        assert_eq!(parsed.data, vec![0x80, 0x81, 0x82]);
    }
}
