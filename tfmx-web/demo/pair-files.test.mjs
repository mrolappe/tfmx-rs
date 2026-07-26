// Plain assert-based check, no framework -- run with `node pair-files.test.mjs`.
import assert from 'node:assert/strict';
import { pairFiles } from './pair-files.mjs';

// matches a valid mdat/smpl pair by shared stem, order-independent
{
  const mdat = { name: 'mdat.turrican intro' };
  const smpl = { name: 'smpl.turrican intro' };
  assert.deepEqual(pairFiles([smpl, mdat]), { mdatFile: mdat, smplFile: smpl });
}

// rejects mismatched stems
{
  const mdat = { name: 'mdat.turrican intro' };
  const smpl = { name: 'smpl.apidya' };
  assert.throws(() => pairFiles([mdat, smpl]), /don't match/);
}

// rejects a single dropped file
{
  assert.throws(() => pairFiles([{ name: 'mdat.turrican intro' }]), /exactly one/);
}

// rejects two files of the same kind
{
  const a = { name: 'mdat.turrican intro' };
  const b = { name: 'mdat.apidya' };
  assert.throws(() => pairFiles([a, b]), /exactly one/);
}

console.log('ok');
