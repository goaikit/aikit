import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
export function verify(directory) {
  const read = name => fs.readFileSync(path.join(directory, name), 'utf8');
  const rows = name => read(name).trim().split('\n').filter(Boolean).map(JSON.parse);
  const stops = rows('command-stops.jsonl');
  const baseline = rows('baseline-transcript.jsonl');
  const competitor = rows('competitor-transcript.jsonl');
  const events = JSON.parse(read('short-circuit-events.json'));
  const requests = rows('requests.jsonl');
  const main = marker => requests.filter(r => r.method === 'POST'
    && r.path.startsWith('/v1/messages') && Array.isArray(r.body?.tools)
    && r.body.tools.length > 0 && JSON.stringify(r.body.messages).includes(marker));
  const baselineStops = stops.filter(r => r.input?.session_id === stops[0].input.session_id);
  assert.equal(stops.length, 2, 'only baseline command calls expected');
  assert.deepEqual(baselineStops.map(r => [r.ordinal, r.result]), [
    [1, { decision: 'block', reason: 'AIKIT_STOP_CHAIN_FIRST_BLOCK' }], [2, {}],
  ]);
  const baselineId = baselineStops[0].input.session_id;
  assert(baseline.some(r => r.sessionId === baselineId && r.subtype === 'turn_duration'));
  assert(baseline.some(r => r.attachment?.type === 'hook_blocking_error'
    && r.attachment.blockingError?.blockingError === 'AIKIT_STOP_CHAIN_FIRST_BLOCK'));
  assert(baseline.some(r => r.attachment?.type === 'hook_success' && r.attachment.exitCode === 0));
  const baselineRequests = main('AIKIT_STOP_CHAIN_BASELINE');
  assert.equal(baselineRequests.length, 2);
  assert(!JSON.stringify(baselineRequests[0].body.messages).includes('AIKIT_STOP_CHAIN_FIRST_BLOCK'));
  assert(JSON.stringify(baselineRequests[1].body.messages).includes('AIKIT_STOP_CHAIN_FIRST_BLOCK'));
  const starts = events.filter(r => r.kind === 'session_start');
  assert.equal(starts.length, 1);
  const competitorId = starts[0].value.sessionId;
  assert.notEqual(competitorId, baselineId);
  assert.equal(starts[0].value.event.surface, 'terminal');
  assert.equal(starts[0].value.event.isInteractive, true);
  const skipped = events.filter(r => r.kind === 'short_circuit_stop');
  assert.equal(skipped.length, 1);
  assert.equal(skipped[0].value.session_id, competitorId);
  assert.equal(skipped[0].value.last_assistant_message, 'Fixture answer');
  assert.equal(stops.filter(r => r.input.session_id === competitorId).length, 0);
  const completed = events.filter(r => r.kind === 'turn_complete');
  assert.equal(completed.length, 1);
  assert.equal(completed[0].value.reason, 'answer');
  assert.equal(completed[0].value.isAborted, false);
  assert.equal(completed[0].value.answer, 'Fixture answer');
  assert(completed[0].value.usage.output_tokens > 0);
  assert(competitor.some(r => r.sessionId === competitorId && r.subtype === 'turn_duration'));
  assert(!competitor.some(r => r.attachment?.hookEvent === 'Stop'));
  const competitorRequests = main('AIKIT_STOP_CHAIN_COMPETITOR');
  assert.equal(competitorRequests.length, 1);
  assert(!JSON.stringify(competitorRequests[0].body.messages).includes('AIKIT_STOP_CHAIN_FIRST_BLOCK'));
  const settings = JSON.parse(read('settings.json'));
  assert.equal(settings.hooks.Stop[0].hooks[0].command, 'node /home/node/probe/stop-chain-command.mjs');
  return { baseline_session: baselineId, competitor_session: competitorId,
    baseline_main_requests: 2, competitor_main_requests: 1, command_hook_bypassed: true,
    scope: 'Claude 2.1.296 Linux, one command hook and one short-circuit mod; no readiness promotion' };
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url))
  console.log(JSON.stringify(verify(process.argv[2]), null, 2));
