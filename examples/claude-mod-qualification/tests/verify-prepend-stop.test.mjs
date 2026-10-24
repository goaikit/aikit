import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import { verify } from '../verify-prepend-stop.mjs';

const original = process.argv[2];
assert(original, 'retained native capture directory required');
verify(original);
for (const [label, file, change] of [
  ['wrong effective tier', 'user-debug.log', t => t.replaceAll('environment 1, tier prepend', 'environment 1, tier user')],
  ['wrong guard position', 'managed-debug.log', t => t.replaceAll('seated by managed prependPlugins at position 2', 'seated elsewhere')],
  ['missing outgoing Block', 'requests.jsonl', t => t.replaceAll('AIKIT_STOP_CATCH_BLOCK', 'removed')],
]) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'aikit-prepend-stop-'));
  try {
    fs.cpSync(original, dir, { recursive: true });
    const target = path.join(dir, file);
    const before = fs.readFileSync(target, 'utf8');
    const after = change(before);
    assert.notEqual(after, before);
    fs.writeFileSync(target, after);
    assert.throws(() => verify(dir), undefined, label);
    console.log(`rejected: ${label}`);
  } finally { fs.rmSync(dir, { recursive: true, force: true }); }
}
console.log('capture + three negative evidence checks passed; not additional native runs');
