import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import { verify } from './verify-native-child.mjs';
const original = process.argv[2];
verify(original, true);
const cases = [
  ['native-child-rust.json', v => { v.accepted.facts.pid++; }],
  ['native-child-rust.json', v => { v.accepted.facts.start_ticks++; }],
  ['native-child-rust.json', v => { v.accepted.facts.executable_sha256 = '0'.repeat(64); }],
  ['native-child-rust.json', v => { v.wrong_hash.exit_code = 0; }],
  ['independent-rust.json', v => { v.accepted = true; v.exit_code = 0; }],
  ['after-exit-rust.json', v => { v.accepted = true; v.exit_code = 0; }],
  ['native-child-rust.json', v => { v.python.peer_is_ancestor = false; }],
  ['native-child-rust.json', v => { v.python.token_read = true; }],
];
for (const [file, change] of cases) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'aikit-native-child-'));
  fs.cpSync(original, root, { recursive: true, filter: value => path.basename(value) !== 'latest' });
  const target = path.join(root, file), value = JSON.parse(fs.readFileSync(target));
  change(value); fs.writeFileSync(target, JSON.stringify(value));
  assert.throws(() => verify(root, true), assert.AssertionError);
}
console.log(JSON.stringify({ passed: true, altered_evidence_checks: cases.length, native_trials: 0 }));
