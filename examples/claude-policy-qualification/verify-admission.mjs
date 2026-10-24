import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
const root=process.argv[2];assert(path.isAbsolute(root));
const read=name=>fs.readFileSync(path.join(root,name));
const json=name=>JSON.parse(read(name)),setup=json('setup.json');
assert(['healthy','catch','catch-fails'].includes(setup.mode));
assert.deepEqual(JSON.parse(fs.readFileSync('/home/node/.claude/settings.json')),setup.settings);
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
assert.equal(hash(fs.readFileSync('/usr/local/bin/claude')),setup.native_sha256);
for(const [name,expected] of Object.entries(setup.source_sha256))
  assert.equal(hash(fs.readFileSync('/home/node/workspace/'+name+'/hooks/register.js')),expected);
const log=read('native-debug.log').toString();
assert(log.includes('hooks module policy-guard@aikit-policy-probe loaded (worker, environment 1, tier prepend)'));
assert(read('native-terminal.log').toString().includes('COMMAND_EXIT_CODE="0"'));
const rows=json('admission.json');assert(rows.length>=2&&rows.length<=16);
const competing=rows.filter(row=>row.event.name==='policy-competing');assert(competing.length>=1);
for(const row of competing) {
  assert.deepEqual(row.origin,{plugin:'engine',tier:'core'});assert.equal(row.event.tier,'user');
  assert(row.event.uses.events.includes('classic.Stop'));assert.equal(row.risky,true);
}
const guard=json('guard-started.json'),benign=json('benign-started.json');assert.equal(guard.session_id,benign.session_id);
assert(log.includes('hooks module policy-benign@aikit-policy-probe loaded'));
const admitted=fs.existsSync(path.join(root,'competing-started.json'));
assert.equal(admitted,setup.mode==='catch-fails');
if(admitted) {
  assert.equal(json('competing-started.json').session_id,guard.session_id);
  assert(log.includes('hooks module policy-competing@aikit-policy-probe loaded'));
  assert(log.includes('its .catch threw Error: Declared fallback fault'));
}else {
  const reason=setup.mode==='healthy'?'AIKit probe: competing completion handler':'AIKit probe: constant fallback refusal';
  assert(log.includes('refused by policy-guard: '+reason));
  // "loaded" describes worker construction before admission, not chain membership.
  const startup=log.split('\n').filter(line=>line.includes('session.start settled')
    &&line.includes('policy-competing@aikit-policy-probe'));
  assert.equal(startup.length,0);
  if(setup.mode==='catch')assert(log.includes('plugin.register; its .catch answered'));
}
console.log(JSON.stringify({passed:true,scope:'ordinary Linux Claude module admission, no model prompt',
  mode:setup.mode,guard_tier:'prepend',competing_admitted:admitted,benign_admitted:true,
  native_sha256:setup.native_sha256,debug_sha256:hash(read('native-debug.log')),
  mandatory_policy_qualified:false,windows_qualified:false}));
