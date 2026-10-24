import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import {verify} from '../verify-current-stop-failure.mjs';

const original=path.resolve(process.argv[2]);verify(original);
const mutations=[
  ['ownership','baseline-root/evidence/managed-ownership.json',s=>s.replace('"uid":0','"uid":1000')],
  ['guard','missing-root/evidence/native-debug.log',s=>s.replaceAll('seated outermost','absent')],
  ['core','missing-root/evidence/events.json',s=>s.replaceAll('"plugin":"engine"','"plugin":"observer"')],
  ['completion','timeout-root/evidence/events.json',s=>s.replaceAll('"isAborted":false','"isAborted":true')],
  ['block propagation','baseline-root/evidence/requests.jsonl',s=>s.replaceAll('AIKIT_STOP_CHAIN_FIRST_BLOCK','MISSING_BLOCK')],
  ['timeout result','timeout-root/evidence/timeout-command.jsonl',s=>s.replace('"phase":"started"','"phase":"late_block"')],
  ['configuration','missing-root/evidence/actual-managed-settings.json',s=>s.replace('"onFailure":"block"','"onFailure":"continue"')],
  ['network','baseline-root/container.json',s=>s.replace('"NetworkMode": "none"','"NetworkMode": "bridge"')],
];
// Keep these bounded mutation copies for inspection; no native reruns or deletion.
const root=fs.mkdtempSync(path.join(os.tmpdir(),'aikit-stop308-verifier-'));
for(const [index,[name,file,mutate]] of mutations.entries()) {
  const directory=path.join(root,String(index));
  for(const mode of ['baseline','missing','timeout']) {
    const sub=mode+'-root';fs.mkdirSync(path.join(directory,sub),{recursive:true});
    fs.cpSync(path.join(original,sub,'evidence'),path.join(directory,sub,'evidence'),{recursive:true});
    fs.copyFileSync(path.join(original,sub,'container.json'),path.join(directory,sub,'container.json'));
  }
  const target=path.join(directory,file),before=fs.readFileSync(target,'utf8'),after=mutate(before);
  assert.notEqual(after,before,name+' mutation applied');fs.writeFileSync(target,after);
  assert.throws(()=>verify(directory),undefined,name+' rejected');
}
console.log(JSON.stringify({negative_evidence_checks:mutations.length,retained_mutations:root}));
