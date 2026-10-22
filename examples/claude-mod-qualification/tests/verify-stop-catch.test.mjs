import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import { verify } from '../verify-stop-catch.mjs';

const original = process.argv[2];
assert(original, 'retained native capture directory required');
verify(original);
const mutations = [
  ['foreign caller', 'throw-events.json', text => text.replaceAll('"session_id":', '"foreign_session_id":')],
  ['no native timeout', 'timeout-events.json', text => text.replace('"kind":"timeout"', '"kind":"throw"')],
  ['missing fallback failure', 'catch-failure-debug.log', text => text.replaceAll('its .catch threw Error: AIKIT_STOP_CATCH_FALLBACK_FAILURE', 'removed')],
  ['no outgoing block reason', 'requests.jsonl', text => text.replaceAll('AIKIT_STOP_CATCH_BLOCK', 'removed')],
  ['no durable native feedback', 'throw-transcript.jsonl', text => text.replaceAll('AIKIT_STOP_CATCH_BLOCK', 'removed')],
  ['aborted native outcome', 'catch-failure-events.json', text => text.replace('"isAborted":false', '"isAborted":true')],
];
for (const [label, file, change] of mutations) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'aikit-stop-catch-'));
  try {
    fs.cpSync(original, dir, { recursive: true });
    const target = path.join(dir, file);
    const before = fs.readFileSync(target, 'utf8');
    const after = change(before);
    assert.notEqual(after, before, `${label} must change evidence`);
    fs.writeFileSync(target, after);
    assert.throws(() => verify(dir), undefined, label);
    console.log(`rejected: ${label}`);
  } finally { fs.rmSync(dir, { recursive: true, force: true }); }
}
console.log('capture + six negative evidence checks passed; not additional native runs');
