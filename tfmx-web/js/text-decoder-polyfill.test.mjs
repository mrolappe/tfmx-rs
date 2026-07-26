// Plain assert-based check, no framework -- run with `node text-decoder-polyfill.test.mjs`.
import assert from 'node:assert/strict';
import { MinimalTextDecoder } from './text-decoder-polyfill.mjs';

const decoder = new MinimalTextDecoder();

assert.equal(decoder.decode(), ''); // V8 JIT warm-up call, no argument
assert.equal(decoder.decode(new TextEncoder().encode('')), '');
assert.equal(decoder.decode(new TextEncoder().encode('invalid module: parse error')), 'invalid module: parse error');
assert.equal(decoder.decode(new TextEncoder().encode('café')), 'café'); // 2-byte sequence
assert.equal(decoder.decode(new TextEncoder().encode('☃')), '☃'); // 3-byte sequence (snowman)
assert.equal(decoder.decode(new TextEncoder().encode('\u{1f600}')), '\u{1f600}'); // 4-byte sequence, surrogate pair

console.log('ok');
