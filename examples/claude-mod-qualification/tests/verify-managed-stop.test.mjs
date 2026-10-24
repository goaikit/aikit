import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import { verify } from '../verify-managed-stop.mjs';

const original = process.argv[2];
assert(original, 'retained capture directory required');
verify(original);
const mutations = [
  ['missing guard', 'managed-debug.log', text => text.replaceAll('cc-plugin-sec-default@builtin seated outermost: this machine has managed settings', 'missing')],
  ['foreign Stop', 'command-stops.jsonl', text => text.replaceAll(JSON.parse(text.split('\n')[0]).input.session_id, 'foreign')],
  ['missing propagation', 'requests.jsonl', text => text.replaceAll('AIKIT_STOP_CHAIN_FIRST_BLOCK', 'removed')],
  ['false completion', 'managed-events.json', text => text.replace('"reason":"answer"', '"reason":"error"')],
  ['missing competitor', 'user-events.json', text => JSON.stringify(JSON.parse(text).filter(e => e.kind !== 'short_circuit_stop'))],
  ['different managed command', 'managed-settings.json', text => text.replace('stop-chain-command.mjs', 'other.mjs')],
  ['network exposed', 'networks.json', () => '{"bridge":{}}'],
];
for (const [label, file, change] of mutations) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'aikit-managed-stop-'));
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
console.log('capture + seven evidence mutation checks passed; not additional native runs');
