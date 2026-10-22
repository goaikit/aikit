import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import { verify } from '../verify-stop-chain.mjs';
const source = process.argv[2];
assert(source, 'supply retained native evidence directory');
assert.equal(verify(source).command_hook_bypassed, true);
const mutations = [
  ['short-circuit-events.json', value => value.filter(r => r.kind !== 'short_circuit_stop')],
  ['short-circuit-events.json', value => value.map(r => r.kind === 'turn_complete'
    ? { ...r, value: { ...r.value, reason: 'error' } } : r)],
  ['short-circuit-events.json', value => value.map(r => r.kind === 'session_start'
    ? { ...r, value: { ...r.value, sessionId: 'foreign' } } : r)],
  ['command-stops.jsonl', value => [...value, { ...value[0], input: {
    ...value[0].input, session_id: 'extra-command-call',
  } }]],
  ['requests.jsonl', value => value.map(r => r.body ? { ...r, body: {
    ...r.body, messages: JSON.parse(JSON.stringify(r.body.messages)
      .replaceAll('AIKIT_STOP_CHAIN_FIRST_BLOCK', 'removed-reason')),
  } } : r)],
];
for (const [name, mutate] of mutations) {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'aikit-stop-verifier-'));
  try {
    for (const file of ['command-stops.jsonl', 'baseline-transcript.jsonl',
      'competitor-transcript.jsonl', 'short-circuit-events.json', 'requests.jsonl', 'settings.json'])
      fs.copyFileSync(path.join(source, file), path.join(temp, file));
    const location = path.join(temp, name);
    const jsonl = name.endsWith('.jsonl');
    const bytes = fs.readFileSync(location, 'utf8');
    const value = jsonl ? bytes.trim().split('\n').map(JSON.parse) : JSON.parse(bytes);
    const changed = mutate(value);
    fs.writeFileSync(location, jsonl ? changed.map(r => JSON.stringify(r)).join('\n') + '\n'
      : JSON.stringify(changed));
    assert.throws(() => verify(temp), undefined, `mutation was accepted: ${name}`);
  } finally { fs.rmSync(temp, { recursive: true, force: true }); }
}
console.log('6 stop-chain verifier checks passed (capture plus 5 mutations; not 6 native runs)');
