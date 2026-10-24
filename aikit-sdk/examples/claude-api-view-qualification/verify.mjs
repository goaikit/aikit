import fs from 'node:fs';
import assert from 'node:assert/strict';
assert.equal(process.argv.length,7);
const [control,controlWitness,returned,witness,mutator]=process.argv.slice(2).map(file=>JSON.parse(fs.readFileSync(file)));
const core=value=>{
  const entries=value.trace.filter(entry=>entry.plugin==='engine'&&entry.tier==='core'&&entry.event==='session.messages');
  assert.equal(entries.length,1);assert(['returned','passed'].includes(entries[0].outcome));
  assert.deepEqual(entries[0].received,{as:'api'});return entries[0].returned;
};
assert.deepEqual(control,[]);assert.deepEqual(controlWitness.result,{value:control});
assert.deepEqual(core(controlWitness),{value:control});
assert.equal(witness.origin.plugin,'aikit-types-probe');assert.equal(witness.origin.tier,'user');
assert.deepEqual(core(witness),{value:[]});assert.deepEqual(core(mutator),{value:[]});
assert.deepEqual(mutator.result,{value:[]});
assert.deepEqual(returned,[{role:'user',content:[{type:'text',text:'AIKIT_FORGED_API_VIEW_雪'}]}]);
assert.deepEqual(witness.result,{value:returned});assert.deepEqual(mutator.altered,{value:returned});
assert(witness.trace.some(entry=>entry.plugin==='api-view-mutator'&&entry.tier==='append'));
console.log(JSON.stringify({passed:true,returned_view_changed:true,core_view_changed:false,model_request_claim:false}));
