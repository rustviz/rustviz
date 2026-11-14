// Run: npx tsx scripts/linkCode.test.ts
import assert from 'node:assert/strict';
import { codeFromHash, decodeCode, encodeCode, hashForCode } from '../src/linkCode';

const samples = [
  'fn main() {}',
  'fn main() {\n    let s = "héllo \\"q\\" \\\\ ✓ 🦀";\n}\n',
  '// rustviz: hide\nlet a = 1;\r\n',
  '??>>~~', // bytes that hit '+' and '/' in standard base64
  'a', 'ab', 'abc',
];
for (const s of samples) {
  const e = encodeCode(s);
  assert.match(e, /^[A-Za-z0-9_-]*$/, 'alphabet / no padding');
  assert.equal(decodeCode(e), s);
  assert.equal(codeFromHash(hashForCode(s)), s);
  assert.equal(Buffer.from(s, 'utf8').toString('base64url'), e, 'matches node base64url');
}
assert.equal(encodeCode('héllo'), 'aMOpbGxv');
for (const bad of ['#code=!!!', '#code=a', '#code=_w', '#other=abc', '#code=', '', '#code=gA']) {
  assert.equal(codeFromHash(bad), null, bad);
}
console.log('linkCode ok');
