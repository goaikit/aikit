// Native resume plus post-receipt crash only; no mid-handoff crash claim.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
const evidence=path.resolve(process.argv[2]);
const read=name=>fs.readFileSync(path.join(evidence,name),'utf8');
const json=name=>JSON.parse(read(name)),hash=value=>createHash('sha256').update(value).digest('hex');
const baseline=json('verification.json'),original=json('after-exit-status.json'),setup=json('setup.json');
assert.equal(baseline.passed,true);assert.equal(baseline.main_requests,2);
for(const name of ['resumed-idle-status.json','resumed-live-status.json','after-crash-status.json'])
  assert.deepEqual(json(name),original,`no retarget, new attempt, proof fabrication or raw return loss in ${name}`);
assert.deepEqual(json('after-crash-events.json'),json('resumed-live-events.json'),'SIGKILL must not invent a graceful SessionEnd');
const rows=json('after-crash-events.json').records;
const select=kind=>rows.filter(row=>row.request.event===kind);
const sessions=select('session_started'),starts=select('native_turn_started'),finishes=select('native_turn_finished');
assert.equal(sessions.length,2);assert.equal(starts.length,3);assert.equal(finishes.length,3);
assert.equal(select('session_ended').length,1);
assert.equal(sessions[0].request.session_id,sessions[1].request.session_id);
assert.notEqual(sessions[0].request.invocation_id,sessions[1].request.invocation_id);
assert.equal(original.receipt.target.invocation_id,sessions[0].request.invocation_id);
assert.equal(starts[2].request.invocation_id,sessions[1].request.invocation_id);
assert.equal(finishes[2].request.invocation_id,sessions[1].request.invocation_id);
assert.equal(starts[2].request.native_turn.id,finishes[2].request.native_turn.id);
assert.deepEqual(finishes[2].request.native_turn.outcome,{reason:'answer',is_aborted:false});
const text=read('requests.jsonl'),lines=text.trimEnd().split('\n');
assert(lines.some((_,i)=>hash(lines.slice(0,i+1).join('\n')+'\n')===baseline.requests_sha256),'original request evidence must be an unchanged prefix');
const tool=`mcp__${path.posix.basename(setup.root)}__review`;
const requests=lines.map(JSON.parse).filter(row=>row.method==='POST'
  && new URL(row.path,'http://fixture').pathname==='/v1/messages' && row.body?.tools?.some(t=>t.name===tool));
assert.equal(requests.length,3,'only one extra request, from the separate owner prompt');
const third=requests[2].body;
assert.equal(JSON.parse(third.metadata.user_id).session_id,original.receipt.target.native_session_id);
const blocks=third.messages.flatMap(message=>typeof message.content==='string'?[{type:'text',text:message.content}]:message.content);
assert.equal(blocks.filter(block=>block?.type==='text' && block.text?.includes(original.receipt.request.content)).length,1,'old review appears once in history, never appended again');
assert.equal(blocks.filter(block=>block?.type==='text' && block.text?.includes('AIKIT_OWNER_AFTER_RESUME')).length,1);
console.log(JSON.stringify({passed:true,scope:'Linux native resume then SIGKILL after a retained receipt and completed owner Turn',
  main_requests:3,native_session_reused:true,invocation_replaced:true,original_receipt_unchanged:true,
  original_unknown_not_resent:true,post_receipt_crash_reopen:true,graceful_end_not_fabricated:true,
  requests_sha256:hash(text),status_sha256:hash(read('after-crash-status.json')),events_sha256:hash(read('after-crash-events.json')),
  mid_handoff_crash_tested:false,native_idle_lease_qualified:false,production_acknowledgement_qualified:false,
  production_consumption_qualified:false,windows_qualified:false},null,2));
