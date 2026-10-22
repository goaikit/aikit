import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';

// Verify retained native observations, never infer protection from settings alone.
export function verify(directory) {
  const read = name => fs.readFileSync(path.join(directory, name), 'utf8').replace(/^\uFEFF/, '');
  const json = name => JSON.parse(read(name));
  const rows = name => read(name).trim().split('\n').filter(Boolean).map(JSON.parse);
  const requests = rows('requests.jsonl');
  const stops = rows('command-stops.jsonl');
  const modes = ['user', 'managed', 'empty-managed', 'guard-present'];
  const markers = ['USER', 'MANAGED', 'GUARD_ONLY', 'GUARD_PRESENT'];
  const ids = [];
  const counts = [];
  for (const [index, mode] of modes.entries()) {
    const events = json(`${mode}-events.json`);
    const starts = events.filter(e => e.kind === 'session_start');
    assert.equal(starts.length, 1);
    const { sessionId, event } = starts[0].value;
    assert.equal(event.surface, 'terminal');
    assert.equal(event.isInteractive, true);
    assert.equal(event.cwd, '/home/node/workspace');
    assert.equal(typeof sessionId, 'string');
    assert(!ids.includes(sessionId), 'runs must use distinct sessions');
    ids.push(sessionId);
    const calls = stops.filter(row => row.input.session_id === sessionId);
    const main = requests.filter(row => row.method === 'POST'
      && row.path.startsWith('/v1/messages') && row.body?.tools?.length > 0
      && JSON.stringify(row.body.messages).includes(`AIKIT_MANAGED_STOP_${markers[index]}`));
    counts.push(main.length);
    const completed = events.filter(e => e.kind === 'turn_complete');
    assert.equal(completed.length, 1);
    assert.equal(completed[0].value.reason, 'answer');
    assert.equal(completed[0].value.isAborted, false);
    assert.equal(completed[0].value.answer, 'Fixture answer');
    assert(completed[0].value.usage.output_tokens > 0);
    const transcript = rows(`${mode}-transcript.jsonl`);
    assert(transcript.some(row => row.sessionId === sessionId && row.subtype === 'turn_duration'));
    const competitor = events.filter(e => e.kind === 'short_circuit_stop');
    const debug = read(`${mode}-debug.log`);
    assert(debug.includes('hooks module aikit-mod-feasibility@inline loaded'));
    const guarded = debug.includes('cc-plugin-sec-default@builtin seated outermost: this machine has managed settings');
    const protectedRun = mode === 'managed' || mode === 'guard-present';
    assert.equal(guarded, protectedRun);
    if (protectedRun) {
      assert.deepEqual(calls.map(row => [row.ordinal, row.result]), [
        [1, { decision: 'block', reason: 'AIKIT_STOP_CHAIN_FIRST_BLOCK' }], [2, {}],
      ]);
      assert(calls.every(row => row.input.hook_event_name === 'Stop'));
      assert.equal(calls[0].input.stop_hook_active, false);
      assert.equal(calls[1].input.stop_hook_active, true);
      assert.equal(competitor.length, 0);
      assert.equal(main.length, 2);
      assert(!JSON.stringify(main[0].body.messages).includes('AIKIT_STOP_CHAIN_FIRST_BLOCK'));
      assert(JSON.stringify(main[1].body.messages).includes('AIKIT_STOP_CHAIN_FIRST_BLOCK'));
      assert(debug.includes('classic.Stop bypassed by cc-plugin-sec-default (tier user); beneath runs'));
      assert(transcript.some(row => row.attachment?.type === 'hook_blocking_error'
        && row.attachment.blockingError?.blockingError === 'AIKIT_STOP_CHAIN_FIRST_BLOCK'));
      assert(transcript.some(row => row.attachment?.type === 'hook_success'
        && row.attachment.hookEvent === 'Stop' && row.attachment.exitCode === 0));
    } else {
      assert.equal(calls.length, 0);
      assert.equal(competitor.length, 1);
      assert.equal(competitor[0].value.session_id, sessionId);
      assert.equal(main.length, 1);
      assert(!JSON.stringify(main[0].body.messages).includes('AIKIT_STOP_CHAIN_FIRST_BLOCK'));
      assert(!transcript.some(row => row.attachment?.hookEvent === 'Stop'));
    }
  }
  assert.equal(stops.length, 4, 'no foreign or extra Stop calls');
  assert(requests.length <= 32);
  const managed = json('managed-settings.json');
  assert.deepEqual(managed, json('user-settings.json'));
  assert.deepEqual(managed, json('empty-managed-user-settings.json'));
  assert.deepEqual(managed, json('guard-present-user-settings.json'));
  assert.deepEqual(json('empty-managed-managed-settings.json'), {});
  assert.deepEqual(json('guard-present-managed-settings.json'), { env: { AIKIT_MANAGED_CONTROL: '1' } });
  assert.equal(managed.hooks.Stop[0].hooks[0].command, 'node /home/node/probe/stop-chain-command.mjs');
  assert.deepEqual(Object.keys(json('networks.json')), ['none']);
  return { sessions: Object.fromEntries(modes.map((mode, i) => [mode, ids[i]])),
    main_requests: counts, managed_hook_resisted_tested_bypass: true,
    user_hook_with_guard_resisted_tested_bypass: true,
    empty_managed_file_did_not_load_guard: true,
    scope: 'Claude 2.1.296 Linux x86-64, one competitor; no production readiness promotion' };
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url))
  console.log(JSON.stringify(verify(process.argv[2]), null, 2));
