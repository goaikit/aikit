// Independent SDK-journal/native transcript/wire comparison for one captured run.
// It does not promote native capability or write a delivery proof.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
assert(process.argv[2], 'capture root required');
const root=path.resolve(process.argv[2]);
const read=name=>fs.readFileSync(path.join(root,name),'utf8');
const json=name=>JSON.parse(read(name));
const jsonl=name=>read(name).trim().split('\n').map(JSON.parse);
const sha=bytes=>createHash('sha256').update(bytes).digest('hex');
const setup=json('setup.json'),stage=json('stage.json'),status=json('message-status.json');
assert.deepEqual(status,json('before-exit-status.json'));
const receipt=status.receipt,append=status.unqualified_append,handoff=status.unqualified_handoff;
assert.deepEqual(json('sqlite-inspection.json'),{schema_version:12,outgoing:[receipt],append_checkpoints:[append],handoffs:[handoff],qualified_observation_count:0});
assert.equal(receipt.state,'unknown');
for(const key of ['acknowledgement','consumption','failure'])assert.equal(receipt[key],null);
assert.equal(receipt.request.content,'SDK_APPEND_CLIENT_REVIEW: exact 雪\nfeedback');
assert.deepEqual(receipt.request,stage.request);
assert.deepEqual(json('repeat-stage.json'),receipt);
assert.equal(receipt.payload_sha256,sha(receipt.request.content));
assert.equal(receipt.binding_id,stage.binding_id);
assert.equal(receipt.target.native_session_id,stage.session_id);
assert.equal(receipt.target.installation_id,setup.installation);
assert.deepEqual(append.reservation,handoff.reservation);
for(const key of ['binding_id','request','payload_sha256','reservation_id','target'])
  assert.deepEqual(append.reservation[key],receipt[key]);
assert.equal(append.schema_version,1);
assert.equal(append.intent.native_source,'claude-code/2.1.296/session.append');
const rowId=append.intent.native_row_id;
assert.match(rowId,/^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/);
assert.match(setup.plugin_name,/^[a-z0-9-]{1,35}$/);
const label=setup.plugin_name.length>24?setup.plugin_name.slice(0,23)+'…':setup.plugin_name;
const envelope=`The ${label} plugin sent a message:\n${receipt.request.content}`
  + "\n\nThis is how Claude Code surfaces a prompt a plugin submits between turns — it starts this turn in the user's place. Address the message above.";
assert.equal(append.intent.framed_payload_sha256,sha(envelope));
assert.equal(append.stored.stored_payload_sha256,sha(envelope));
assert.equal(append.stored.evidence_reference,`claude-code:session:${stage.session_id}:row:${rowId}`);
assert(append.prepared_at_unix_ms<=append.stored_at_unix_ms);
assert(append.stored_at_unix_ms<=handoff.recorded_at_unix_ms);
assert.deepEqual(handoff.outcome,{kind:'returned',result:{text:receipt.request.content,origin:{kind:'plugin',name:setup.plugin_name}}});
const rows=jsonl('transcript.jsonl'),kept=rows.filter(row=>row.uuid===rowId);
assert.equal(kept.length,1);assert.equal(kept[0].sessionId,stage.session_id);
assert.equal(kept[0].type,'user');assert.equal(kept[0].message.role,'user');
assert.equal(kept[0].message.content,envelope);
assert.deepEqual(kept[0].origin,{kind:'plugin',name:setup.plugin_name});
assert.equal(rows.filter(row=>row.type==='queue-operation'&&row.operation==='enqueue'&&row.content===receipt.request.content).length,1);
const main=jsonl('requests.jsonl').filter(request=>request.body?.tools?.some(tool=>tool.name===`mcp__${setup.plugin_name}__review`));
assert.equal(main.length,2);
for(const request of main)assert.equal(JSON.parse(request.body.metadata.user_id).session_id,stage.session_id);
assert.equal(JSON.stringify(main[0].body.messages).includes('SDK_APPEND_CLIENT_REVIEW'),false);
const sent=main[1].body.messages.filter(message=>message.role==='user')
  .flatMap(message=>typeof message.content==='string'?[message.content]:message.content.filter(block=>block.type==='text').map(block=>block.text));
assert.equal(sent.filter(value=>value===envelope).length,1);
assert.equal(JSON.stringify(main[1].body.messages).split(JSON.stringify(receipt.request.content).slice(1,-1)).length-1,1);
const events=json('events.json').records;
const finished=events.filter(record=>record.request.event==='native_turn_finished');
assert.equal(finished.length,2);
for(const record of finished){
  assert.equal(record.request.session_id,stage.session_id);
  assert.equal(record.request.invocation_id,receipt.target.invocation_id);
  assert.deepEqual(record.request.native_turn.outcome,{reason:'answer',is_aborted:false});
}
const moduleHash=sha(read('generated-module.js'));
assert.equal(setup.plugin_version,'0.1.0-sdk'+moduleHash.slice(0,12));
assert.equal(json('binary-identities.json')[setup.plugin+'/hooks/register.js'],moduleHash);
assert.deepEqual(Object.keys(json('networks.json')),['none']);
console.log(JSON.stringify({passed:true,scope:'One ordinary Linux Claude 2.1.296 session with actual generated SDK module/stdio bridge/SQLite append checkpoints',
  session_id:stage.session_id,row_uuid:rowId,main_requests:main.length,checkpoint_survived_normal_exit:true,
  logical_payload_sha256:receipt.payload_sha256,native_envelope_sha256:sha(envelope),
  production_acknowledgement_qualified:false,production_consumption_qualified:false,windows_qualified:false,
  combined_sdk_sigkill_path_qualified:false,
  hashes:Object.fromEntries(['setup.json','stage.json','message-status.json','sqlite-inspection.json','transcript.jsonl','requests.jsonl','events.json','generated-module.js','binary-identities.json'].map(name=>[name,sha(read(name))])),
},null,2));
