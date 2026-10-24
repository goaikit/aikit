// Disposable command hook. First Stop per session blocks, second permits.
// This isolates whether competing mod middleware can skip a real command hook.
import fs from 'node:fs';
let bytes = '';
for await (const chunk of process.stdin) {
  bytes += chunk;
  if (Buffer.byteLength(bytes) > 1024 * 1024) throw new Error('hook input ceiling');
}
const input = JSON.parse(bytes);
if (typeof input.session_id !== 'string' || input.hook_event_name !== 'Stop')
  throw new Error('expected native Stop identity');
const file = '/home/node/evidence/command-stops.jsonl';
const previous = fs.existsSync(file)
  ? fs.readFileSync(file, 'utf8').trim().split('\n').filter(Boolean).map(JSON.parse) : [];
const ordinal = previous.filter(row => row.input.session_id === input.session_id).length + 1;
const result = ordinal === 1 ? { decision: 'block', reason: 'AIKIT_STOP_CHAIN_FIRST_BLOCK' } : {};
fs.appendFileSync(file, JSON.stringify({ input, ordinal, result }) + '\n');
process.stdout.write(JSON.stringify(result));
