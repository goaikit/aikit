import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import {verify} from '../verify-native-api-reuse.mjs';
const original=path.resolve(process.argv[2]);verify(original);
const targetUuid=JSON.parse(fs.readFileSync(path.join(original,'history-reuse-20261024/expected-review.json'))).native_row_id;
const files={
  'inbox-auth-20261024':['container.json','native-version.txt','model-dispatch-summary.json','native-loaded.log','inbox-auth.json','process-match.json','peer-after-exit.json'],
  'row-read-20261024':['container.json','native-version.txt','model-dispatch-summary.json','native-loaded.log','row-read.json','stored-row.json'],
  'history-reuse-20261024':['expected-review.json','history-review.json','history-note-published.json','history-review-published.json','history-before.sha','history-published-after.sha','published-manifest.json'],
};
const json=f=>s=>{const v=JSON.parse(s);f(v);return JSON.stringify(v);};
const mutations=[
  ['wrong native binary','inbox-auth-20261024/inbox-auth.json',json(v=>v.facts.connections[0].peer_executable_sha256='foreign')],
  ['wrong birth time','inbox-auth-20261024/process-match.json',json(v=>v.peer_start_ticks='0')],
  ['invented auth acknowledgement','inbox-auth-20261024/inbox-auth.json',json(v=>v.facts.connections[0].observation='auth_ok')],
  ['wrong stored note UUID','row-read-20261024/stored-row.json',json(v=>v.rows[0].uuid='foreign')],
  ['changed review body','history-reuse-20261024/history-review-published.json',json(v=>{
    const row=v.messages.Ok.find(m=>m.uuid===targetUuid);row.content.text+='changed';
  })],
  ['foreign history session','history-reuse-20261024/history-review-published.json',json(v=>v.info.Ok.session_id='foreign')],
  ['wrong dependency revision','history-reuse-20261024/published-manifest.json',json(v=>v.claude_sdk_revision='aa01a48')],
  ['changed native transcript','history-reuse-20261024/history-published-after.sha',s=>s+'changed'],
  ['receipt promotion','history-reuse-20261024/expected-review.json',json(v=>v.receipt_state='consumed')],
];
const root=fs.mkdtempSync(path.join(os.tmpdir(),'aikit-native-api-reuse-'));
for(const [index,[name,file,mutate]]of mutations.entries()){
  const dest=path.join(root,String(index));
  for(const [dir,names]of Object.entries(files)){
    fs.mkdirSync(path.join(dest,dir),{recursive:true});
    for(const name of names)fs.copyFileSync(path.join(original,dir,name),path.join(dest,dir,name));
  }
  const target=path.join(dest,file),before=fs.readFileSync(target,'utf8'),after=mutate(before);
  assert(before!==after,name+' applied');fs.writeFileSync(target,after);assert.throws(()=>verify(dest),undefined,name+' rejected');
}
console.log(JSON.stringify({negative_evidence_checks:mutations.length,retained_mutations:root}));
