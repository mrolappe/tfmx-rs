//! `Module` — parses and borrows the `mdat`/`smpl` header data.
//!
//! See `docs/format.md` §2 for the byte layout this implements.

const MAGIC: &[u8; 10] = b"TFMX-SONG ";
const HEADER_LEN: usize = 0x1DC;
const TEXT_OFFSET: usize = 0x010;
const TEXT_LEN: usize = 240;
const SONG_START_OFFSET: usize = 0x100;
const SONG_END_OFFSET: usize = 0x140;
const TEMPO_OFFSET: usize = 0x180;
const TABLE_ENTRIES: usize = 32;

const LAYOUT_TABLE_OFFSET: usize = 0x1D0;
const FIXED_PATTERN_PTR_OFFSET: u32 = 0x400;
const FIXED_MACRO_PTR_OFFSET: u32 = 0x600;
const FIXED_TRACKSTEP_OFFSET: u32 = 0x800;

/// Which of the two on-disk header layouts a module uses. See
/// `docs/format.md` §3: detection is a plain zero check on the three longs
/// at `$1D0`, not a heuristic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// The three longs at `$1D0` are all zero; tables sit at fixed offsets
    /// (`docs/format.md` §3.2).
    Fixed,
    /// The three longs at `$1D0` are explicit in-file offsets
    /// (`docs/format.md` §3.1).
    Packed,
}

/// A parsed `mdat`/`smpl` pair. Borrows both buffers; never copies.
#[derive(Debug)]
pub struct Module<'a> {
    text: &'a [u8],
    song_start: [u16; TABLE_ENTRIES],
    song_end: [u16; TABLE_ENTRIES],
    tempo: [u16; TABLE_ENTRIES],
    layout: Layout,
    trackstep_offset: u32,
    pattern_ptr_offset: u32,
    macro_ptr_offset: u32,
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
    /// Parses the fixed `mdat` header: magic, free-text area, the 96-word
    /// song-start/song-end/tempo table, and the `$1D0` layout table. Does
    /// not touch `smpl` (step 2.3's `sample()`).
    pub fn parse(mdat: &'a [u8], _smpl: &'a [u8]) -> Result<Module<'a>, ParseError> {
        if mdat.len() < HEADER_LEN {
            return Err(ParseError::TooShort);
        }
        if &mdat[0..10] != MAGIC {
            return Err(ParseError::BadMagic);
        }

        let raw_trackstep = read_long(mdat, LAYOUT_TABLE_OFFSET);
        let raw_pattern_ptr = read_long(mdat, LAYOUT_TABLE_OFFSET + 4);
        let raw_macro_ptr = read_long(mdat, LAYOUT_TABLE_OFFSET + 8);

        let (layout, trackstep_offset, pattern_ptr_offset, macro_ptr_offset) =
            if raw_trackstep == 0 && raw_pattern_ptr == 0 && raw_macro_ptr == 0 {
                (
                    Layout::Fixed,
                    FIXED_TRACKSTEP_OFFSET,
                    FIXED_PATTERN_PTR_OFFSET,
                    FIXED_MACRO_PTR_OFFSET,
                )
            } else {
                (Layout::Packed, raw_trackstep, raw_pattern_ptr, raw_macro_ptr)
            };

        Ok(Module {
            text: &mdat[TEXT_OFFSET..TEXT_OFFSET + TEXT_LEN],
            song_start: read_word_table(mdat, SONG_START_OFFSET),
            song_end: read_word_table(mdat, SONG_END_OFFSET),
            tempo: read_word_table(mdat, TEMPO_OFFSET),
            layout,
            trackstep_offset,
            pattern_ptr_offset,
            macro_ptr_offset,
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

    /// Which on-disk header layout this module uses.
    pub fn layout(&self) -> Layout {
        self.layout
    }

    /// Absolute `mdat` byte offset of the trackstep table.
    pub fn trackstep_offset(&self) -> u32 {
        self.trackstep_offset
    }

    /// Absolute `mdat` byte offset of the pattern-pointer table.
    pub fn pattern_ptr_offset(&self) -> u32 {
        self.pattern_ptr_offset
    }

    /// Absolute `mdat` byte offset of the macro-pointer table.
    pub fn macro_ptr_offset(&self) -> u32 {
        self.macro_ptr_offset
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

fn read_long(mdat: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes([
        mdat[offset],
        mdat[offset + 1],
        mdat[offset + 2],
        mdat[offset + 3],
    ])
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
        let mut mdat = vec![0u8; HEADER_LEN];
        mdat[0..10].copy_from_slice(b"NOT-A-TFMX");
        assert_eq!(
            Module::parse(&mdat, &[]).unwrap_err(),
            ParseError::BadMagic
        );
    }

    #[test]
    fn rejects_truncated_header() {
        let mut mdat = vec![0u8; HEADER_LEN - 1];
        mdat[0..10].copy_from_slice(b"TFMX-SONG ");
        assert_eq!(
            Module::parse(&mdat, &[]).unwrap_err(),
            ParseError::TooShort
        );
    }

    #[test]
    fn detects_fixed_layout() {
        let Some(mdat) = read_corpus("mdat.turrican intro") else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let smpl = read_corpus("smpl.turrican intro").expect("smpl present alongside mdat");
        let module = Module::parse(&mdat, &smpl).expect("valid header parses");

        assert_eq!(module.layout(), Layout::Fixed);
        assert_eq!(module.trackstep_offset(), 0x800);
        assert_eq!(module.pattern_ptr_offset(), 0x400);
        assert_eq!(module.macro_ptr_offset(), 0x600);
    }

    #[test]
    fn detects_packed_layout() {
        let Some(mdat) = read_corpus("mdat.turrican 2 level 1-desert") else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let smpl = read_corpus("smpl.turrican 2 level 1-desert")
            .expect("smpl present alongside mdat");
        let module = Module::parse(&mdat, &smpl).expect("valid header parses");

        assert_eq!(module.layout(), Layout::Packed);
        assert_eq!(module.trackstep_offset(), 0x3E8);
        assert_eq!(module.pattern_ptr_offset(), 0x3078);
        assert_eq!(module.macro_ptr_offset(), 0x31DC);
    }

    #[test]
    fn all_corpus_files_parse_with_documented_layout() {
        // Layout column of testdata/README.md.
        let files: [(&str, Layout); 10] = [
            ("turrican intro", Layout::Fixed),
            ("turrican outside", Layout::Fixed),
            ("r-type", Layout::Fixed),
            ("x-out (title)", Layout::Fixed),
            ("turrican 2 title (st)", Layout::Fixed),
            ("turrican 2 level 1-desert", Layout::Packed),
            ("turrican 2 level 3-flight", Layout::Packed),
            ("turrican 3 level 1", Layout::Packed),
            ("apidya (title)", Layout::Packed),
            ("apidya (level 1)", Layout::Packed),
        ];

        for (name, expected) in files {
            let Some(mdat) = read_corpus(&format!("mdat.{name}")) else {
                eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
                return;
            };
            let smpl =
                read_corpus(&format!("smpl.{name}")).expect("smpl present alongside mdat");
            let module = Module::parse(&mdat, &smpl).unwrap_or_else(|e| panic!("{name}: {e:?}"));
            assert_eq!(module.layout(), expected, "{name}");
        }
    }
}
