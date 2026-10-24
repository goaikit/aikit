// Synthetic handler wrapper. No production policy or owner data.
import fs from 'node:fs';
import { execFileSync } from 'node:child_process';
const [state, workspace, evidence, mode] = process.argv.slice(2);
if (!['baseline', 'invalid', 'timeout'].includes(mode)) throw new Error('invalid case');
let input = '';
for await (const chunk of process.stdin) {
  input += chunk;
  if (Buffer.byteLength(input) > 1024 * 1024) throw new Error('input ceiling');
}
const event = JSON.parse(input), file = evidence + '/calls.jsonl';
const prior = fs.existsSync(file) ? fs.readFileSync(file, 'utf8').trim().split('\n').filter(Boolean).map(JSON.parse) : [];
const stop = event.hook_event_name === 'Stop';
const ordinal = stop ? prior.filter(row => row.phase === 'input' && row.event.hook_event_name === 'Stop').length + 1 : 0;
const record = value => fs.appendFileSync(file, JSON.stringify({ at: Date.now(), mode, ordinal, ...value }) + '\n');
record({ phase: 'input', event });
if (stop && ordinal === 1 && mode === 'invalid') {
  record({ phase: 'invalid_response' });
  process.stdout.write('intentionally invalid JSON');
} else {
  if (stop && ordinal === 1 && mode === 'timeout') {
    record({ phase: 'delayed' });
    await new Promise(resolve => setTimeout(resolve, 1500));
    record({ phase: 'after_delay' });
  }
  const output = execFileSync('/home/node/bin/integration_hooks',
    ['hook', state, workspace, mode === 'baseline' ? '1' : '0'], { input, encoding: 'utf8', maxBuffer: 1024 * 1024 });
  record({ phase: 'sdk_response', output });
  process.stdout.write(output);
}
