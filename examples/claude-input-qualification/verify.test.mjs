import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import {verify} from './verify.mjs';
const original=process.argv[2],options={early:process.argv[3]==='--early'};verify(original,options);
const mutateJson=(root,file,change)=>{
  const name=path.join(root,file),value=JSON.parse(fs.readFileSync(name));change(value);fs.writeFileSync(name,JSON.stringify(value));
};
const cases={
  'missing core trace':root=>mutateJson(root,'input-events.json',events=>{events.find(row=>row.kind==='after').value.trace=[];}),
  'duplicate core trace':root=>mutateJson(root,'input-events.json',events=>{const trace=events.find(row=>row.kind==='after').value.trace;trace.push(structuredClone(trace.find(row=>row.plugin==='engine')));}),
  'foreign core Turn':root=>mutateJson(root,'input-events.json',events=>{events.find(row=>row.kind==='after').value.trace.find(row=>row.plugin==='engine').received.turnId='foreign';}),
  'missing post snapshot row':root=>mutateJson(root,'input-events.json',events=>{events.filter(row=>row.kind==='after')[1].value.messages=[];}),
  'invented append identity':root=>mutateJson(root,'late-append.json',value=>{value.result.uuid='00000000-0000-4000-8000-000000000000';}),
  'changed settings':root=>mutateJson(root,'settings-after.json',value=>{value.prependPlugins=[];}),
  'late row really sent':root=>{
    const file=path.join(root,'requests.jsonl'),rows=fs.readFileSync(file,'utf8').trim().split('\n').map(JSON.parse);
    rows.filter(row=>row.body?.tools?.some(tool=>tool.name==='Read'))[1].body.messages.push({role:'user',content:'AIKIT_INPUT_LATE_雪'});
    fs.writeFileSync(file,rows.map(row=>JSON.stringify(row)).join('\n')+'\n');
  },
  'foreign native session':root=>mutateJson(root,'input-events.json',events=>{events.find(row=>row.kind==='before').value.session_id='foreign';}),
  'wrong pre snapshot':root=>mutateJson(root,'input-events.json',events=>{
    const value=events.filter(row=>row.kind==='before')[1].value;
    if(options.early)value.messages=value.messages.filter(row=>!JSON.stringify(row).includes('AIKIT_INPUT_LATE_雪'));
    else value.messages.push({role:'user',content:'AIKIT_INPUT_LATE_雪'});
  }),
  'wrong native tier':root=>{
    const file=path.join(root,'native-debug.log'),name=options.early?'input-shortcut':'input-observer';
    const pattern=new RegExp(`(hooks module ${name}@input-qualification loaded \\(worker, environment [0-9]+, tier )prepend\\)`);
    fs.writeFileSync(file,fs.readFileSync(file,'utf8').replace(pattern,'$1user)'));
  },
};
for(const [name,change] of Object.entries(cases)){
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'aikit-input-evidence-'));
  fs.cpSync(original,root,{recursive:true,filter:source=>path.basename(source)!=='latest'});
  change(root);assert.throws(()=>verify(root,options),assert.AssertionError,name);
}
console.log(JSON.stringify({passed:true,altered_evidence_checks:Object.keys(cases).length,native_trials:0}));
