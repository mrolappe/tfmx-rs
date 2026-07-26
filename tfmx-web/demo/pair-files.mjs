// Pairs dropped files by the mdat.*/smpl.* filename convention (docs/format.md
// §header: modules ship as an mdat.<name> + smpl.<name> pair with a shared
// stem). Throws a descriptive Error if the drop doesn't resolve unambiguously.
export function pairFiles(files) {
  const mdat = files.filter((f) => f.name.startsWith('mdat.'));
  const smpl = files.filter((f) => f.name.startsWith('smpl.'));

  if (mdat.length !== 1 || smpl.length !== 1) {
    const names = files.map((f) => f.name).join(', ') || '(none)';
    throw new Error(`need exactly one mdat.* and one smpl.* file, got: ${names}`);
  }

  const mdatStem = mdat[0].name.slice('mdat.'.length);
  const smplStem = smpl[0].name.slice('smpl.'.length);
  if (mdatStem !== smplStem) {
    throw new Error(`mdat/smpl filenames don't match: ${mdat[0].name} vs ${smpl[0].name}`);
  }

  return { mdatFile: mdat[0], smplFile: smpl[0] };
}
