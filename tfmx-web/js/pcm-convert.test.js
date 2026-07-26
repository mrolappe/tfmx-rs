// Plain assert-based check, no framework -- run with `node pcm-convert.test.js`.
'use strict';

const assert = require('node:assert/strict');
const { interleavedI16ToPlanarF32 } = require('./pcm-convert.js');

// 3 stereo frames: L/R pairs picked to hit zero and both extremes.
const interleaved = new Int16Array([0, 0, 32767, -32768, 100, -100]);
const left = new Float32Array(3);
const right = new Float32Array(3);

interleavedI16ToPlanarF32(interleaved, [left, right]);

assert.deepEqual(Array.from(left), [0, 32767 / 32768, 100 / 32768]);
assert.deepEqual(Array.from(right), [0, -1, -100 / 32768]);

console.log('ok');
