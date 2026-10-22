import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';

export function verify(directory) {
  const read = file => fs.readFileSync(path.join(directory, file), 'utf8').replace(/^\uFEFF/, '');
  const json = file => JSON.parse(read(file));
  const rows = file => read(file).trim().split('\n').filter(Boolean).map(JSON.parse);
  const requests = rows('requests.jsonl'), ids = [];
  for (const mode of ['baseline', 'invalid', 'timeout']) {
    const setup = json(`${mode}/setup.json`), calls = rows(`${mode}/calls.jsonl`);
    assert.equal(setup.tool.native_completion_bridge, true);
    assert.equal(setup.tool.timeout_ms, 1000);
    assert(setup.spec.events.includes('completion_proposed'));
    const module = read(`${mode}/generated-module.js`);
    const digest = createHash('sha256').update(module).digest('hex');
    assert.equal(setup.plugin_version, `0.1.0-sdk${digest.slice(0, 12)}`);
    const config = JSON.parse(module.match(/^const config = (.*);$/m)[1]);
    assert.equal(config.installationId, setup.installation);
    assert.equal(config.timeoutMs, 1000);
    assert.deepEqual(config.sessionArgv, [setup.spec.handler.executable, ...setup.spec.handler.arguments]);
    assert(module.includes("on('classic.Stop'"));
    assert(module.includes(".catch(() => ({ block: 'Completion validation unavailable: restore the local handler and retry.' }));"));
    const inputs = calls.filter(c => c.phase === 'input' && c.event.hook_event_name === 'Stop');
    assert.deepEqual(inputs.map(c => c.ordinal), [1, 2, 3]);
    const session = inputs[0].event.session_id;
    assert.equal(typeof session, 'string');
    assert(!ids.includes(session)); ids.push(session);
    assert(inputs.every(c => c.event.session_id === session && c.event.cwd === setup.workspace));
    const records = json(`${mode}/events.json`).records;
    const starts = records.filter(r => r.request.event === 'native_turn_started');
    const finishes = records.filter(r => r.request.event === 'native_turn_finished');
    assert.equal(starts.length, 1); assert.equal(finishes.length, 1);
    const start = starts[0].request, finish = finishes[0].request;
    assert.equal(start.session_id, session); assert.equal(finish.session_id, session);
    assert.equal(finish.invocation_id, start.invocation_id);
    assert.equal(finish.native_turn.id, start.native_turn.id);
    assert.deepEqual(finish.native_turn.outcome, { is_aborted: false, reason: 'answer' });
    assert.equal(finish.final_answer, 'Fixture answer');
    const decisions = records.filter(r => r.request.event === 'completion_proposed' && r.decision);
    assert.deepEqual(decisions.map(r => r.decision.decision), mode === 'baseline' ? ['block', 'allow', 'allow'] : ['allow', 'allow']);
    assert(decisions.every(r => r.request.session_id === session && r.request.invocation_id === start.invocation_id
      && r.request.native_turn.id === start.native_turn.id));
    const main = requests.filter(r => r.method === 'POST' && r.path.startsWith('/v1/messages')
      && r.body?.tools?.length > 0 && JSON.parse(r.body.metadata.user_id).session_id === session);
    assert.equal(main.length, 2);
    assert(main.every(r => JSON.stringify(r.body.messages).includes(`AIKIT_GENERATED_STOP_${mode.toUpperCase()}`)));
    const reason = mode === 'baseline' ? decisions[0].decision.reason
      : 'Completion validation unavailable: restore the local handler and retry.';
    assert(!JSON.stringify(main[0].body.messages).includes(reason));
    assert(JSON.stringify(main[1].body.messages).includes(reason));
    const transcript = rows(`${mode}/transcript.jsonl`);
    assert(transcript.some(r => r.sessionId === session && r.isMeta && r.message?.content === `Stop hook feedback:\n${reason}`));
    assert(transcript.some(r => r.sessionId === session && r.subtype === 'turn_duration'));
    const debug = read(`${mode}/${mode === 'invalid' ? 'final-debug.log' : 'debug.log'}`);
    const id = Object.keys(setup.settings.enabledPlugins)[0];
    assert(debug.includes(`hooks module ${id} loaded (worker, environment 1, tier prepend)`));
    assert(debug.includes('Hooks: Stop [') && debug.includes('generated-stop-handler.mjs'));
    const project = json(`${mode}/project-settings.json`);
    const projectHandlers = project.hooks.Stop.flatMap(g => g.hooks);
    assert.equal(projectHandlers.length, 1);
    assert.equal(projectHandlers[0].command, setup.spec.handler.executable);
    assert.deepEqual(projectHandlers[0].args, setup.spec.handler.arguments);
    assert.equal(projectHandlers[0].timeout, setup.spec.timeout_seconds);
    if (mode !== 'baseline') {
      assert(debug.includes('classic.Stop; its .catch answered'));
      assert(!calls.some(c => c.ordinal === 1 && c.phase === 'sdk_response'));
      assert(calls.some(c => c.ordinal === 1 && c.phase === (mode === 'invalid' ? 'invalid_response' : 'delayed')));
    }
    if (mode === 'timeout') {
      assert(!calls.some(c => c.phase === 'after_delay'));
      assert(inputs[1].at - inputs[0].at >= 1000);
    }
  }
  const prefix = read('invalid-premise-debug.log');
  assert.equal(read('invalid/debug.log'), prefix + read('invalid/final-debug.log'));
  assert(prefix.includes('which is not an enabled plugin with a hooks module; skipped'));
  assert.equal(requests.length, 9);
  assert.deepEqual(Object.keys(json('networks.json')), ['none']);
  return { sessions: ids, main_requests: [2, 2, 2], sdk_decisions: [['block', 'allow', 'allow'], ['allow', 'allow'], ['allow', 'allow']],
    generated_fallback_blocked: ['invalid', 'timeout'], total_requests: requests.length,
    scope: 'Claude 2.1.296 Linux x86-64; generated SDK example handler, no Codelaya policy/readiness/Windows proof' };
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url))
  console.log(JSON.stringify(verify(process.argv[2]), null, 2));
