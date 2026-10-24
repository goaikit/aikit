// Independent factual assertions; never submit, resume or repair native state.
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
const directory=process.argv[2];assert(path.isAbsolute(directory));
const read=name=>fs.readFileSync(path.join(directory,name),'utf8').replace(/^\uFEFF/,'');
const json=name=>JSON.parse(read(name));
const hash=name=>createHash('sha256').update(fs.readFileSync(path.join(directory,name))).digest('hex');
const owner=json('owner-review.json');
const before=json('native-inspection.json'),after=json('native-inspection-after-exit.json');
const requests=read('requests.jsonl').trim().split('\n').map(JSON.parse);
const rollout=read('rollout-after-exit.jsonl').trim().split('\n').map(JSON.parse);
assert.equal(requests.length,4,'two main requests and two title requests required');
const baseline='CODELAYA_CODEX_BASELINE: Reply with the fixture answer. Do not call tools.';
const messageText=item=>(item.content??[]).map(part=>part.text??'').join('');
const lastUser=request=>request.body.input.filter(item=>item.role==='user').at(-1);
const main=requests.filter(request=>[baseline,owner.message].includes(messageText(lastUser(request))));
assert.equal(main.length,2);assert.equal(messageText(lastUser(main[0])),baseline);
assert.equal(messageText(lastUser(main[1])),owner.message);
assert.equal(main[1].body.input.filter(item=>item.role==='user'&&messageText(item)===owner.message).length,1);
assert.equal(main[0].body.input.filter(item=>item.role==='user'&&messageText(item)===owner.message).length,0);
const queueMatch=read('queue-output.log').trim().match(/^Queued message ([\da-f-]+) for thread ([\da-f-]+)\.$/);
assert(queueMatch);assert.equal(queueMatch[2],owner.thread_id);
const metadata=rollout.find(item=>item.type==='session_meta').payload;
assert.equal(metadata.id,owner.thread_id);assert.equal(metadata.cli_version,'0.163.0');
assert.equal(path.resolve(metadata.cwd),path.join(directory,'workspace'));
for(const inspection of [before,after]){
 const thread=inspection.history.thread;
 assert.equal(thread.id,owner.thread_id);assert.equal(thread.cliVersion,'0.163.0');
 assert.equal(path.resolve(thread.cwd),path.join(directory,'workspace'));
 assert.equal(thread.turns.length,2);assert(thread.turns.every(turn=>turn.status==='completed'));
 assert.deepEqual(inspection.queue.data,[]);assert.equal(inspection.queue.nextCursor,null);
 const messages=thread.turns[1].items.filter(item=>item.type==='userMessage');
 assert.equal(messages.length,1);assert.equal(messageText(messages[0]),owner.message);
 assert.equal(typeof messages[0].clientId,'string');assert(messages[0].clientId.length>0);
 assert(thread.turns[1].items.some(item=>item.type==='agentMessage'&&item.text==='CODELAYA_CODEX_FIXTURE_ANSWER_3'));
}
assert.deepEqual(before.history.thread.turns,after.history.thread.turns);
assert.equal(rollout.filter(item=>item.type==='event_msg'&&item.payload.type==='task_complete').length,2);
console.log(JSON.stringify({passed:true,scope:'Windows Codex 0.163.0 native CLI queue feasibility with scripted replies',
 native_thread_source:after.history.thread.source,main_requests:2,title_requests:2,queue_submissions:1,
 exact_original_session_input:true,retained_after_terminal_exit:true,native_queue_empty:true,
 terminal_source_classification_qualified:false,sdk_message_transport_implemented:false,
 native_caller_authenticated:false,production_acknowledgement_qualified:false,production_consumption_qualified:false,
 full_windows_acceptance:false,requests_sha256:hash('requests.jsonl'),history_sha256:hash('native-inspection-after-exit.json'),
 rollout_sha256:hash('rollout-after-exit.jsonl')},null,2));
