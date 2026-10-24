// Intentional native ID/content fault probes in a new disposable busy session.
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {randomUUID} from 'node:crypto';
import {connect} from './proxy.mjs';
const [executable,home,directory]=process.argv.slice(2);
for(const value of [executable,home,directory])assert(path.isAbsolute(value));
assert(fs.readFileSync(path.join(home,'config.toml'),'utf8').includes('Codelaya loopback fixture'));
const output=path.join(directory,'queue-contract.json');assert(!fs.existsSync(output),'never repeat a prepared contract probe');
const owner=JSON.parse(fs.readFileSync(path.join(directory,'owner-review.json')));
const gateway=JSON.parse(fs.readFileSync(path.join(directory,'gateway.json')));
const base=gateway.url.replace(/\/v1$/,'');assert.equal(new URL(base).hostname,'127.0.0.1');
const state=async()=>{const response=await fetch(`${base}/fixture/state`,{signal:AbortSignal.timeout(3000)});assert(response.ok);return response.json();};
const client=await connect(executable,home);
const record={schema_version:1,thread_id:owner.thread_id,probe_client_id:randomUUID(),delivery_client_id:randomUUID()};
const result=reply=>{assert(!reply.error,JSON.stringify(reply.error));return reply.result;};
const input=text=>[{type:'text',text,text_elements:[]}];
try{
 record.before_gateway=await state();assert.equal(record.before_gateway.held,true);
 const history=result(await client.rpc('thread/read',{threadId:owner.thread_id,includeTurns:true}));
 assert.equal(path.resolve(history.thread.cwd),path.join(directory,'workspace'));
 assert.equal(history.thread.turns.length,1);assert.equal(history.thread.turns[0].status,'inProgress');
 record.baseline_turn_id=history.thread.turns[0].id;record.native_thread_source=history.thread.source;
 const initial=result(await client.rpc('thread/queue/list',{threadId:owner.thread_id,limit:100}));assert.deepEqual(initial.data,[]);
 const params={threadId:owner.thread_id,clientUserMessageId:record.probe_client_id,input:input('CODELAYA_CODEX_IDEMPOTENCE_PROBE')};
 record.first=await client.rpc('thread/queue/add',params);result(record.first);
 // Prior response was received, and the baseline remains held. These are
 // deliberate duplicate/content probes, not recovery of an uncertain handoff.
 assert.equal((await state()).held,true);
 record.same=await client.rpc('thread/queue/add',params);
 record.changed=await client.rpc('thread/queue/add',{...params,input:input('CODELAYA_CODEX_CHANGED_PROBE')});
 record.probe_queue=result(await client.rpc('thread/queue/list',{threadId:owner.thread_id,limit:100}));
 assert.equal(record.probe_queue.nextCursor,null);
 assert(record.probe_queue.data.every(item=>item.clientUserMessageId===record.probe_client_id));
 for(const id of new Set(record.probe_queue.data.map(item=>item.id)))result(await client.rpc('thread/queue/delete',{threadId:owner.thread_id,queuedSubmissionId:id}));
 assert.deepEqual(result(await client.rpc('thread/queue/list',{threadId:owner.thread_id,limit:100})).data,[]);
 record.delivery=result(await client.rpc('thread/queue/add',{threadId:owner.thread_id,clientUserMessageId:record.delivery_client_id,input:input(owner.message)}));
 record.pending=result(await client.rpc('thread/queue/list',{threadId:owner.thread_id,limit:100}));
 assert.equal(record.pending.data.length,1);assert.deepEqual(record.pending.data[0],record.delivery.queuedSubmission);
 assert.equal(record.pending.data[0].clientUserMessageId,record.delivery_client_id);
 assert.deepEqual(record.pending.data[0].input,input(owner.message));
 record.after_gateway=await state();assert.equal(record.after_gateway.held,true);
 const busy=result(await client.rpc('thread/read',{threadId:owner.thread_id,includeTurns:true}));
 assert.equal(busy.thread.turns.length,1);assert.equal(busy.thread.turns[0].id,record.baseline_turn_id);assert.equal(busy.thread.turns[0].status,'inProgress');
 record.prepared=true;
 console.log(JSON.stringify({prepared:true,first_queue_id:record.first.result.queuedSubmission.id,
   duplicate_queue_id:record.same.result?.queuedSubmission?.id??null,changed_error:record.changed.error??null,
   pending_delivery_id:record.delivery.queuedSubmission.id,baseline_still_busy:true}));
}finally{
 record.trace=client.trace;fs.writeFileSync(output,JSON.stringify(record,null,2),{flag:'wx'});client.close();
}
