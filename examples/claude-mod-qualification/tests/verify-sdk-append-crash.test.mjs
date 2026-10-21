// Verifier sensitivity checks, not additional native kill scenarios.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import {fileURLToPath} from 'node:url';
import {spawnSync} from 'node:child_process';
import test from 'node:test';
assert(process.argv[2], 'native SDK crash capture required');
const source=path.resolve(process.argv[2]);
const verifier=fileURLToPath(new URL('../verify-sdk-append-crash.mjs',import.meta.url));
const names=['setup.json','stage.json','checkpoint-committed.json','before-recovery-sqlite.json','after-recovery-sqlite.json',
  'recovered-receipt.json','repeat-stage.json','message-status.json','killed-state.json','after-kill-transcript.jsonl',
  'after-resume-transcript.jsonl','requests.jsonl','resume-request-presence.json','events.json',
  'historical-binding-status.json','networks.json','generated-module.js','binary-identities.json'];
const edit=(files,name,change)=>{const value=JSON.parse(files[name]);change(value);files[name]=JSON.stringify(value);};
const cases=[
  ['original SDK crash capture passes',null],
  ['missing cold checkpoint fails',files=>edit(files,'before-recovery-sqlite.json',v=>{v.append_checkpoints=[];})],
  ['replacement invocation on old operation fails',files=>edit(files,'after-recovery-sqlite.json',v=>{v.outgoing[0].target.invocation_id='replacement';})],
  ['invented recovered acknowledgement fails',files=>edit(files,'recovered-receipt.json',v=>{v.acknowledgement={invented:true};})],
  ['kill after RPC deadline fails',files=>{
    const checkpoint=JSON.parse(files['checkpoint-committed.json']).reply.append_checkpoint;
    edit(files,'killed-state.json',v=>{v.FinishedAt=new Date(checkpoint.stored_at_unix_ms+31000).toISOString();});
  }],
  ['duplicate enqueue after resume fails',files=>{
    const rows=files['after-resume-transcript.jsonl'].trim().split('\n').map(JSON.parse);
    const review=rows.find(row=>row.type==='queue-operation'&&row.operation==='enqueue'
      && row.content==='SDK_APPEND_CLIENT_REVIEW: exact 雪\nfeedback');
    assert(review);rows.push(structuredClone(review));
    files['after-resume-transcript.jsonl']=rows.map(JSON.stringify).join('\n');
  }],
];
for(const [name,mutate] of cases)test(name,()=>{
  const directory=fs.mkdtempSync(path.join(os.tmpdir(),'aikit-sdk-append-crash-'));
  try{
    const files=Object.fromEntries(names.map(n=>[n,fs.readFileSync(path.join(source,n),'utf8')]));
    mutate?.(files);
    for(const [n,bytes] of Object.entries(files))fs.writeFileSync(path.join(directory,n),bytes);
    const result=spawnSync(process.execPath,[verifier,directory],{encoding:'utf8',timeout:10000});
    assert.ifError(result.error);assert.equal(result.signal,null);
    if(mutate){assert.equal(result.status,1,result.stdout+result.stderr);assert.match(result.stderr,/AssertionError/);}
    else{assert.equal(result.status,0,result.stderr);assert.equal(JSON.parse(result.stdout).production_acknowledgement_qualified,false);}
  }finally{
    assert.equal(path.dirname(path.resolve(directory)),path.resolve(os.tmpdir()));
    assert(path.basename(directory).startsWith('aikit-sdk-append-crash-'));
    fs.rmSync(directory,{recursive:true});
  }
});
