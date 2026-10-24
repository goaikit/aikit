// Owner-side fixture staging only. Does not start Claude or type a follow-up.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
const evidence=process.argv[2] || '/home/node/evidence';
const setup=JSON.parse(fs.readFileSync(`${evidence}/setup.json`));
assert(!fs.existsSync(`${evidence}/staged.json`),'do not restage a used fixture');
const run=(args)=>JSON.parse(execFileSync('/generated/integration_hooks',args,{encoding:'utf8',maxBuffer:4*1024*1024}));
const events=run(['events',setup.state,setup.installation_id]);
const finished=events.records.filter(record=>record.request.event==='native_turn_finished');
assert.equal(finished.length,1,'stage only after one actual initial native Turn');
const first=finished[0].request;
assert.equal(first.native_turn.outcome.reason,'answer');
assert.equal(first.native_turn.outcome.is_aborted,false);
const binding=run(['bind',setup.state,setup.installation_id,first.session_id]);
assert.equal(binding.reference.invocation_id,first.invocation_id);
const content=JSON.stringify({schema_version:1,review_id:'review-native-client',message:'AIKIT_DURABLE_REVIEW: preserve exact 雪\nfeedback',
  discussions:[{id:'thread-native',file:'src/fixture.rs',message:'Please explain this change.'}]});
const receipt=JSON.parse(execFileSync('/generated/integration_messages',[setup.state,binding.id,'review-native-client'],
  {input:content,encoding:'utf8',maxBuffer:2*1024*1024}));
assert.equal(receipt.request.content,content);
assert.equal(receipt.state,'accepted');
fs.writeFileSync(`${evidence}/before-stage-events.json`,JSON.stringify(events,null,2));
fs.writeFileSync(`${evidence}/staged.json`,JSON.stringify({binding,receipt},null,2));
console.log(JSON.stringify({binding_id:binding.id,operation_id:receipt.request.operation_id,payload_sha256:receipt.payload_sha256}));
