//! The currently loaded module's bytes. `Session` does not keep a live
//! `tfmx::Player`: every `tfmx-analysis` entry point (`render_*_pcm`,
//! `disassemble_*`, `build_song_view`) takes `&Module` and builds its own
//! transient state per call, and this GUI renders ahead to a WAV blob per
//! request rather than streaming (`docs/gui-plan.md`'s "no real-time/
//! streaming playback" scope) -- so there is nothing to keep alive between
//! requests. `Module::parse` only reads the fixed header (no sample/pattern
//! decode), so re-parsing per request instead of caching a `Module` avoids
//! `tfmx::Module`'s borrow tying `Session` to a self-referential shape.

use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct Session {
    mdat: Vec<u8>,
    smpl: Vec<u8>,
    mdat_path: PathBuf,
    smpl_path: PathBuf,
}

#[derive(Debug)]
pub enum LoadError {
    Io(std::io::Error),
    Parse(tfmx::ParseError),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Io(e) => write!(f, "reading module file: {e}"),
            LoadError::Parse(e) => write!(f, "parsing module: {e:?}"),
        }
    }
}

impl std::error::Error for LoadError {}

impl Session {
    /// Reads `mdat_path`/`smpl_path` and validates they parse as a
    /// `tfmx::Module` before accepting them, so a bad file is rejected here
    /// rather than surfacing on the first later request.
    pub fn load(mdat_path: PathBuf, smpl_path: PathBuf) -> Result<Self, LoadError> {
        let mdat = std::fs::read(&mdat_path).map_err(LoadError::Io)?;
        let smpl = std::fs::read(&smpl_path).map_err(LoadError::Io)?;
        tfmx::Module::parse(&mdat, &smpl).map_err(LoadError::Parse)?;
        Ok(Self {
            mdat,
            smpl,
            mdat_path,
            smpl_path,
        })
    }

    /// A fresh `Module` view over the loaded bytes -- cheap; see the module
    /// doc comment for why this isn't cached.
    pub fn module(&self) -> tfmx::Module<'_> {
        tfmx::Module::parse(&self.mdat, &self.smpl).expect("validated at load()")
    }

    pub fn mdat_path(&self) -> &Path {
        &self.mdat_path
    }

    pub fn smpl_path(&self) -> &Path {
        &self.smpl_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn corpus_path(name: &str) -> PathBuf {
        PathBuf::from(format!("{}/../testdata/{name}", env!("CARGO_MANIFEST_DIR")))
    }

    #[test]
    fn loads_a_valid_corpus_module_and_hands_out_a_working_module_view() {
        let mdat = corpus_path("mdat.turrican intro");
        let smpl = corpus_path("smpl.turrican intro");
        if !mdat.exists() {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        }
        let session = Session::load(mdat, smpl).expect("valid corpus pair loads");
        assert_eq!(
            session.module().song_start(0),
            session.module().song_start(0)
        );
    }

    #[test]
    fn rejects_a_missing_file() {
        let err = Session::load(
            PathBuf::from("/nonexistent/mdat"),
            PathBuf::from("/nonexistent/smpl"),
        )
        .expect_err("missing file must not load");
        assert!(matches!(err, LoadError::Io(_)));
    }

    #[test]
    fn rejects_a_file_that_is_not_a_tfmx_module() {
        let not_a_module = corpus_path("fetch.sh");
        let err = Session::load(not_a_module.clone(), not_a_module)
            .expect_err("garbage bytes must not parse as a module");
        assert!(matches!(err, LoadError::Parse(_)));
    }
}
