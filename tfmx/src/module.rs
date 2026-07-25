//! `Module` — parses and borrows the `mdat`/`smpl` header data.
//!
//! See `docs/format.md` §2 for the byte layout this implements.

const MAGIC: &[u8; 10] = b"TFMX-SONG ";
const HEADER_LEN: usize = 0x1C0;
const TEXT_OFFSET: usize = 0x010;
const TEXT_LEN: usize = 240;
const SONG_START_OFFSET: usize = 0x100;
const SONG_END_OFFSET: usize = 0x140;
const TEMPO_OFFSET: usize = 0x180;
const TABLE_ENTRIES: usize = 32;

/// A parsed `mdat`/`smpl` pair. Borrows both buffers; never copies.
#[derive(Debug)]
pub struct Module<'a> {
    text: &'a [u8],
    song_start: [u16; TABLE_ENTRIES],
    song_end: [u16; TABLE_ENTRIES],
    tempo: [u16; TABLE_ENTRIES],
}

/// Why [`Module::parse`] failed.
#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    /// `mdat` is shorter than the fixed header ($0–$1C0).
    TooShort,
    /// `mdat` does not start with `"TFMX-SONG "`.
    BadMagic,
}

impl<'a> Module<'a> {
    /// Parses the fixed `mdat` header: magic, free-text area, and the 96-word
    /// song-start/song-end/tempo table. Does not touch the `$1D0` layout
    /// table (step 2.2) or `smpl` (step 2.3's `sample()`).
    pub fn parse(mdat: &'a [u8], _smpl: &'a [u8]) -> Result<Module<'a>, ParseError> {
        if mdat.len() < HEADER_LEN {
            return Err(ParseError::TooShort);
        }
        if &mdat[0..10] != MAGIC {
            return Err(ParseError::BadMagic);
        }
        Ok(Module {
            text: &mdat[TEXT_OFFSET..TEXT_OFFSET + TEXT_LEN],
            song_start: read_word_table(mdat, SONG_START_OFFSET),
            song_end: read_word_table(mdat, SONG_END_OFFSET),
            tempo: read_word_table(mdat, TEMPO_OFFSET),
        })
    }

    /// The 40x6 free-text area: raw space-padded ASCII bytes, as stored.
    pub fn text(&self) -> &'a [u8] {
        self.text
    }

    /// Trackstep line index where song `n` (0–31) begins.
    pub fn song_start(&self, n: u8) -> u16 {
        self.song_start[n as usize]
    }

    /// Trackstep line index where song `n` (0–31) ends.
    pub fn song_end(&self, n: u8) -> u16 {
        self.song_end[n as usize]
    }

    /// Tempo value for slot `n` (0–31). See `docs/playback-model.md` §3.2
    /// for how to interpret it (50 Hz-divider path vs. BPM path).
    pub fn tempo(&self, n: u8) -> u16 {
        self.tempo[n as usize]
    }
}

fn read_word_table(mdat: &[u8], offset: usize) -> [u16; TABLE_ENTRIES] {
    let mut table = [0u16; TABLE_ENTRIES];
    for (i, slot) in table.iter_mut().enumerate() {
        let o = offset + i * 2;
        *slot = u16::from_be_bytes([mdat[o], mdat[o + 1]]);
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn read_corpus(name: &str) -> Option<Vec<u8>> {
        let path = format!("{}/../testdata/{}", env!("CARGO_MANIFEST_DIR"), name);
        fs::read(path).ok()
    }

    #[test]
    fn parses_known_file_header() {
        let Some(mdat) = read_corpus("mdat.turrican intro") else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let smpl = read_corpus("smpl.turrican intro").expect("smpl present alongside mdat");

        let module = Module::parse(&mdat, &smpl).expect("valid header parses");

        assert!(module.text().starts_with(b"(Empty)"));
        assert_eq!(module.text().len(), 240);
        assert_eq!(module.song_start(0), 75);
        assert_eq!(module.song_end(0), 129);
        assert_eq!(module.tempo(0), 3);
        assert_eq!(module.tempo(1), 120);
        assert_eq!(module.tempo(2), 160);
    }

    #[test]
    fn rejects_bad_magic() {
        let mut mdat = vec![0u8; 0x1C0];
        mdat[0..10].copy_from_slice(b"NOT-A-TFMX");
        assert_eq!(
            Module::parse(&mdat, &[]).unwrap_err(),
            ParseError::BadMagic
        );
    }

    #[test]
    fn rejects_truncated_header() {
        let mut mdat = vec![0u8; 0x1C0 - 1];
        mdat[0..10].copy_from_slice(b"TFMX-SONG ");
        assert_eq!(
            Module::parse(&mdat, &[]).unwrap_err(),
            ParseError::TooShort
        );
    }
}
