// Independent actual-input verifier. Never writes acknowledgement/consumption.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
const evidence=path.resolve(process.argv[2]);
const read=name=>fs.readFileSync(path.join(evidence,name),'utf8');
const json=name=>JSON.parse(read(name));
const sha=value=>createHash('sha256').update(value).digest('hex');
const setup=json('setup.json'),staged=json('staged.json'),live=json('live-status.json'),after=json('after-exit-status.json');
assert.equal(setup.production_ready,false);
assert.equal(staged.receipt.state,'accepted');
assert.equal(staged.receipt.payload_sha256,sha(staged.receipt.request.content));
assert.equal(staged.receipt.request.operation_id,'review-native-client');
assert.equal(JSON.parse(staged.receipt.request.content).message,'AIKIT_DURABLE_REVIEW: preserve exact 雪\nfeedback');
assert.deepEqual(after,live,'raw return and receipt must survive native exit and SDK reopen');
assert.equal(live.receipt.state,'unknown','raw candidate must not fabricate qualified delivery');
for(const field of ['acknowledgement','consumption','failure'])assert.equal(live.receipt[field],null);
assert.deepEqual(live.receipt.request,staged.receipt.request);
assert.deepEqual(live.receipt.target,staged.receipt.target);
assert.equal(live.receipt.binding_id,staged.binding.id);
const raw=live.unqualified_handoff;
assert.equal(raw.schema_version,1);
assert.equal(raw.reservation.reservation_id,live.receipt.reservation_id);
assert.deepEqual(raw.reservation.target,staged.receipt.target);
assert.deepEqual(raw.reservation.request,staged.receipt.request);
assert.equal(raw.reservation.payload_sha256,staged.receipt.payload_sha256);
assert.equal(raw.outcome.kind,'returned');
assert.equal(raw.outcome.result.text,staged.receipt.request.content);
assert.equal(raw.outcome.result.origin.kind,'plugin');
assert.equal(raw.outcome.result.origin.name,path.posix.basename(setup.root));
assert(Number.isSafeInteger(raw.recorded_at_unix_ms) && raw.recorded_at_unix_ms>0);
const rows=json('after-exit-events.json').records;
const select=kind=>rows.filter(row=>row.request.event===kind);
const sessions=select('session_started'),starts=select('native_turn_started'),finishes=select('native_turn_finished');
assert.equal(sessions.length,1);assert.equal(starts.length,2);assert.equal(finishes.length,2);
assert.notEqual(starts[0].request.native_turn.id,starts[1].request.native_turn.id);
const ref=staged.receipt.target;
for(const row of [...sessions,...starts,...finishes]){
  assert.equal(row.request.session_id,ref.native_session_id);
  assert.equal(row.request.invocation_id,ref.invocation_id);
}
for(let i=0;i<2;i++){
  assert.equal(starts[i].request.native_turn.id,finishes[i].request.native_turn.id);
  assert.deepEqual(finishes[i].request.native_turn.outcome,{reason:'answer',is_aborted:false});
}
assert.equal(finishes[0].request.final_answer,'Initial fixture answer');
assert.equal(finishes[1].request.final_answer,'Durable feedback received by scripted fixture');
assert.equal(select('session_ended').length,1);
const nativeTool=`mcp__${path.posix.basename(setup.root)}__review`;
const requests=read('requests.jsonl').trim().split('\n').map(JSON.parse).filter(row=>
  row.method==='POST' && new URL(row.path,'http://fixture').pathname==='/v1/messages'
  && row.body?.tools?.some(tool=>tool.name===nativeTool));
assert.equal(requests.length,2,'one initial and one native feedback request, no resend');
for(const [index,row] of requests.entries()){
  assert.equal(JSON.parse(row.body.metadata.user_id).session_id,ref.native_session_id);
  const blocks=row.body.messages.flatMap(message=>typeof message.content==='string'?[{type:'text',text:message.content}]:message.content);
  const payloads=blocks.filter(block=>block?.type==='text' && block.text?.includes(staged.receipt.request.content));
  assert.equal(payloads.length,index===0?0:1,'exact complete payload in independently captured native request');
}
const report={passed:true,scope:'Linux ordinary Claude terminal, generated SDK module, scripted loopback replies',
  main_requests:requests.length,exact_payload_in_actual_input:true,original_session:ref.native_session_id,
  reservation_id:live.receipt.reservation_id,raw_return_persisted:true,unknown_preserved:true,
  requests_sha256:sha(read('requests.jsonl')),status_sha256:sha(read('after-exit-status.json')),
  events_sha256:sha(read('after-exit-events.json')),production_ready:false,native_caller_authenticated:false,
  protected_admission_qualified:false,production_acknowledgement_qualified:false,
  production_consumption_qualified:false,windows_qualified:false};
console.log(JSON.stringify(report,null,2));
