// Verify recorded native counterexamples and the distinct busy delivery case.
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
const directory=process.argv[2];assert(path.isAbsolute(directory));
const read=name=>fs.readFileSync(path.join(directory,name),'utf8').replace(/^\uFEFF/,'');
const json=name=>JSON.parse(read(name));
const hash=name=>createHash('sha256').update(fs.readFileSync(path.join(directory,name))).digest('hex');
const record=json('queue-contract.json'),owner=json('owner-review.json');
assert.equal(record.prepared,true);assert.equal(record.thread_id,owner.thread_id);
assert.equal(record.before_gateway.held,true);assert.equal(record.after_gateway.held,true);
assert.notEqual(record.probe_client_id,record.delivery_client_id);
const ids=[record.first,record.same,record.changed].map(reply=>{
 assert(!reply.error);assert.equal(reply.result.queuedSubmission.clientUserMessageId,record.probe_client_id);
 return reply.result.queuedSubmission.id;
});
assert.equal(new Set(ids).size,3,'native duplicate-ID counterexample must be retained');
assert.equal(record.probe_queue.data.length,3);
assert.deepEqual(record.probe_queue.data.map(item=>item.id),ids);
assert(record.probe_queue.data.every(item=>item.clientUserMessageId===record.probe_client_id));
const text=item=>(item.content??item.input??[]).map(part=>part.text??'').join('');
assert.deepEqual(record.probe_queue.data.map(text),[
 'CODELAYA_CODEX_IDEMPOTENCE_PROBE','CODELAYA_CODEX_IDEMPOTENCE_PROBE','CODELAYA_CODEX_CHANGED_PROBE']);
const adds=record.trace.filter(item=>item.method==='thread/queue/add');assert.equal(adds.length,4);
assert.deepEqual(adds[0].params,adds[1].params);
assert.equal(adds[2].params.clientUserMessageId,adds[0].params.clientUserMessageId);
const deletes=record.trace.filter(item=>item.method==='thread/queue/delete');assert.equal(deletes.length,3);
assert.deepEqual(deletes.map(item=>item.params.queuedSubmissionId),ids);assert(deletes.every(item=>!item.reply.error));
assert(record.trace.every(item=>['initialize','thread/read','thread/queue/list','thread/queue/add','thread/queue/delete'].includes(item.method)));
assert.equal(record.pending.data.length,1);assert.equal(text(record.pending.data[0]),owner.message);
assert.equal(record.pending.data[0].clientUserMessageId,record.delivery_client_id);
assert.equal(record.pending.data[0].id,record.delivery.queuedSubmission.id);
const controls=json('gateway-controls.json');assert.equal(controls.length,1);assert.equal(controls[0].action,'release');
assert.equal(json('release.json').http_status,200);
assert(record.trace.at(-1).observed_at_unix_ms<=controls[0].at_unix_ms);
let originalTurns;
for(const file of ['native-inspection.json','native-inspection-after-exit.json','native-inspection-after-restart.json']){
 const inspection=json(file),thread=inspection.history.thread;
 assert.equal(thread.id,owner.thread_id);assert.equal(path.resolve(thread.cwd),path.join(directory,'workspace'));
 assert.equal(thread.turns.length,2);assert(thread.turns.every(turn=>turn.status==='completed'));
 assert.equal(thread.turns[0].id,record.baseline_turn_id);
 const delivered=thread.turns[1].items.filter(item=>item.type==='userMessage');assert.equal(delivered.length,1);
 assert.equal(delivered[0].clientId,record.delivery_client_id);assert.equal(text(delivered[0]),owner.message);
 assert.deepEqual(inspection.queue.data,[]);assert.equal(inspection.queue.nextCursor,null);
 if(originalTurns)assert.deepEqual(thread.turns,originalTurns);else originalTurns=thread.turns;
}
assert(read('daemon-stop-before-restart.json').includes('"status":"stopped"'));
const requests=read('requests.jsonl').trim().split('\n').map(JSON.parse);assert.equal(requests.length,4);
const baseline='CODELAYA_CODEX_BASELINE: Reply with the fixture answer. Do not call tools.';
const main=requests.filter(request=>[baseline,owner.message].includes(text(request.body.input.filter(item=>item.role==='user').at(-1))));
assert.equal(main.length,2);assert.equal(main[1].body.input.filter(item=>item.role==='user'&&text(item)===owner.message).length,1);
assert(!requests.some(request=>JSON.stringify(request.body.input).includes('CODELAYA_CODEX_IDEMPOTENCE_PROBE')||JSON.stringify(request.body.input).includes('CODELAYA_CODEX_CHANGED_PROBE')));
assert(!fs.existsSync(path.join(directory,'rejected-requests.jsonl')));
const rollout=read('rollout-after-restart.jsonl').trim().split('\n').map(JSON.parse);
assert.equal(rollout.filter(item=>item.type==='event_msg'&&item.payload.type==='task_complete').length,2);
console.log(JSON.stringify({passed:true,scope:'Windows Codex 0.163.0 scripted busy queue/native ID contract probe',
 native_same_id_deduplicates:false,native_changed_content_conflicts:false,probe_entries_deleted_before_release:3,
 distinct_feedback_queued_while_busy:true,native_client_id_retained:true,retained_after_daemon_restart:true,
 recorded_main_requests:2,recorded_title_requests:2,production_delivery_qualified:false,native_admission_qualified:false,
 native_caller_authenticated:false,full_windows_acceptance:false,
 contract_sha256:hash('queue-contract.json'),requests_sha256:hash('requests.jsonl'),
 restart_inspection_sha256:hash('native-inspection-after-restart.json'),rollout_sha256:hash('rollout-after-restart.jsonl')},null,2));
