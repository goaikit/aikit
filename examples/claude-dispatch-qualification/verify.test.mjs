// Sensitivity checks on copies; not additional native sessions.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
const original=process.argv[2];assert(path.isAbsolute(original));
const verifier=fileURLToPath(new URL('./verify.mjs',import.meta.url));
const run=(dir,analysis=true)=>spawnSync(process.execPath,[verifier,dir,...analysis?['--main-loop-analysis']:[]],{encoding:'utf8'});
const baseline=run(original);assert.equal(baseline.status,0,baseline.stderr);
const originalHypothesis=run(original,false);assert.notEqual(originalHypothesis.status,0);
assert.match(originalHypothesis.stderr,/original no-HTTP-send hypothesis/);
const json=change=>text=>{const value=JSON.parse(text);change(value);return JSON.stringify(value);};
const after=events=>events.filter(row=>row.kind==='after');
const mutations=[
  ['foreign session','dispatch-events.json',json(v=>{v.find(r=>r.kind==='before').value.session_id='foreign';})],
  ['lost control core','dispatch-events.json',json(v=>{after(v)[0].value.trace=after(v)[0].value.trace.filter(r=>r.plugin!=='engine');})],
  ['invented shortcut core','dispatch-events.json',json(v=>{after(v)[1].value.trace.push(structuredClone(after(v)[0].value.trace.find(r=>r.plugin==='engine')));})],
  ['changed core Turn','dispatch-events.json',json(v=>{after(v)[0].value.trace.find(r=>r.plugin==='engine').received.turnId='foreign';})],
  ['missing API-view marker','dispatch-events.json',t=>t.replaceAll('AIKIT_DISPATCH_WITHOUT_ENGINE','removed')],
  ['changed synthetic usage','shortcut.json',json(v=>{v.result.usage.input_tokens=0;})],
  ['invented core input witness','dispatch-events.json',json(v=>{after(v)[0].value.trace.find(r=>r.plugin==='engine').received.messages=[];})],
  ['main-loop marker leak','requests.jsonl',t=>t.replaceAll('AIKIT_DISPATCH_CONTROL','AIKIT_DISPATCH_WITHOUT_ENGINE')],
  ['unclassified background input','requests.jsonl',t=>t.replaceAll('Write the title in the predominant language','unknown input')],
  ['wrong native tier','native-debug.log',t=>t.replaceAll('tier prepend)','tier user)')],
  ['changed effective settings','settings-after.json',json(v=>{v.prependPlugins=[];})],
  ['foreign transcript','native-transcript.jsonl',t=>t.split('\n').map(line=>{if(!line)return line;const v=JSON.parse(line);if(v.sessionId)v.sessionId='foreign';return JSON.stringify(v);}).join('\n')],
];
for(const [label,file,change]of mutations){
  const temporary=fs.mkdtempSync(path.join(os.tmpdir(),'aikit-dispatch-evidence-'));
  try{
    for(const entry of fs.readdirSync(original,{withFileTypes:true}))if(entry.isFile())
      fs.copyFileSync(path.join(original,entry.name),path.join(temporary,entry.name));
    const target=path.join(temporary,file),before=fs.readFileSync(target,'utf8'),changed=change(before);
    assert.notEqual(before,changed,label);fs.writeFileSync(target,changed);
    const result=run(temporary);assert.notEqual(result.status,0,label);assert.match(result.stderr,/AssertionError/,label);
  }finally{fs.rmSync(temporary,{recursive:true,force:true});}
}
console.log(JSON.stringify({separate_analysis_passed:true,original_hypothesis_rejected:true,
  negative_evidence_checks:mutations.length,additional_native_runs:0,production_ready:false}));
