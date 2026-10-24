import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
const directory=process.argv[2];assert(path.isAbsolute(directory));
const mainAnalysis=process.argv[3]==='--main-loop-analysis';
assert.equal(process.argv.length,mainAnalysis?4:3,'unexpected analysis arguments');
const read=name=>fs.readFileSync(path.join(directory,name),'utf8'),json=name=>JSON.parse(read(name));
assert.equal(read('native-version.txt').trim(),'2.1.308 (Claude Code)');
const events=json('dispatch-events.json'),starts=events.filter(r=>r.kind==='session_start');
const before=events.filter(r=>r.kind==='before'),after=events.filter(r=>r.kind==='after'),ends=events.filter(r=>r.kind==='turn_complete');
assert.equal(starts.length,1);assert.equal(before.length,2);assert.equal(after.length,2);assert.equal(ends.length,2);
const session=starts[0].value.session_id;assert.match(session,/^[0-9a-f-]{36}$/);
assert.notEqual(before[0].value.event.turnId,before[1].value.event.turnId);
for(let i=0;i<2;i++){
  for(const row of [before[i],after[i],ends[i]])assert.equal(row.value.session_id,session);
  assert.equal(after[i].value.event.turnId,before[i].value.event.turnId);
  assert.equal(ends[i].value.event.turnId,before[i].value.event.turnId);
  assert.equal(before[i].value.event.index,0);assert.equal(after[i].value.event.index,0);
  assert.deepEqual(before[i].value.origin,{plugin:'engine',tier:'core'});
  assert.equal(ends[i].value.event.reason,'answer');assert.equal(ends[i].value.event.isAborted,false);
}
assert(JSON.stringify(before[0].value.messages).includes('AIKIT_DISPATCH_CONTROL'));
assert(!JSON.stringify(before[0].value.messages).includes('AIKIT_DISPATCH_WITHOUT_ENGINE'));
assert(JSON.stringify(before[1].value.messages).includes('AIKIT_DISPATCH_WITHOUT_ENGINE'));
assert.equal(after[0].value.matching_core,true);assert.equal(after[0].value.naive_consumption_candidate,false);
assert.equal(after[1].value.matching_core,false);assert.equal(after[1].value.naive_consumption_candidate,true);
assert.equal(after[0].value.result.answer,'Native dispatch control response.');
const core=after[0].value.trace.filter(r=>r.plugin==='engine'&&r.tier==='core'&&r.event==='turn.step');
assert.equal(core.length,1);assert(['returned','passed'].includes(core[0].outcome));
assert.equal(core[0].received.turnId,before[0].value.event.turnId);assert.equal(core[0].received.index,0);
assert.equal(core[0].returned.answer,'Native dispatch control response.');
assert.equal(core[0].returned.usage.input_tokens,100);assert.equal(core[0].returned.stopReason,'end_turn');
for(const key of ['messages','input','payload','messageIds','requestId','request_id'])
  assert.equal(core[0].received[key],undefined,'this capture does not expose an exact input witness');
assert.equal(after[1].value.trace.filter(r=>r.plugin==='engine'&&r.tier==='core'&&r.event==='turn.step').length,0);
assert(after[1].value.trace.some(r=>['dispatch-shortcut','dispatch-shortcut@dispatch-qualification'].includes(r.plugin)
  &&r.tier==='user'&&r.event==='turn.step'&&r.outcome==='returned'));
const shortcut=json('shortcut.json');assert.equal(shortcut.session_id,session);assert.equal(shortcut.delegated,false);
assert.equal(shortcut.event.turnId,before[1].value.event.turnId);assert.deepEqual(shortcut.origin,{plugin:'engine',tier:'core'});
assert.deepEqual(shortcut.result,after[1].value.result);
assert.equal(shortcut.result.answer,'Synthetic handler response; no model request.');
assert.equal(shortcut.result.usage.input_tokens,100);assert.equal(shortcut.result.stopReason,'end_turn');
const requests=read('requests.jsonl').trim().split('\n').map(JSON.parse);
assert(requests.length<=6);
const withMarker=requests.filter(r=>JSON.stringify(r.body?.messages).includes('AIKIT_DISPATCH_WITHOUT_ENGINE'));
if(mainAnalysis){
  // Original no-HTTP-send premise failed: independently classify the unexpected
  // session-title calls, never relabel them as no send or main-loop consumption.
  assert(withMarker.length>0,'this analysis requires the retained failed premise');
  for(const row of withMarker){
    assert.equal(new URL(row.path,'http://fixture').pathname,'/v1/messages');
    assert.equal(row.body.tools?.length??0,0);
    assert(JSON.stringify(row.body.messages).includes('Write the title in the predominant language'));
    assert.equal(JSON.parse(row.body.metadata.user_id).session_id,session);
  }
}else assert.equal(withMarker.length,0,'original no-HTTP-send hypothesis');
const main=requests.filter(r=>new URL(r.path,'http://fixture').pathname==='/v1/messages'
  &&r.body?.tools?.some(tool=>tool.name==='Read'));
assert.equal(main.length,1);assert.equal(JSON.parse(main[0].body.metadata.user_id).session_id,session);
assert(JSON.stringify(main[0].body.messages).includes('AIKIT_DISPATCH_CONTROL'));
assert(!JSON.stringify(main[0].body.messages).includes('AIKIT_DISPATCH_WITHOUT_ENGINE'));
assert.deepEqual(json('settings-before.json'),json('settings-after.json'));
const debug=read('native-debug.log');
assert(debug.includes('hooks module dispatch-observer@dispatch-qualification loaded (worker, environment 1, tier prepend)'));
assert(debug.includes('hooks module dispatch-shortcut@dispatch-qualification loaded (worker, environment 2, tier user)'));
assert(!debug.includes('hook failed: dispatch-'));
const transcript=read('native-transcript.jsonl').trim().split('\n').map(JSON.parse);
assert(transcript.some(r=>r.type==='user'&&JSON.stringify(r.message).includes('AIKIT_DISPATCH_WITHOUT_ENGINE')));
for(const row of transcript)if(row.sessionId!==undefined)assert.equal(row.sessionId,session);
console.log(JSON.stringify({passed:true,native_version:'2.1.308',session,main_requests:main.length,
  analysis:mainAnalysis?'separate main-loop analysis':'original no-HTTP-send hypothesis',
  original_no_http_send_hypothesis:withMarker.length===0,title_requests_with_marker:withMarker.length,
  actual_control_core_trace:true,synthetic_usage_without_core:true,shortcut_not_sent_to_main:true,
  full_wire_input_attested:false,production_consumption_qualified:false,windows_qualified:false,
  requests_sha256:createHash('sha256').update(read('requests.jsonl')).digest('hex')},null,2));
