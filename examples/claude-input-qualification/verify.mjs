// Independent evidence reader. Never invokes Claude or changes a saved trial.
import fs from 'node:fs';
import assert from 'node:assert/strict';
export function verify(root,{early=false}={}){
  const read=name=>fs.readFileSync(`${root}/${name}`,'utf8');
  const json=name=>JSON.parse(read(name));
  const events=json('input-events.json');
  const starts=events.filter(row=>row.kind==='session_start');assert.equal(starts.length,1);
  const session=starts[0].value.session_id;assert.match(session,/^[a-f0-9-]{36}$/);
  const before=events.filter(row=>row.kind==='before').map(row=>row.value);
  const after=events.filter(row=>row.kind==='after').map(row=>row.value);
  assert.equal(before.length,2);assert.equal(after.length,2);
  assert.notEqual(before[0].event.turnId,before[1].event.turnId);
  const contains=(messages,text)=>messages.some(message=>typeof message.content==='string'
    ?message.content.includes(text):message.content.some(block=>block.type==='text'&&block.text.includes(text)));
  const late='AIKIT_INPUT_LATE_雪';
  for(let i=0;i<2;i++){
    assert.equal(before[i].session_id,session);assert.equal(after[i].session_id,session);
    assert.deepEqual(before[i].event,after[i].event);assert.equal(before[i].event.index,0);
    assert.equal(before[i].event.agentId,undefined);
    assert.equal(contains(before[i].messages,late),early&&i===1);
    const core=after[i].trace.filter(row=>row.plugin==='engine'&&row.tier==='core'&&row.event==='turn.step');
    assert.equal(core.length,1);assert(['returned','passed'].includes(core[0].outcome));
    assert.deepEqual(core[0].received,before[i].event);
    assert.equal(core[0].returned.answer,'Native input comparison response.');
    assert.equal(after[i].result.answer,core[0].returned.answer);
  }
  assert(contains(before[0].messages,'AIKIT_INPUT_CONTROL'));
  assert(contains(before[1].messages,'AIKIT_INPUT_MUTATION'));
  assert.equal(contains(after[0].messages,late),false);
  assert.equal(contains(after[1].messages,late),true);
  const appended=json('late-append.json');assert.equal(appended.session_id,session);
  assert.deepEqual(appended.event,before[1].event);assert.equal(appended.result.deny,undefined);
  assert.match(appended.result.uuid,/^[a-f0-9-]{36}$/);
  assert.equal(contains(appended.before,late),false);assert(contains(appended.after,late));
  assert(contains([appended.result.message],late));
  const requests=read('requests.jsonl').trim().split('\n').map(JSON.parse);
  assert(requests.length<=12);
  const main=requests.filter(row=>row.body?.tools?.some(tool=>tool.name==='Read'));
  assert.equal(main.length,2);
  for(let i=0;i<2;i++){
    assert.equal(new URL(main[i].path,'http://fixture').pathname,'/v1/messages');
    assert.equal(JSON.parse(main[i].body.metadata.user_id).session_id,session);
    assert(contains(main[i].body.messages,i===0?'AIKIT_INPUT_CONTROL':'AIKIT_INPUT_MUTATION'));
    assert.equal(contains(main[i].body.messages,late),false);
  }
  for(const row of requests.filter(row=>!main.includes(row))){
    assert.equal(row.body.tools?.length??0,0);
    assert(JSON.stringify(row.body.messages).includes('Write the title in the predominant language'));
  }
  const transcript=read('native-transcript.jsonl').trim().split('\n').map(JSON.parse);
  const rows=transcript.filter(row=>row.uuid===appended.result.uuid);assert.equal(rows.length,1);
  assert.equal(rows[0].sessionId,session);assert(contains([rows[0].message],late));
  assert.deepEqual(json('settings-before.json'),json('settings-after.json'));
  const debug=read('native-debug.log');
  const loaded=(name,tier)=>new RegExp(`hooks module ${name}@input-qualification loaded \\(worker, environment [0-9]+, tier ${tier}\\)`).test(debug);
  assert(loaded('input-observer',early?'user':'prepend'));
  assert(loaded('input-shortcut',early?'prepend':'user'));
  assert(!debug.includes('hook failed: input-'));assert(!debug.includes('input-shortcut: turn.step hook skipped'));
  return {passed:true,session,native_version:'2.1.308',main_requests:2,other_requests:requests.length-2,
    pre_snapshot_excludes_late_row:!early,late_row_stored:true,post_snapshot_contains_unsent_row:true,
    late_row_sent_to_this_main_request:false,core_trace_present_for_both:true,
    pre_snapshot_plus_core_supported_for_this_trial:!early,production_consumption_qualified:false,windows_qualified:false};
}
if(process.argv[1]?.replaceAll('\\','/').endsWith('/verify.mjs'))
  console.log(JSON.stringify(verify(process.argv[2],{early:process.argv[3]==='--early'}),null,2));
