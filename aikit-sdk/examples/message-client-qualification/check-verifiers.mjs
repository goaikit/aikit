// Offline evidence mutation checks, not additional native scenarios.
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
const root=path.resolve(process.argv[2]),here=path.dirname(fileURLToPath(import.meta.url));
const cases=[
  ['missing-outgoing-payload','success','verify.mjs',dir=>{
    const staged=JSON.parse(fs.readFileSync(path.join(dir,'staged.json'))),file=path.join(dir,'requests.jsonl');
    // The payload is JSON-escaped in the outer JSONL; replace it recursively.
    let replaced=0;
    const replace=x=>{if(x.includes(staged.receipt.request.content))replaced++;return x.split(staged.receipt.request.content).join('OMITTED');};
    const walk=x=>typeof x==='string'?replace(x):
      Array.isArray(x)?x.map(walk):x && typeof x==='object'?Object.fromEntries(Object.entries(x).map(([k,v])=>[k,walk(v)])):x;
    fs.writeFileSync(file,fs.readFileSync(file,'utf8').trimEnd().split('\n').map(line=>JSON.stringify(walk(JSON.parse(line)))).join('\n')+'\n');
    assert(replaced>0,'must remove an actual outgoing payload');
  }],
  ['foreign-native-session','success','verify.mjs',dir=>{
    const file=path.join(dir,'requests.jsonl'),rows=fs.readFileSync(file,'utf8').trimEnd().split('\n').map(JSON.parse);
    const body=rows.at(-1).body,metadata=JSON.parse(body.metadata.user_id);metadata.session_id='foreign';body.metadata.user_id=JSON.stringify(metadata);
    fs.writeFileSync(file,rows.map(row=>JSON.stringify(row)).join('\n')+'\n');
  }],
  ['changed-raw-return','success','verify.mjs',dir=>{
    for(const name of ['live-status.json','after-exit-status.json']){
      const file=path.join(dir,name),x=JSON.parse(fs.readFileSync(file));x.unqualified_handoff.outcome.result.text='changed';fs.writeFileSync(file,JSON.stringify(x));
    }
  }],
  ['invented-acknowledgement','success','verify.mjs',dir=>{
    for(const name of ['live-status.json','after-exit-status.json']){
      const file=path.join(dir,name),x=JSON.parse(fs.readFileSync(file));x.receipt.state='acknowledged';fs.writeFileSync(file,JSON.stringify(x));
    }
  }],
  ['resume-without-new-invocation','resumed-post-receipt-crash','verify-resume.mjs',dir=>{
    const files=['resumed-live-events.json','after-crash-events.json'];
    for(const name of files){const file=path.join(dir,name),x=JSON.parse(fs.readFileSync(file));
      const sessions=x.records.filter(r=>r.request.event==='session_started');sessions[1].request.invocation_id=sessions[0].request.invocation_id;fs.writeFileSync(file,JSON.stringify(x));}
  }],
  ['crash-changed-attempt','resumed-post-receipt-crash','verify-resume.mjs',dir=>{
    const file=path.join(dir,'after-crash-status.json'),x=JSON.parse(fs.readFileSync(file));x.receipt.reservation_id='new-attempt';fs.writeFileSync(file,JSON.stringify(x));
  }],
];
const rejected=[];
for(const [name,base,verifier,mutate] of cases){
  const source=path.join(root,base),directory=fs.mkdtempSync(path.join(root,`${name}-`));
  for(const entry of fs.readdirSync(source,{withFileTypes:true}))if(entry.isFile())fs.copyFileSync(path.join(source,entry.name),path.join(directory,entry.name));
  const baseline=spawnSync(process.execPath,[path.join(here,verifier),directory],{encoding:'utf8'});
  assert.equal(baseline.status,0,`baseline ${name}: ${baseline.stderr}`);
  mutate(directory);
  const result=spawnSync(process.execPath,[path.join(here,verifier),directory],{encoding:'utf8'});
  fs.writeFileSync(path.join(directory,'verifier-stderr.txt'),result.stderr);
  assert.notEqual(result.status,0,`mutated evidence incorrectly passed: ${name}`);rejected.push(name);
}
console.log(JSON.stringify({passed:true,scope:'offline retained-evidence mutations, not native fault injections',rejected},null,2));
