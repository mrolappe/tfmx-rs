//! Mutation robustness: a seeded LCG flips bytes in real corpus `mdat`/`smpl`
//! buffers and asserts that `Module::parse` followed by ~1 s of
//! `Player::render` never panics. `Err` from either call is a fine outcome
//! for corrupted input -- a panic is not.

use tfmx::{Module, Player};

const MODULES: &[&str] = &[
    "turrican intro",
    "turrican outside",
    "turrican 2 level 1-desert",
    "turrican 2 level 3-flight",
    "turrican 3 level 1",
    "apidya (title)",
    "apidya (level 1)",
    "r-type",
    "x-out (title)",
    "turrican 2 title (st)",
];

const MUTATIONS_PER_MODULE: usize = 300;
const SAMPLE_RATE: u32 = 44100;

fn read_corpus(name: &str) -> Option<(Vec<u8>, Vec<u8>)> {
    let dir = format!("{}/../testdata", env!("CARGO_MANIFEST_DIR"));
    let mdat = std::fs::read(format!("{dir}/mdat.{name}")).ok()?;
    let smpl = std::fs::read(format!("{dir}/smpl.{name}")).ok()?;
    Some((mdat, smpl))
}

/// Minimal linear congruential generator (no dependency), same constants as
/// `Numerical Recipes`' 64-bit LCG. Deterministic: same seed, same mutations.
struct Lcg(u64);

impl Lcg {
    fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
}

#[test]
fn mutated_corpus_never_panics() {
    let mut found_any = false;

    for &name in MODULES {
        let Some((mdat, smpl)) = read_corpus(name) else {
            eprintln!("skipping {name}: run `sh testdata/fetch.sh` to fetch the test corpus");
            continue;
        };
        found_any = true;

        let mut lcg = Lcg(0x9e37_79b9_7f4a_7c15 ^ name.len() as u64);
        for i in 0..MUTATIONS_PER_MODULE {
            let mut mdat = mdat.clone();
            let mut smpl = smpl.clone();

            let target_smpl = lcg.next_u64() % 2 == 0 && !smpl.is_empty();
            let buf = if target_smpl { &mut smpl } else { &mut mdat };
            let idx = (lcg.next_u64() as usize) % buf.len();
            let flip = (lcg.next_u64() & 0xFF) as u8;
            buf[idx] ^= flip;

            let seed_before = lcg.0;
            let result = std::panic::catch_unwind(|| {
                let module = Module::parse(&mdat, &smpl).ok()?;
                let mut player = Player::new(&module, 0, SAMPLE_RATE, 100).ok()?;
                let mut out = vec![0i16; SAMPLE_RATE as usize * 2];
                let _ = player.render(&mut out);
                Some(())
            });

            assert!(
                result.is_ok(),
                "mutation {i} of \"{name}\" panicked (byte {idx} of {}, flip {flip:#04x}, lcg state {seed_before:#x})",
                if target_smpl { "smpl" } else { "mdat" },
            );
        }
    }

    if !found_any {
        eprintln!("skipping: no corpus files found; run `sh testdata/fetch.sh`");
    }
}
