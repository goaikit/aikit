// Explicit offline fault injector around the REAL SDK receiver. After its
// stored-row transaction commits, hold its stdout so the controller can SIGKILL
// the owned container before the native module receives the reply. No resend.
import fs from 'node:fs';
import {execFileSync} from 'node:child_process';
if (process.argv.length !== 4) throw new Error('fixed state and installation required');
const input = fs.readFileSync(0);
if (input.length > 131072) throw new Error('oversized diagnostic envelope');
const frame = JSON.parse(input.toString('utf8'));
const output = execFileSync('/home/node/bin/integration_message_bridge', process.argv.slice(2), {
  input, encoding: 'utf8', maxBuffer: 1048576,
});
if (frame.action.kind === 'record_append_stored') {
  const reply = JSON.parse(output);
  if (!reply.append_checkpoint?.stored || !reply.append_checkpoint.stored_at_unix_ms)
    throw new Error('SDK storage checkpoint did not commit');
  const file = '/home/node/evidence-crash/checkpoint-committed.json';
  const fd = fs.openSync(file, 'wx', 0o600);
  try { fs.writeFileSync(fd, JSON.stringify({frame, reply, stdout_withheld: true})); fs.fsyncSync(fd); }
  finally { fs.closeSync(fd); }
  const directory = fs.openSync('/home/node/evidence-crash', 'r');
  try { fs.fsyncSync(directory); } finally { fs.closeSync(directory); }
  // Controller must observe marker and kill within the native module's 30s RPC
  // bound. A timeout is a different scenario and cannot be relabelled SIGKILL.
  await new Promise(resolve => setTimeout(resolve, 60000));
}
process.stdout.write(output);
