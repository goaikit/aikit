// Behavioral engine stub only: not native client qualification.
import {test} from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
const source = await fs.readFile(new URL('./agent_tools_claude.js',import.meta.url),'utf8');
let sequence = 0;
async function fixture() {
  const hooks = new Map(), frames = [], environment = new Map();
  const config = {applicationId:'fixture',installationId:'fixture-install',toolName:'review',description:'Fixture',
    schema:{type:'object'},instructions:'',sessionArgv:['fixed-handler','fixed-arg'],timeoutMs:1000};
  const module = await import('data:text/javascript,'+encodeURIComponent(source.replace('/* AIKIT_CONFIG */null',JSON.stringify(config)))+`#${++sequence}`);
  module.register((name, ...args) => { hooks.set(name,args.at(-1)); return {catch(){}}; });
  const api = {session:{id:async()=> 'native-session',cwd:async()=>'/workspace'},
    env:{set:async(key,value)=>environment.set(key,JSON.parse(value))},
    process:{run:async(argv,options)=> { assert.deepEqual(argv,config.sessionArgv); frames.push(JSON.parse(options.stdin));
      return {exitCode:0,isStdoutTruncated:false,isStderrTruncated:false}; }},
    tool:{register:async()=>{}},ui:{invalidate(){}}};
  await hooks.get('session.start')(api,{},async()=>({}));
  return {hooks,frames,environment,api};
}
test('native outcome follows downstream completion and retains every reason without accepting it',async()=>{
  for (const reason of ['answer','aborted','refusal','error']) {
    const f=await fixture();
    await f.hooks.get('turn.start')(f.api,{turnId:'native-turn'},async()=>({}));
    assert.equal(f.frames[1].hook_event_name,'AIKitNativeTurnStarted');
    assert.equal([...f.environment.values()][0].native_turn_id,'native-turn');
    const expected={text:'unchanged downstream result'};
    const result=await f.hooks.get('turn.complete')(f.api,{turnId:'native-turn',reason,isAborted:reason==='aborted',answer:'Exact 雪\n'},async()=>{
      assert.equal(f.frames.length,2); return expected;
    });
    assert.equal(result,expected);
    assert.equal(f.frames.length,3);
    assert.deepEqual(f.frames[2].native_turn,{id:'native-turn',outcome:{reason,is_aborted:reason==='aborted'}});
    assert.equal(f.frames[2].last_assistant_message,'Exact 雪\n');
  }
});
test('unknown, mismatched and subagent outcomes cannot manufacture a main Turn result',async()=>{
  const f=await fixture();
  await f.hooks.get('turn.start')(f.api,{turnId:'main'},async()=>({}));
  for (const event of [{turnId:'old',reason:'answer',isAborted:false,answer:'old'},
    {turnId:'main',reason:'unknown',isAborted:false,answer:'bad'}]) {
    await assert.rejects(f.hooks.get('turn.complete')(f.api,event,async()=>({})));
  }
  await f.hooks.get('turn.start')(f.api,{turnId:'child',agentId:'subagent'},async()=>({}));
  await f.hooks.get('turn.complete')(f.api,{turnId:'child',agentId:'subagent'},async()=>({}));
  assert.equal(f.frames.length,2);
  assert.equal([...f.environment.values()][0].native_turn_id,'main');
});
test('downstream failure emits no outcome and process failure is attempted only once',async()=>{
  const f=await fixture();
  await f.hooks.get('turn.start')(f.api,{turnId:'main'},async()=>({}));
  const event={turnId:'main',reason:'answer',isAborted:false,answer:'answer'};
  await assert.rejects(f.hooks.get('turn.complete')(f.api,event,async()=>{throw Error('downstream');}));
  assert.equal(f.frames.length,2);
  let calls=0;
  f.api.process.run=async()=>{calls++;return {exitCode:2,isStdoutTruncated:false,isStderrTruncated:false};};
  await assert.rejects(f.hooks.get('turn.complete')(f.api,event,async()=>({})));
  assert.equal(calls,1);
});
