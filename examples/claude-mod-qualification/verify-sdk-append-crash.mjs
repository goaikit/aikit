// Independent cold SQLite/native transcript comparison for one combined SDK kill.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
assert(process.argv[2], 'capture root required');
const root=path.resolve(process.argv[2]),read=name=>fs.readFileSync(path.join(root,name),'utf8');
const json=name=>JSON.parse(read(name)),lines=name=>read(name).trim().split('\n').map(JSON.parse);
const sha=bytes=>createHash('sha256').update(bytes).digest('hex');
const setup=json('setup.json'),stage=json('stage.json'),marker=json('checkpoint-committed.json');
const cold=json('before-recovery-sqlite.json'),after=json('after-recovery-sqlite.json');
assert.equal(cold.schema_version,12);assert.equal(after.schema_version,12);
assert.equal(cold.outgoing.length,1);assert.equal(cold.append_checkpoints.length,1);
assert.equal(after.outgoing.length,1);assert.equal(after.append_checkpoints.length,1);
for(const database of [cold,after]){
  assert.deepEqual(database.handoffs,[]);assert.equal(database.qualified_observation_count,0);
}
const original=cold.outgoing[0],checkpoint=cold.append_checkpoints[0];
assert.equal(original.state,'dispatching');
assert.equal(original.binding_id,stage.binding_id);assert.deepEqual(original.request,stage.request);
assert.equal(original.target.native_session_id,stage.session_id);
assert.equal(original.target.installation_id,setup.installation);
assert.equal(original.request.content,'SDK_APPEND_CLIENT_REVIEW: exact 雪\nfeedback');
assert.equal(original.payload_sha256,sha(original.request.content));
for(const key of ['acknowledgement','consumption','failure'])assert.equal(original[key],null);
const expectedRecovered={...original,state:'unknown'};
assert.deepEqual(after.outgoing[0],expectedRecovered);
assert.deepEqual(json('recovered-receipt.json'),expectedRecovered);
assert.deepEqual(json('repeat-stage.json'),expectedRecovered);
const final=json('message-status.json');
assert.deepEqual(final.receipt,expectedRecovered);
assert.equal(final.unqualified_handoff,null);
assert.deepEqual(final.unqualified_append,checkpoint);
assert.deepEqual(after.append_checkpoints[0],checkpoint);
assert.deepEqual(marker.reply.append_checkpoint,checkpoint);
assert.equal(marker.stdout_withheld,true);
assert.equal(marker.frame.action.kind,'record_append_stored');
assert.equal(marker.frame.action.operation_id,original.request.operation_id);
assert.equal(marker.frame.action.reservation_id,original.reservation_id);
assert.equal(marker.frame.native_context.session_id,stage.session_id);
assert.equal(marker.frame.native_context.invocation_id,original.target.invocation_id);
const {reservation}=checkpoint;
for(const key of ['binding_id','request','payload_sha256','reservation_id','target'])
  assert.deepEqual(reservation[key],original[key]);
const rowId=checkpoint.intent.native_row_id;
assert.equal(marker.frame.action.native_row_id,rowId);
assert.match(rowId,/^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/);
const label=setup.plugin_name.length>24?setup.plugin_name.slice(0,23)+'…':setup.plugin_name;
const envelope=`The ${label} plugin sent a message:\n${original.request.content}`
  + "\n\nThis is how Claude Code surfaces a prompt a plugin submits between turns — it starts this turn in the user's place. Address the message above.";
assert.equal(checkpoint.intent.native_source,'claude-code/2.1.296/session.append');
assert.equal(checkpoint.intent.framed_payload_sha256,sha(envelope));
assert.equal(checkpoint.stored.stored_payload_sha256,sha(envelope));
assert.equal(checkpoint.stored.evidence_reference,`claude-code:session:${stage.session_id}:row:${rowId}`);
assert.deepEqual(marker.frame.action.stored,checkpoint.stored);
const killed=json('killed-state.json');
assert.equal(killed.Running,false);assert.equal(killed.ExitCode,137);assert.equal(killed.OOMKilled,false);
const delay=Date.parse(killed.FinishedAt)-checkpoint.stored_at_unix_ms;
assert(delay>=0 && delay<30000,'kill must precede the withheld RPC timeout');
for(const name of ['after-kill-transcript.jsonl','after-resume-transcript.jsonl']){
  const rows=lines(name),kept=rows.filter(row=>row.uuid===rowId);
  assert.equal(kept.length,1);assert.equal(kept[0].sessionId,stage.session_id);
  assert.equal(kept[0].type,'user');assert.equal(kept[0].message.role,'user');
  assert.equal(kept[0].isSidechain,false);
  assert.equal(kept[0].message.content,envelope);
  assert.deepEqual(kept[0].origin,{kind:'plugin',name:setup.plugin_name});
  assert.equal(rows.filter(row=>row.type==='queue-operation'&&row.operation==='enqueue'&&row.content===original.request.content).length,1);
}
const main=lines('requests.jsonl').filter(request=>request.method==='POST'
  && request.path.startsWith('/v1/messages')&&!request.path.includes('count_tokens')
  && request.body?.tools?.some(tool=>tool.name===`mcp__${setup.plugin_name}__review`));
assert.equal(main.length,1);
assert.equal(JSON.parse(main[0].body.metadata.user_id).session_id,stage.session_id);
assert.equal(JSON.stringify(main[0].body.messages).includes('SDK_APPEND_CLIENT_REVIEW'),false);
assert.deepEqual(json('resume-request-presence.json'),{exists:false});
const events=json('events.json').records;
const starts=events.filter(event=>event.request.event==='session_started');
assert.equal(starts.length,2);
assert.equal(starts[0].request.invocation_id,original.target.invocation_id);
assert.notEqual(starts[1].request.invocation_id,original.target.invocation_id);
for(const event of starts)assert.equal(event.request.session_id,stage.session_id);
assert.equal(events.filter(event=>event.request.event==='native_turn_finished').length,1);
assert.equal(json('historical-binding-status.json'),'stale');
assert.deepEqual(Object.keys(json('networks.json')),['none']);
const moduleHash=sha(read('generated-module.js'));
assert.equal(setup.plugin_version,'0.1.0-sdk'+moduleHash.slice(0,12));
assert(read('generated-module.js').includes('/home/node/probe/append-crash-bridge.mjs'));
console.log(JSON.stringify({passed:true,
  scope:'One Claude 2.1.296 Linux actual SDK post-commit/pre-reply SIGKILL and same-session resume',
  session_id:stage.session_id,row_uuid:rowId,kill_after_stored_checkpoint_ms:delay,
  old_operation_kept_original_invocation:true,duplicate_enqueue_observed:false,
  review_model_request_observed:false,production_acknowledgement_qualified:false,
  production_consumption_qualified:false,windows_qualified:false,
  hashes:Object.fromEntries(['setup.json','checkpoint-committed.json','before-recovery-sqlite.json','after-recovery-sqlite.json',
    'after-kill-transcript.jsonl','after-resume-transcript.jsonl','message-status.json','requests.jsonl','events.json','binary-identities.json']
    .map(name=>[name,sha(read(name))])),
},null,2));
