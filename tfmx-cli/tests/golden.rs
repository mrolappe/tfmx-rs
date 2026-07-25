//! Golden-hash regression test (step 6.1): renders the first 10 s of song 0
//! for every corpus module and checks its SHA-256 against `tests/golden.txt`.
//!
//! Regenerate the golden file after an intentional output change:
//!
//!     TFMX_REGEN_GOLDEN=1 cargo test -p tfmx-cli --test golden

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::PathBuf;

const SAMPLE_RATE: u32 = 44_100;
const SEPARATION: u8 = 100;
const SECONDS: u32 = 10;

const CORPUS: &[&str] = &[
    "turrican intro",
    "turrican outside",
    "r-type",
    "x-out (title)",
    "turrican 2 title (st)",
    "turrican 2 level 1-desert",
    "turrican 2 level 3-flight",
    "turrican 3 level 1",
    "apidya (title)",
    "apidya (level 1)",
];

fn golden_path() -> PathBuf {
    PathBuf::from(format!("{}/tests/golden.txt", env!("CARGO_MANIFEST_DIR")))
}

fn corpus_path(name: &str, prefix: &str) -> Option<PathBuf> {
    let path = PathBuf::from(format!(
        "{}/../testdata/{prefix}.{name}",
        env!("CARGO_MANIFEST_DIR")
    ));
    path.exists().then_some(path)
}

fn render_hash(name: &str) -> Option<String> {
    let mdat = std::fs::read(corpus_path(name, "mdat")?).unwrap();
    let smpl = std::fs::read(corpus_path(name, "smpl")?).unwrap();
    let module = tfmx::Module::parse(&mdat, &smpl).unwrap_or_else(|e| panic!("{name}: {e:?}"));
    let mut player = tfmx::Player::new(&module, 0, SAMPLE_RATE, SEPARATION)
        .unwrap_or_else(|e| panic!("{name}: {e:?}"));

    let mut hasher = Sha256::new();
    let mut frames_left = SAMPLE_RATE as usize * SECONDS as usize;
    let mut buf = vec![0i16; 4096 * 2];
    while frames_left > 0 {
        let chunk_frames = frames_left.min(4096);
        let out = &mut buf[..chunk_frames * 2];
        player
            .render(out)
            .unwrap_or_else(|e| panic!("{name}: {e:?}"));
        for sample in out.iter() {
            hasher.update(sample.to_le_bytes());
        }
        frames_left -= chunk_frames;
    }
    Some(format!("{:x}", hasher.finalize()))
}

fn parse_golden(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let (name, hash) = line
                .split_once(": ")
                .unwrap_or_else(|| panic!("malformed golden.txt line: {line:?}"));
            (name.to_string(), hash.to_string())
        })
        .collect()
}

#[test]
fn rendered_output_matches_golden_hashes() {
    let mut hashes = BTreeMap::new();
    for &name in CORPUS {
        match render_hash(name) {
            Some(hash) => {
                hashes.insert(name.to_string(), hash);
            }
            None => {
                eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
                return;
            }
        }
    }

    if std::env::var_os("TFMX_REGEN_GOLDEN").is_some() {
        let text: String = hashes
            .iter()
            .map(|(name, hash)| format!("{name}: {hash}\n"))
            .collect();
        std::fs::write(golden_path(), text).expect("write tests/golden.txt");
        return;
    }

    let golden_text = std::fs::read_to_string(golden_path()).unwrap_or_else(|_| {
        panic!(
            "missing {}; regenerate with TFMX_REGEN_GOLDEN=1 cargo test -p tfmx-cli --test golden",
            golden_path().display()
        )
    });
    let golden = parse_golden(&golden_text);

    for (name, hash) in &hashes {
        match golden.get(name) {
            Some(expected) => assert_eq!(
                hash, expected,
                "{name}: rendered output no longer matches tests/golden.txt \
                 (regenerate with TFMX_REGEN_GOLDEN=1 if this change is intentional)"
            ),
            None => panic!(
                "{name}: no golden hash on record; regenerate with TFMX_REGEN_GOLDEN=1 cargo test -p tfmx-cli --test golden"
            ),
        }
    }
}
