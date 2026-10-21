// Sensitivity checks on the retained native SDK capture, not extra native runs.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import {fileURLToPath} from 'node:url';
import {spawnSync} from 'node:child_process';
import test from 'node:test';
assert(process.argv[2], 'native SDK capture required');
const source=path.resolve(process.argv[2]);
const verifier=fileURLToPath(new URL('../verify-sdk-append.mjs',import.meta.url));
const names=['setup.json','stage.json','message-status.json','before-exit-status.json','repeat-stage.json',
  'transcript.jsonl','requests.jsonl','events.json','generated-module.js','binary-identities.json','networks.json','sqlite-inspection.json'];
const editStatus=(files,edit)=>{
  const value=JSON.parse(files['message-status.json']);edit(value);
  files['message-status.json']=JSON.stringify(value);
  files['before-exit-status.json']=files['message-status.json'];
};
const cases=[
  ['original capture passes',null],
  ['foreign reservation fails',files=>editStatus(files,s=>{s.unqualified_append.reservation.reservation_id='foreign';})],
  ['changed native row identity fails',files=>editStatus(files,s=>{s.unqualified_append.intent.native_row_id='00000000-0000-0000-0000-000000000000';})],
  ['invented acknowledgement fails',files=>editStatus(files,s=>{s.receipt.acknowledgement={invented:true};})],
  ['missing wire payload fails',files=>{files['requests.jsonl']=files['requests.jsonl'].replaceAll('SDK_APPEND_CLIENT_REVIEW','REMOVED');}],
];
for(const [name,mutate] of cases)test(name,()=>{
  const directory=fs.mkdtempSync(path.join(os.tmpdir(),'aikit-sdk-append-verifier-'));
  try{
    const files=Object.fromEntries(names.map(n=>[n,fs.readFileSync(path.join(source,n),'utf8')]));
    mutate?.(files);
    for(const [n,bytes]of Object.entries(files))fs.writeFileSync(path.join(directory,n),bytes);
    const result=spawnSync(process.execPath,[verifier,directory],{encoding:'utf8',timeout:10000});
    assert.ifError(result.error);assert.equal(result.signal,null);
    if(mutate){assert.equal(result.status,1,result.stdout+result.stderr);assert.match(result.stderr,/AssertionError/);}
    else{assert.equal(result.status,0,result.stderr);assert.equal(JSON.parse(result.stdout).production_acknowledgement_qualified,false);}
  }finally{
    assert.equal(path.dirname(path.resolve(directory)),path.resolve(os.tmpdir()));
    assert(path.basename(directory).startsWith('aikit-sdk-append-verifier-'));
    fs.rmSync(directory,{recursive:true});
  }
});
