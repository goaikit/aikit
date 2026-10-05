// Usage: node scripts/gateway-load.mjs hosts.json [sessions-per-host=10]
// Executes real prompts on configured backends. hosts.json entries:
// { base, token, cwd, backend, permission_policy? }. Keep this credential file private.
import { readFile } from 'node:fs/promises';
import { randomUUID } from 'node:crypto';
import { performance } from 'node:perf_hooks';
import { AgentHost } from '../examples/gateway-client.mjs';

const hosts = JSON.parse(await readFile(process.argv[2], 'utf8'));
const perHost = Number(process.argv[3] ?? 10);
if (!Number.isInteger(perHost) || perHost < 1 || perHost > 100) throw new Error('sessions-per-host must be 1..100');
const results = await Promise.all(hosts.flatMap((host, hostIndex) => Array.from({ length: perHost }, async () => {
  const client = new AgentHost(host.base, host.token);
  const start = performance.now(); let session; let acceptedMs;
  try {
    const receipt = await client.create({ command_id: randomUUID(), backend: host.backend, cwd: host.cwd, prompt: 'Reply with exactly ready. Do not use tools or read files.', permission_policy: host.permission_policy ?? 'deny' });
    session = receipt.session_id; acceptedMs = performance.now() - start;
    let completed = false;
    for await (const event of client.events(session, 0, AbortSignal.timeout(120_000))) {
      if (event.type === 'agent' && event.payload?.payload?.terminal) {
        if (event.payload.payload.terminal.outcome !== 'success') throw new Error('agent turn failed');
        completed = true; break;
      }
      if (event.type === 'state' && ['failed', 'closed', 'interrupted'].includes(event.payload)) throw new Error('session ended before completion');
    }
    if (!completed) throw new Error('stream ended before terminal event');
    return { host: hostIndex, accepted_ms: acceptedMs, completed_ms: performance.now() - start, ok: true };
  } catch (error) { return { host: hostIndex, accepted_ms: acceptedMs, ok: false, error: error.message }; }
  finally { if (session) await client.command(session, { command_id: randomUUID(), type: 'close' }).catch(() => {}); }
})));
const latencies = results.filter(r => r.ok).map(r => r.accepted_ms).sort((a, b) => a - b);
console.log(JSON.stringify({ hosts: hosts.length, sessions: results.length, successful: latencies.length, accepted_p50_ms: latencies[Math.floor(latencies.length * .5)] ?? null, accepted_p95_ms: latencies[Math.min(latencies.length - 1, Math.floor(latencies.length * .95))] ?? null, results }, null, 2));
if (latencies.length !== results.length) process.exitCode = 1;
