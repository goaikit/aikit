import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
export function verify(directory) {
  const read=name=>fs.readFileSync(path.join(directory,name),'utf8').replace(/^\uFEFF/,'');
  const json=name=>JSON.parse(read(name));
  const rows=name=>read(name).trim().split('\n').filter(Boolean).map(JSON.parse);
  const core={plugin:'engine',tier:'core'};
  const observed=json('origin-events.json');
  const start=observed.find(e=>e.kind==='session_start').value;
  assert.deepEqual(start.origin,core);
  assert.equal(start.event.isInteractive,true);assert.equal(start.event.surface,'terminal');
  const calls=observed.filter(e=>e.kind==='tool_call');
  assert.equal(calls.length,2);
  const peer=calls.find(e=>e.value.text==='AIKIT_PLUGIN_RAISED_TOOL');
  assert.deepEqual(peer.value.origin,{plugin:'aikit-mod-feasibility',tier:'user'});
  assert.deepEqual(calls.find(e=>e.value.text==='Native tool stdin 雪\n').value.origin,core);
  const ends=observed.filter(e=>e.kind==='turn_complete');
  assert.equal(ends.length,1);assert.deepEqual(ends[0].value.origin,core);
  assert.equal(ends[0].value.event.answer,'Fixture answer');
  assert.equal(ends[0].value.event.reason,'answer');
  assert(!observed.some(e=>e.kind==='plugin_completion_return'));
  assert(read('origin-final-debug.log').includes('$.turn.complete is not a function'));
  for(const item of observed.filter(e=>e.kind==='stop'||e.kind==='turn_complete')) {
    assert.deepEqual(item.value.before,[]);
    assert.equal(item.value.after.length,1);
    assert.equal(item.value.after[0].plugin,'engine');
    assert.equal(item.value.after[0].tier,'core');
  }
  const setup=json('guarded/setup.json'), spy=json('guarded/spy-result.json');
  assert.deepEqual(spy.origin,core); // The turn.start caller, not its nested tool caller.
  assert.equal(spy.result.deny,'Agent operation requires a native engine tool dispatch.');
  const handler=rows('guarded/handler-calls.jsonl');
  assert.equal(handler.length,1);
  assert.equal(handler[0].schema_version,1);
  assert.equal(handler[0].application_id,setup.spec.application_id);
  assert.equal(handler[0].installation_id,setup.spec.installation_id);
  assert.equal(handler[0].native_context.session_id,spy.sessionId);
  assert.deepEqual(handler[0].request,{text:'Native SDK bridge stdin 雪\n'});
  assert.notEqual(spy.sessionId,start.sessionId);
  const source=read('guarded/generated-module.js');
  assert(source.includes("next?.origin?.plugin !== 'engine' || next.origin.tier !== 'core'"));
  assert.equal(json('guarded/plugin.json').version,`0.1.0-sdk${createHash('sha256').update(source).digest('hex').slice(0,12)}`);
  const requests=rows('guarded/requests.jsonl');
  const main=requests.filter(r=>r.method==='POST'&&r.path.startsWith('/v1/messages')&&r.body?.tools?.some(t=>t.name===setup.tool_name));
  assert.equal(main.length,2);
  assert(main.every(r=>JSON.parse(r.body.metadata.user_id).session_id===spy.sessionId));
  const results=main[1].body.messages.flatMap(m=>Array.isArray(m.content)?m.content:[])
    .filter(b=>b.type==='tool_result'&&b.tool_use_id==='fixture-native-bridge');
  assert.equal(results.length,1);assert(!results[0].is_error);
  assert.deepEqual(JSON.parse(results[0].content),handler[0]);
  const transcript=rows('guarded/native-transcript.jsonl');
  assert(transcript.some(r=>r.sessionId===spy.sessionId&&r.subtype==='turn_duration'));
  const debug=read('guarded/guarded-final-debug.log');
  assert(debug.includes(`hooks module ${setup.plugin_name}@inline loaded`));
  assert(debug.includes('hooks module origin-spy@inline loaded'));
  assert(!debug.includes('hook skipped'));
  assert.deepEqual(Object.keys(json('networks.json')),['none']);
  return {origin_distinguished:true,foreign_dispatch_denied:true,handler_calls:handler.length,
    guarded_main_requests:main.length,raised_completion_api:'unavailable on tested native surface',
    scope:'Claude 2.1.296 Linux x86-64; SDK-rendered unowned export; no physical caller/configuration attestation'};
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url))
  console.log(JSON.stringify(verify(process.argv[2]),null,2));
