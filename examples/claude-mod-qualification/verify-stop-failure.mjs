import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';

export function verify(directory) {
  const read = name => fs.readFileSync(path.join(directory, name), 'utf8').replace(/^\uFEFF/, '');
  const json = name => JSON.parse(read(name));
  const rows = name => read(name).trim().split('\n').filter(Boolean).map(JSON.parse);
  const requests = rows('requests.jsonl');
  const stops = rows('command-stops.jsonl');
  const timeout = rows('timeout-command.jsonl');
  const ids = [];
  const mainCounts = [];
  for (const mode of ['baseline', 'missing', 'timeout']) {
    const events = json(`${mode}-events.json`);
    const starts = events.filter(e => e.kind === 'session_start');
    assert.equal(starts.length, 1);
    const { sessionId, event } = starts[0].value;
    assert.equal(event.isInteractive, true);
    assert.equal(event.surface, 'terminal');
    assert.equal(typeof sessionId, 'string');
    assert(!ids.includes(sessionId));
    ids.push(sessionId);
    assert.equal(events.filter(e => e.kind === 'short_circuit_stop').length, 0);
    const complete = events.filter(e => e.kind === 'turn_complete');
    assert.equal(complete.length, 1);
    assert.equal(complete[0].value.reason, 'answer');
    assert.equal(complete[0].value.isAborted, false);
    assert.equal(complete[0].value.answer, 'Fixture answer');
    assert(complete[0].value.usage.output_tokens > 0);
    const hook = json(`${mode}-settings.json`).hooks.Stop[0].hooks[0];
    assert.equal(hook.type, 'command');
    assert.equal(hook.timeout, 1);
    assert.equal(hook.onFailure, 'block');
    const script = { baseline: 'stop-chain-command.mjs', missing: 'intentionally-missing-policy.mjs', timeout: 'stop-timeout-command.mjs' }[mode];
    assert.equal(hook.command, `node /home/node/probe/${script}`);
    const debug = read(`${mode}-debug.log`);
    assert(debug.includes('cc-plugin-sec-default@builtin seated outermost: this machine has managed settings'));
    assert(debug.includes('classic.Stop bypassed by cc-plugin-sec-default (tier user); beneath runs'));
    const transcript = rows(`${mode}-transcript.jsonl`);
    assert(transcript.some(row => row.sessionId === sessionId && row.subtype === 'turn_duration'));
    const main = requests.filter(row => row.method === 'POST' && row.path.startsWith('/v1/messages')
      && row.body?.tools?.length > 0 && JSON.stringify(row.body.messages).includes(`AIKIT_STOP_FAILURE_${mode.toUpperCase()}`));
    mainCounts.push(main.length);
    const calls = stops.filter(row => row.input.session_id === sessionId);
    if (mode === 'baseline') {
      assert.deepEqual(calls.map(row => [row.ordinal, row.result]), [[1, { decision: 'block', reason: 'AIKIT_STOP_CHAIN_FIRST_BLOCK' }], [2, {}]]);
      assert.equal(main.length, 2);
      assert(JSON.stringify(main[1].body.messages).includes('AIKIT_STOP_CHAIN_FIRST_BLOCK'));
      assert(transcript.some(row => row.attachment?.type === 'hook_blocking_error'));
      assert(transcript.some(row => row.attachment?.type === 'hook_success' && row.attachment.exitCode === 0));
    } else {
      assert.equal(calls.length, 0);
      assert.equal(main.length, 1);
      assert(!JSON.stringify(main[0].body.messages).includes('AIKIT_STOP_FAILURE_LATE_BLOCK'));
      assert(!transcript.some(row => row.attachment?.type === 'hook_blocking_error'));
      assert(debug.includes('not blocking (onFailure: "block" is ignored on Stop)'));
      if (mode === 'missing') {
        assert(debug.includes('MODULE_NOT_FOUND'));
        assert(debug.includes('Stop command hook failed; not blocking'));
      } else {
        assert(debug.includes('timed out after 1000ms'));
        assert(debug.includes('Stop command hook timed out; not blocking'));
        assert.equal(timeout.length, 1);
        assert.equal(timeout[0].phase, 'started');
        assert.equal(timeout[0].input.session_id, sessionId);
        assert.equal(timeout[0].input.hook_event_name, 'Stop');
        assert(transcript.some(row => row.attachment?.type === 'hook_cancelled'));
      }
    }
  }
  assert.equal(stops.length, 2);
  assert(requests.length <= 32);
  assert.deepEqual(Object.keys(json('networks.json')), ['none']);
  return { sessions: ids, main_requests: mainCounts, native_stop_failure_allows_completion: true,
    scope: 'Claude 2.1.296 Linux x86-64 guarded managed command hooks; no readiness promotion' };
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url))
  console.log(JSON.stringify(verify(process.argv[2]), null, 2));
