import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import { verify } from '../verify-stop-failure.mjs';

const original = process.argv[2];
assert(original, 'retained native capture directory required');
verify(original);
const mutations = [
  ['missing guard', 'timeout-debug.log', text => text.replaceAll('cc-plugin-sec-default@builtin seated outermost: this machine has managed settings', 'missing')],
  ['no failure policy', 'missing-settings.json', text => text.replace('"onFailure":"block"', '"onFailure":"continue"')],
  ['foreign timed-out invocation', 'timeout-command.jsonl', text => text.replace(JSON.parse(text.split('\n')[0]).input.session_id, 'foreign')],
  ['false accepted outcome', 'timeout-events.json', text => text.replace('"reason":"answer"', '"reason":"error"')],
  ['no effective timeout', 'timeout-debug.log', text => text.replaceAll('timed out after 1000ms', 'not timed out')],
  ['missing baseline propagation', 'requests.jsonl', text => text.replaceAll('AIKIT_STOP_CHAIN_FIRST_BLOCK', 'removed')],
];
for (const [label, file, change] of mutations) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'aikit-stop-failure-'));
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
console.log('capture + six mutation checks passed; not additional native runs');
