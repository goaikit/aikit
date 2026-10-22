// Disposable native failure diagnostic, never a production hook.
import fs from 'node:fs';
let bytes = '';
for await (const chunk of process.stdin) {
  bytes += chunk;
  if (Buffer.byteLength(bytes) > 1024 * 1024) throw new Error('hook input ceiling');
}
const input = JSON.parse(bytes);
if (input.hook_event_name !== 'Stop' || typeof input.session_id !== 'string')
  throw new Error('native Stop identity required');
const file = '/home/node/evidence/timeout-command.jsonl';
fs.appendFileSync(file, JSON.stringify({ phase: 'started', at: Date.now(), input }) + '\n');
await new Promise(resolve => setTimeout(resolve, 1500));
const result = { decision: 'block', reason: 'AIKIT_STOP_FAILURE_LATE_BLOCK' };
fs.appendFileSync(file, JSON.stringify({ phase: 'late_block', at: Date.now(), input, result }) + '\n');
process.stdout.write(JSON.stringify(result));
