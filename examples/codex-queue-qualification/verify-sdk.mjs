// Exercise the reusable Rust SDK against retained native records; no live I/O.
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
const [executable,directory]=process.argv.slice(2);
assert(path.isAbsolute(executable));assert(path.isAbsolute(directory));
const json=name=>JSON.parse(fs.readFileSync(path.join(directory,name),'utf8').replace(/^\uFEFF/,''));
const record=json('queue-contract.json'),owner=json('owner-review.json');
const after=json('native-inspection-after-restart.json');
const busy=record.trace.filter(item=>item.method==='thread/read').at(-1).reply.result;
const query=(client,content,queue,history)=>({thread_id:owner.thread_id,client_message_id:client,frozen_content:content,queue,history});
function inspect(input){
 const child=spawnSync(executable,[],{input:JSON.stringify(input),encoding:'utf8',timeout:10000,maxBuffer:1024*1024});
 assert.equal(child.status,0,child.stderr);return JSON.parse(child.stdout);
}
const pending=inspect(query(record.delivery_client_id,owner.message,record.pending,busy));
assert.equal(pending.records.length,1);assert.equal(pending.records[0].location.kind,'queued');
assert.equal(pending.records[0].location.queue_id,record.delivery.queuedSubmission.id);
assert.equal(pending.records[0].payload_matches,true);
const retained=inspect(query(record.delivery_client_id,owner.message,after.queue,after.history));
assert.equal(retained.records.length,1);assert.equal(retained.records[0].location.kind,'retained');
assert.equal(retained.records[0].location.turn_id,after.history.thread.turns[1].id);
assert.equal(retained.records[0].payload_matches,true);
const duplicate=inspect(query(record.probe_client_id,'CODELAYA_CODEX_IDEMPOTENCE_PROBE',record.probe_queue,busy));
assert.equal(duplicate.records.length,3);assert.deepEqual(duplicate.records.map(item=>item.payload_matches),[true,true,false]);
const overlap=inspect(query(record.delivery_client_id,owner.message,record.pending,after.history));
assert.equal(overlap.records.length,2);assert(overlap.records.every(item=>item.payload_matches));
const changed=inspect(query(record.delivery_client_id,owner.message+'changed',after.queue,after.history));
assert.equal(changed.records.length,1);assert.equal(changed.records[0].payload_matches,false);
const foreign=query(record.delivery_client_id,owner.message,after.queue,after.history);foreign.thread_id='foreign';
const rejected=spawnSync(executable,[],{input:JSON.stringify(foreign),encoding:'utf8',timeout:10000});assert.notEqual(rejected.status,0);
console.log(JSON.stringify({passed:true,cases:6,copied_native_records:true,live_native_io:false,
 duplicate_matches_preserved:3,overlapping_matches_preserved:2,changed_payload_not_matched:true,foreign_scope_rejected:true,
 native_generation_qualified:false,production_delivery_qualified:false,
 executable_sha256:createHash('sha256').update(fs.readFileSync(executable)).digest('hex')},null,2));
