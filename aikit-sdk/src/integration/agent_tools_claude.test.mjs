// Behavioral engine stub only: not native client qualification.
import {test} from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
const source = await fs.readFile(new URL('./agent_tools_claude.js',import.meta.url),'utf8');
let sequence = 0;
function nativeNext(event, delegate=async()=>({}), entries) {
  const next=async()=>{
    const result=await delegate();
    next.trace=entries??[{plugin:'engine',tier:'core',event:'turn.complete',outcome:'returned',received:structuredClone(event),returned:result}];
    return result;
  };
  next.origin={plugin:'engine',tier:'core'};next.trace=[];
  return next;
}
async function fixture() {
  const hooks = new Map(), frames = [], environment = new Map();
  const config = {applicationId:'fixture',installationId:'fixture-install',toolName:'review',description:'Fixture',
    schema:{type:'object'},instructions:'',sessionArgv:['fixed-handler','fixed-arg'],argv:['tool-handler'],timeoutMs:1000};
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
    const event={turnId:'native-turn',reason,isAborted:reason==='aborted',answer:'Exact 雪\n'};
    const result=await f.hooks.get('turn.complete')(f.api,event,nativeNext(event,async()=>{
      assert.equal(f.frames.length,2); return expected;
    }));
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
    await assert.rejects(f.hooks.get('turn.complete')(f.api,event,nativeNext(event)));
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
  await assert.rejects(f.hooks.get('turn.complete')(f.api,event,nativeNext(event,async()=>{throw Error('downstream');})));
  assert.equal(f.frames.length,2);
  let calls=0;
  f.api.process.run=async()=>{calls++;return {exitCode:2,isStdoutTruncated:false,isStderrTruncated:false};};
  await assert.rejects(f.hooks.get('turn.complete')(f.api,event,nativeNext(event)));
  assert.equal(calls,1);
});
test('owned tool waits for an asynchronous native Turn startup without retrying',async()=>{
  const f=await fixture();
  assert.match((await f.hooks.get('tool.call')(f.api,{request:{}},{origin:{plugin:'engine',tier:'core'}})).deny,/startup/);
  let release,entered,toolCalls=0;
  const ready=new Promise(resolve=>{entered=resolve;});
  const pending=new Promise(resolve=>{release=resolve;});
  f.api.process.run=async(argv)=>{
    if(argv[0]==='fixed-handler') {entered();await pending;}
    else {assert.equal(argv[0],'tool-handler');toolCalls++;}
    return {exitCode:0,isStdoutTruncated:false,isStderrTruncated:false,stdout:'{"ok":true}'};
  };
  const start=f.hooks.get('turn.start')(f.api,{turnId:'main'},async()=>({}));
  await ready;
  const result=f.hooks.get('tool.call')(f.api,{request:{}},{origin:{plugin:'engine',tier:'core'}});
  await Promise.resolve();assert.equal(toolCalls,0);
  release();await start;
  assert.deepEqual(await result,{result:'{"ok":true}'});assert.equal(toolCalls,1);
});
test('missing or plugin-raised tool origin cannot invoke the application handler',async()=>{
  const f=await fixture();
  await f.hooks.get('turn.start')(f.api,{turnId:'main'},async()=>({}));
  let calls=0;
  f.api.process.run=async()=>{calls++;return {exitCode:0,isStdoutTruncated:false,isStderrTruncated:false,stdout:'{"ok":true}'};};
  for(const next of [undefined,{}, {origin:{plugin:'peer',tier:'user'}},
    {origin:{plugin:'engine',tier:'user'}},{origin:{plugin:'peer',tier:'core'}}]) {
    const result=await f.hooks.get('tool.call')(f.api,{request:{}},next);
    assert.match(result.deny,/native engine/);
  }
  assert.equal(calls,0);
});

test('completion without a matching settled engine trace never emits an outcome',async()=>{
  const event={turnId:'main',reason:'answer',isAborted:false,answer:'answer'};
  const core={plugin:'engine',tier:'core',event:'turn.complete',outcome:'returned',received:event,returned:{text:'answer'}};
  const cases=[[],[{...core,plugin:'peer'}],[{...core,tier:'user'}],
    [{...core,event:'turn.start'}],[{...core,outcome:'skipped'}],[{...core,outcome:'rejected'}],
    [{...core,received:{...event,reason:'aborted',isAborted:true}}],
    [{...core,received:{...event,answer:'rewritten'}}],
    [{...core,received:{...event,turnId:'foreign'}}],
    [{...core,received:{...event,agentId:'child'}}],[core,core]];
  for(const entries of cases){
    const f=await fixture();await f.hooks.get('turn.start')(f.api,{turnId:'main'},async()=>({}));
    await assert.rejects(f.hooks.get('turn.complete')(f.api,event,nativeNext(event,undefined,entries)));
    assert.equal(f.frames.length,2,'no completion frame for a missing or contradictory engine run');
  }
});

test('foreign completion dispatch is refused before delegation or handler I/O',async()=>{
  const event={turnId:'main',reason:'answer',isAborted:false,answer:'answer'};
  for(const origin of [undefined,{plugin:'peer',tier:'user'},{plugin:'engine',tier:'user'}]){
    const f=await fixture();await f.hooks.get('turn.start')(f.api,{turnId:'main'},async()=>({}));
    let delegated=0;const next=nativeNext(event,async()=>{delegated++;return {};});next.origin=origin;
    await assert.rejects(f.hooks.get('turn.complete')(f.api,event,next));
    assert.equal(delegated,0);assert.equal(f.frames.length,2);
  }
});
