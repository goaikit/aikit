import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import {verify} from '../verify-current-mod-failure.mjs';
const original=path.resolve(process.argv[2]);verify(original);
const mutations=[
  ['worker recovery','busy/evidence/native-debug.log',s=>s.replaceAll('.catch asked in the replacement','missing replacement handler')],
  ['core provenance','busy/evidence/events.json',s=>s.replaceAll('"plugin":"engine"','"plugin":"observer"')],
  ['completion','pending/evidence/events.json',s=>s.replaceAll('"isAborted":false','"isAborted":true')],
  ['Block propagation','busy/evidence/requests.jsonl',s=>s.replaceAll('Completion validation unavailable: restore the local handler and retry.','MISSING_BLOCK')],
  ['session','busy/evidence/guard-calls.json',s=>s.replaceAll(JSON.parse(s)[0].event.session_id,'foreign')],
  ['exact fallback','pending/evidence/case.json',s=>s.replace('restore the local handler','allow without the handler')],
  ['native budget','pending/evidence/guard-calls.json',s=>s.replaceAll('"ms":10000','"ms":20000')],
  ['network','busy/container.json',s=>s.replace('"NetworkMode": "none"','"NetworkMode": "bridge"')],
];
const root=fs.mkdtempSync(path.join(os.tmpdir(),'aikit-mod308-verifier-'));
for(const [index,[name,file,mutate]] of mutations.entries()) {
  const directory=path.join(root,String(index));
  for(const mode of ['pending','busy']) {
    const src=path.join(original,mode),dest=path.join(directory,mode);fs.mkdirSync(dest,{recursive:true});
    for(const name of ['evidence','guard'])fs.cpSync(path.join(src,name),path.join(dest,name),{recursive:true});
    for(const name of ['container.json','completion-source.js'])fs.copyFileSync(path.join(src,name),path.join(dest,name));
  }
  const target=path.join(directory,file),before=fs.readFileSync(target,'utf8'),after=mutate(before);
  assert.notEqual(after,before,name+' mutation applied');fs.writeFileSync(target,after);
  assert.throws(()=>verify(directory),undefined,name+' rejected');
}
console.log(JSON.stringify({negative_evidence_checks:mutations.length,retained_mutations:root}));
