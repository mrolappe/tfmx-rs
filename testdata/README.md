# Test corpus

The modules used to develop and verify this player. **They are not in this repository** —
they are copyrighted works of their composers. `fetch.sh` records how to obtain them:

```sh
sh testdata/fetch.sh
```

Source: [Modland](https://modland.com/pub/modules/TFMX/), the community archive of Amiga
module files. The script is idempotent — already-downloaded files are skipped.

## Selection

Ten modules, deliberately spread across TFMX's lifetime so that both on-disk header layouts
are exercised. Composer is Chris Hülsbeck throughout, except the last entry (Jochen Hippel).

| Module | mdat | smpl | Layout | Why it's here |
|---|---:|---:|---|---|
| `turrican intro` | 19108 | 45828 | fixed | 1990, earliest era |
| `turrican outside` | 12252 | 24822 | fixed | ditto, shorter |
| `turrican 2 level 1-desert` | 13024 | 40242 | packed | the reference track for A/B listening |
| `turrican 2 level 3-flight` | 14328 | 45072 | packed | dense arrangement |
| `turrican 3 level 1` | 16732 | 41220 | packed | latest Hülsbeck era |
| `apidya (title)` | 7056 | 131072 | packed | very large sample bank |
| `apidya (level 1)` | 8148 | 63644 | packed | |
| `r-type` | 7432 | 116160 | fixed | |
| `x-out (title)` | 9116 | 89600 | fixed | |
| `turrican 2 title (st)` | 20340 | 81454 | fixed | Hippel, 7V candidate — deferred past M1 |

## Observed on this corpus

- All ten files start with the magic `"TFMX-SONG "` (trailing space included).
- **Layout detection is a zero check.** The three longs at `$1D0` are either all zero
  (fixed-address layout: 128 patterns and 128 macros at hard-coded addresses) or all three are
  plausible in-file offsets in ascending order (packed layout). Nothing ambiguous appears in
  between, so the parser does not need a fuzzy heuristic.
- The split is 5 packed / 5 fixed, so neither path can silently rot.
