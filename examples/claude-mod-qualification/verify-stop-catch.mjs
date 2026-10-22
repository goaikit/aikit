import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';

export function verify(directory) {
  const read = name => fs.readFileSync(path.join(directory, name), 'utf8').replace(/^\uFEFF/, '');
  const json = name => JSON.parse(read(name));
  const rows = name => read(name).trim().split('\n').filter(Boolean).map(JSON.parse);
  const requests = rows('requests.jsonl');
  const ids = [];
  const counts = [];
  for (const mode of ['throw', 'timeout', 'catch-failure']) {
    const events = json(`${mode}-events.json`);
    const select = kind => events.filter(e => e.kind === kind);
    assert.equal(select('session_start').length, 1);
    const start = select('session_start')[0].value;
    assert.equal(start.mode, mode);
    assert.equal(start.event.surface, 'terminal');
    assert.equal(start.event.isInteractive, true);
    assert.equal(typeof start.sessionId, 'string');
    assert(!ids.includes(start.sessionId));
    ids.push(start.sessionId);
    const stops = select('stop');
    assert.deepEqual(stops.map(e => e.value.stops), mode === 'catch-failure' ? [1] : [1, 2]);
    for (const stop of stops) {
      assert.equal(stop.value.event.session_id, start.sessionId);
      assert.equal(stop.value.event.hook_event_name, 'Stop');
      assert.equal(stop.value.mode, mode);
    }
    assert.equal(select('catch').length, 1);
    const caught = select('catch')[0];
    assert.equal(caught.value.event.session_id, start.sessionId);
    assert.equal(caught.value.called, false);
    assert.equal(caught.value.error.kind, mode === 'timeout' ? 'timeout' : 'throw');
    assert.equal(caught.value.error.budget, 1000);
    if (mode === 'timeout') assert(caught.at - stops[0].at >= 10000);
    else assert.equal(caught.value.error.message, 'AIKIT_STOP_CATCH_INJECTED_FAILURE');
    const main = requests.filter(r => r.method === 'POST' && r.path.startsWith('/v1/messages')
      && r.body?.tools?.length > 0
      && JSON.parse(r.body.metadata.user_id).session_id === start.sessionId);
    const marker = `AIKIT_STOP_CATCH_${mode === 'catch-failure' ? 'FAILURE' : mode.toUpperCase()}`;
    assert(main.every(r => JSON.stringify(r.body.messages).includes(marker)));
    counts.push(main.length);
    assert.equal(main.length, mode === 'catch-failure' ? 1 : 2);
    assert.equal(select('turn_complete').length, 1);
    const complete = select('turn_complete')[0];
    assert.equal(complete.value.reason, 'answer');
    assert.equal(complete.value.isAborted, false);
    assert.equal(complete.value.answer, 'Fixture answer');
    assert.equal(typeof complete.value.turnId, 'string');
    assert(complete.value.usage.output_tokens > 0);
    assert(complete.at >= caught.at);
    const transcript = rows(`${mode}-transcript.jsonl`);
    assert(transcript.some(r => r.sessionId === start.sessionId && r.subtype === 'turn_duration'));
    const debug = read(`${mode}-debug.log`);
    assert(debug.includes('aikit-mod-feasibility@inline loaded (worker, environment 1, tier user)'));
    assert(!debug.includes('cc-plugin-sec-default@builtin seated outermost'));
    if (mode === 'catch-failure') {
      assert.equal(select('catch_block').length, 0);
      assert(debug.includes('its .catch threw Error: AIKIT_STOP_CATCH_FALLBACK_FAILURE'));
      assert(!transcript.some(r => r.isMeta && r.message?.content === 'Stop hook feedback:\nAIKIT_STOP_CATCH_BLOCK'));
    } else {
      assert.equal(select('catch_block').length, 1);
      assert.deepEqual(select('catch_block')[0].value.result, { block: 'AIKIT_STOP_CATCH_BLOCK' });
      assert(JSON.stringify(main[1].body.messages).includes('AIKIT_STOP_CATCH_BLOCK'));
      assert(!JSON.stringify(main[0].body.messages).includes('AIKIT_STOP_CATCH_BLOCK'));
      assert(transcript.some(r => r.sessionId === start.sessionId && r.isMeta
        && r.message?.content === 'Stop hook feedback:\nAIKIT_STOP_CATCH_BLOCK'));
      assert(debug.includes('classic.Stop; its .catch answered'));
    }
  }
  assert.equal(requests.length, 8);
  assert.deepEqual(Object.keys(json('networks.json')), ['none']);
  return { sessions: ids, main_requests: counts, total_requests: requests.length,
    callback_failures_recovered: ['throw', 'timeout'], fallback_failure_allows_completion: true,
    scope: 'Claude 2.1.296 Linux x86-64 user-tier mod; scripted responses; no readiness promotion' };
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url))
  console.log(JSON.stringify(verify(process.argv[2]), null, 2));
