import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import { verify } from '../verify-generated-stop.mjs';
const original = process.argv[2];
assert(original, 'retained native capture directory required');
verify(original);
for (const [label, file, change] of [
  ['missing native fallback reason', 'requests.jsonl', t => t.replaceAll('Completion validation unavailable: restore the local handler and retry.', 'removed')],
  ['changed retained command arguments', 'baseline/project-settings.json', t => {
    const v = JSON.parse(t); v.hooks.Stop[0].hooks[0].args = ['foreign']; return JSON.stringify(v);
  }],
  ['late timed-out callback', 'timeout/calls.jsonl', t => t + '\n' + JSON.stringify({ phase: 'after_delay', ordinal: 1 }) + '\n'],
  ['foreign native completion', 'invalid/events.json', t => {
    const v = JSON.parse(t); v.records.find(r => r.request.event === 'native_turn_finished').request.session_id = 'foreign'; return JSON.stringify(v);
  }],
]) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'aikit-generated-stop-'));
  try {
    fs.cpSync(original, dir, { recursive: true });
    const target = path.join(dir, file), before = fs.readFileSync(target, 'utf8'), after = change(before);
    assert.notEqual(after, before);
    fs.writeFileSync(target, after);
    assert.throws(() => verify(dir), undefined, label);
    console.log(`rejected: ${label}`);
  } finally { fs.rmSync(dir, { recursive: true, force: true }); }
}
console.log('capture + four negative evidence checks passed; not additional native runs');
