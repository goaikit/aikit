// Deterministic module tests, not native provider qualification.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
const base = await fs.readFile(new URL('./agent_tools_claude.js', import.meta.url), 'utf8');
const completion = await fs.readFile(new URL('./agent_tools_completion_claude.js', import.meta.url), 'utf8');
let sequence = 0;
async function fixture() {
  const hooks = new Map(), catches = new Map(), frames = [];
  const config = { applicationId: 'fixture', installationId: 'owned', toolName: 'review',
    description: 'Fixture', schema: { type: 'object' }, instructions: '',
    sessionArgv: ['fixed-handler', 'fixed-argument'], argv: ['tools'], timeoutMs: 1000 };
  const source = base.replace('/* AIKIT_COMPLETION_BRIDGE */', completion)
    .replace('/* AIKIT_CONFIG */null', JSON.stringify(config));
  const module = await import('data:text/javascript,' + encodeURIComponent(source) + `#${++sequence}`);
  module.register((name, ...args) => {
    hooks.set(name, args.at(-1));
    return { catch(handler) { catches.set(name, handler); } };
  });
  const api = { session: { id: async () => 'native', cwd: async () => '/workspace' },
    env: { set: async () => {} }, tool: { register: async () => {} }, ui: { invalidate() {} },
    process: { run: async (argv, options) => {
      assert.deepEqual(argv, config.sessionArgv);
      assert.equal(options.timeoutMs, 1000);
      frames.push(JSON.parse(options.stdin));
      return { exitCode: 0, isStdoutTruncated: false, isStderrTruncated: false, stdout: '{}' };
    } } };
  const event = { hook_event_name: 'Stop', session_id: 'native', cwd: '/workspace',
    last_assistant_message: 'Exact answer 雪\n', stop_hook_active: true };
  let delegated = 0;
  const next = async e => { assert.equal(e, event); delegated++; return { block: 'downstream decision' }; };
  const controller = new AbortController();
  next.signal = controller.signal;
  const start = () => hooks.get('turn.start')(api, { turnId: 'turn' }, async () => ({}));
  await hooks.get('session.start')(api, {}, async () => ({}));
  frames.length = 0;
  return { api, hooks, catches, frames, event, next, controller, start, delegated: () => delegated };
}
test('Allow delegates unchanged; Block preserves exact reason and does not skip into downstream', async () => {
  const f = await fixture();
  await f.start();
  assert.deepEqual(await f.hooks.get('classic.Stop')(f.api, f.event, f.next), { block: 'downstream decision' });
  assert.equal(f.delegated(), 1);
  assert.deepEqual(f.frames[1], f.event);
  f.api.process.run = async () => ({ exitCode: 0, isStdoutTruncated: false, isStderrTruncated: false,
    stdout: JSON.stringify({ decision: 'block', reason: 'Fresh notes required 雪\n' }) });
  assert.deepEqual(await f.hooks.get('classic.Stop')(f.api, f.event, f.next), { block: 'Fresh notes required 雪\n' });
  assert.equal(f.delegated(), 1);
});
test('missing context, foreign session, subagent and oversized input refuse without a policy call', async () => {
  const f = await fixture();
  const invoke = event => f.hooks.get('classic.Stop')(f.api, event, f.next);
  assert.match((await invoke(f.event)).block, /context/);
  await f.start();
  for (const event of [{ ...f.event, session_id: 'foreign' }, { ...f.event, cwd: '/foreign' },
    { ...f.event, agent_id: 'child' }, { ...f.event, last_assistant_message: 'x'.repeat(131073) }])
    assert.equal(typeof (await invoke(event)).block, 'string');
  assert.equal(f.frames.length, 1);
  assert.equal(f.delegated(), 0);
});
test('failed, truncated and unsupported responses cannot Allow; parse and process failures use a constant catch', async () => {
  const f = await fixture();
  await f.start();
  for (const patch of [{ exitCode: 1 }, { isStdoutTruncated: true }, { isStderrTruncated: true },
    { stdout: 'null' }, { stdout: '[]' }, { stdout: '{"continue":false}' },
    { stdout: '{"decision":"allow"}' }, { stdout: '{"decision":"block","reason":" "}' },
    { stdout: JSON.stringify({ decision: 'block', reason: 'x'.repeat(32769) }) }, { stdout: 'x'.repeat(65537) }]) {
    f.api.process.run = async () => ({ exitCode: 0, isStdoutTruncated: false, isStderrTruncated: false, stdout: '{}', ...patch });
    assert.equal(typeof (await f.hooks.get('classic.Stop')(f.api, f.event, f.next)).block, 'string');
  }
  for (const run of [async () => { throw Error('process unavailable'); }, async () => ({ exitCode: 0,
    isStdoutTruncated: false, isStderrTruncated: false, stdout: 'invalid JSON' })]) {
    f.api.process.run = run;
    await assert.rejects(f.hooks.get('classic.Stop')(f.api, f.event, f.next));
    assert.match(f.catches.get('classic.Stop')().block, /restore/);
  }
  assert.equal(f.delegated(), 0);
});
test('cancellation or a new Turn while the callback runs cannot delegate stale Allow', async () => {
  for (const change of ['abort', 'new-turn', 'clear']) {
    const f = await fixture();
    await f.start();
    f.api.process.run = async () => {
      if (change === 'abort') f.controller.abort();
      else if (change === 'new-turn') {
        f.api.process.run = async () => ({ exitCode: 0, isStdoutTruncated: false, isStderrTruncated: false, stdout: '{}' });
        await f.hooks.get('turn.start')(f.api, { turnId: 'replacement' }, async () => ({}));
      } else {
        f.api.process.run = async () => ({exitCode: 0, isStdoutTruncated: false, isStderrTruncated: false, stdout: '{}'});
        await f.hooks.get('session.end')(f.api, {reason: 'clear'}, async () => ({}));
        f.api.session.id = async () => 'replacement';
        await f.hooks.get('classic.SessionStart')(f.api,
          {source: 'clear', session_id: 'replacement', cwd: '/workspace'}, async () => ({}));
      }
      return { exitCode: 0, isStdoutTruncated: false, isStderrTruncated: false, stdout: '{}' };
    };
    assert.match((await f.hooks.get('classic.Stop')(f.api, f.event, f.next)).block, /changed or was cancelled/);
    assert.equal(f.delegated(), 0);
  }
});
