import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';

export function verify(directory) {
  const read = file => fs.readFileSync(path.join(directory, file), 'utf8').replace(/^\uFEFF/, '');
  const json = file => JSON.parse(read(file));
  const rows = file => read(file).trim().split('\n').filter(Boolean).map(JSON.parse);
  const requests = rows('requests.jsonl');
  const ids = [];
  for (const mode of ['user', 'managed']) {
    const events = json(`${mode}-events.json`);
    const select = kind => events.filter(e => e.kind === kind);
    assert.equal(select('session_start').length, 1);
    const { sessionId, event } = select('session_start')[0].value;
    assert.equal(event.isInteractive, true);
    assert.equal(event.surface, 'terminal');
    assert.equal(typeof sessionId, 'string');
    assert(!ids.includes(sessionId));
    ids.push(sessionId);
    const stops = select('stop');
    assert.deepEqual(stops.map(s => s.value.stops), [1, 2]);
    for (const stop of stops) assert.equal(stop.value.event.session_id, sessionId);
    assert.equal(select('catch').length, 1);
    const caught = select('catch')[0].value;
    assert.equal(caught.called, false);
    assert.equal(caught.event.session_id, sessionId);
    assert.equal(caught.error.kind, 'throw');
    assert.equal(caught.error.message, 'AIKIT_STOP_CATCH_INJECTED_FAILURE');
    assert.equal(select('catch_block').length, 1);
    assert.deepEqual(select('catch_block')[0].value.result, { block: 'AIKIT_STOP_CATCH_BLOCK' });
    const complete = select('turn_complete');
    assert.equal(complete.length, 1);
    assert.equal(complete[0].value.reason, 'answer');
    assert.equal(complete[0].value.isAborted, false);
    assert.equal(complete[0].value.answer, 'Fixture answer');
    const main = requests.filter(r => r.method === 'POST' && r.path.startsWith('/v1/messages')
      && r.body?.tools?.length > 0 && JSON.parse(r.body.metadata.user_id).session_id === sessionId);
    assert.equal(main.length, 2);
    assert(main.every(r => JSON.stringify(r.body.messages).includes(`AIKIT_PREPEND_STOP_${mode.toUpperCase()}`)));
    assert(!JSON.stringify(main[0].body.messages).includes('AIKIT_STOP_CATCH_BLOCK'));
    assert(JSON.stringify(main[1].body.messages).includes('AIKIT_STOP_CATCH_BLOCK'));
    const transcript = rows(`${mode}-transcript.jsonl`);
    assert(transcript.some(r => r.sessionId === sessionId && r.isMeta
      && r.message?.content === 'Stop hook feedback:\nAIKIT_STOP_CATCH_BLOCK'));
    assert(transcript.some(r => r.sessionId === sessionId && r.subtype === 'turn_duration'));
    const settings = json(`${mode}-settings.json`);
    assert.equal(settings.enabledPlugins['stop-guard@aikit-prepend-probe'], true);
    assert.deepEqual(settings.prependPlugins, ['stop-guard@aikit-prepend-probe', ...(mode === 'managed' ? ['sec-default@builtin'] : [])]);
    const debug = read(`${mode}-debug.log`);
    assert(debug.includes('hooks module stop-guard@aikit-prepend-probe loaded (worker, environment 1, tier prepend)'));
    assert(debug.includes('stop-competitor@inline loaded (worker, environment'));
    assert(debug.includes(`prependPlugins from ${mode === 'user' ? 'user settings' : 'managed policy'}:`));
    assert(debug.includes('classic.Stop; its .catch answered'));
    const competitor = json(`${mode}-competitor-events.json`);
    assert.equal(competitor.filter(e => e.kind === 'session_start').length, 1);
    assert.equal(competitor.find(e => e.kind === 'session_start').value.sessionId, sessionId);
    const skipped = competitor.filter(e => e.kind === 'short_circuit_stop');
    assert.equal(skipped.length, mode === 'user' ? 1 : 0);
    if (mode === 'user') {
      assert.equal(skipped[0].value.session_id, sessionId);
      assert.equal(skipped[0].value.stop_hook_active, true);
    } else {
      assert(debug.includes('cc-plugin-sec-default@builtin seated by managed prependPlugins at position 2'));
      assert(debug.includes('classic.Stop bypassed by cc-plugin-sec-default (tier user); beneath runs'));
    }
    const otherComplete = competitor.filter(e => e.kind === 'turn_complete');
    assert.equal(otherComplete.length, 1);
    assert.equal(otherComplete[0].value.turnId, complete[0].value.turnId);
  }
  assert.equal(requests.length, 6);
  assert.deepEqual(Object.keys(json('networks.json')), ['none']);
  return { sessions: ids, main_requests: [2, 2], total_requests: requests.length,
    ordered_catch_preserved_block: ['user', 'managed'],
    scope: 'Claude 2.1.296 Linux x86-64; one short-circuit competitor; scripted replies; no readiness promotion' };
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url))
  console.log(JSON.stringify(verify(process.argv[2]), null, 2));
