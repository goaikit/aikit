// State-machine fault tests only; no native lease, caller or receipt qualification.
import {test} from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import {createHash} from 'node:crypto';
const base = await fs.readFile(new URL('./agent_tools_claude.js',import.meta.url),'utf8');
const helper = await fs.readFile(new URL('./agent_tools_messages_claude.js',import.meta.url),'utf8');
let sequence=0;
async function fixture() {
  const hooks=new Map(),actions=[],submissions=[],timers=[];
  let invocation, sid='native', ready=true, pending=false, busy=false;
  const config={applicationId:'fixture',installationId:'install',toolName:'review',description:'Fixture',
    schema:{type:'object'},instructions:'',sessionArgv:['session-handler'],argv:['tools'],messageArgv:['messages','fixed'],timeoutMs:1000};
  const source=base.replace('/* AIKIT_MESSAGE_HELPERS */',helper)
    .replace('/* AIKIT_MESSAGE_LIFECYCLE */',"on('session.end', async ($, e, next) => {messageStopped=true;return next(e);});")
    .replace('/* AIKIT_MESSAGE_START */','await startMessagePump($);')
    .replace('/* AIKIT_MESSAGE_BUSY */','messageActive=true;')
    .replace('/* AIKIT_MESSAGE_IDLE */','if(event.agentId===undefined && nativeTurnId===event.turnId) messageActive=false;')
    .replace('/* AIKIT_CONFIG */null',JSON.stringify(config));
  const module=await import('data:text/javascript,'+encodeURIComponent(source)+`#${++sequence}`);
  module.register((name,...args)=>{hooks.set(name,args.at(-1));return {catch(){}};});
  const content='Exact\n雪\u0000feedback';
  const api={session:{id:async()=>sid,cwd:async()=>'/workspace'},
    env:{set:async(_,value)=>{invocation=JSON.parse(value).native_context.invocation_id;}},
    process:{run:async(argv,opts)=>{
      if(argv[0]==='session-handler') return {exitCode:0,isStdoutTruncated:false,isStderrTruncated:false};
      assert.deepEqual(argv,config.messageArgv);
      const frame=JSON.parse(opts.stdin);actions.push(frame.action);
      assert.equal(frame.native_context.session_id,'native');
      const reference={installation_id:'install',installation_revision:'revision',native_session_id:'native',invocation_id:invocation,start_cursor:1};
      const reply={schema_version:1,reference,recorded_idle:!busy,recorded_ready:frame.action.kind==='recheck' && ready && !busy,reservation:null};
      if(frame.action.kind==='poll' && !pending){pending=true;reply.reservation={binding_id:'binding',target:reference,
        reservation_id:'attempt',request:{operation_id:'review',content},payload_sha256:createHash('sha256').update(content).digest('hex')};}
      return {exitCode:0,isStdoutTruncated:false,isStderrTruncated:false,stdout:JSON.stringify(reply)};
    }},
    prompt:{submit:async(value)=>{submissions.push(value);return {text:value.text,origin:{type:'plugin',plugin:'fixture'}};}},
    tool:{register:async()=>{}},ui:{invalidate(){}},clock:{every:(ms,fn)=>{assert.equal(ms,250);timers.push(fn);}}};
  await hooks.get('session.start')(api,{},async()=>({}));
  return {api,hooks,actions,submissions,content,poll:()=>timers[0](),setSid:value=>{sid=value;},
    setReady:value=>{ready=value;},setBusy:value=>{busy=value;}};
}
test('one exact native call retains raw return without consumption or resend',async()=>{
  const f=await fixture();await f.poll();await f.poll();
  assert.deepEqual(f.submissions,[{text:f.content}]);
  assert.deepEqual(f.actions.map(a=>a.kind),['poll','recheck','record_handoff','poll']);
  const report=f.actions[2];assert.equal(report.operation_id,'review');assert.equal(report.reservation_id,'attempt');
  assert.deepEqual(report.outcome,{kind:'returned',result:{text:f.content,origin:{type:'plugin',plugin:'fixture'}}});
});
test('uncertain native call is never retried, even after more ticks',async()=>{
  const f=await fixture();let calls=0;
  f.api.prompt.submit=async()=>{calls++;throw Error('lost native reply');};
  await f.poll();await f.poll();assert.equal(calls,1);
  assert.deepEqual(f.actions[2].outcome,{kind:'uncertain',reason:'native_submit_threw'});
});
test('lost journal response stops the pump and cannot resend',async()=>{
  const f=await fixture(),run=f.api.process.run;let reports=0;
  f.api.process.run=async(argv,opts)=>{
    if(JSON.parse(opts.stdin).action?.kind==='record_handoff'){reports++;throw Error('lost commit response');}
    return run(argv,opts);
  };
  await f.poll();await f.poll();assert.equal(reports,1);assert.equal(f.submissions.length,1);
});
test('busy native interval defers a retained reservation until idle',async()=>{
  const f=await fixture();f.setReady(false);await f.poll();assert.equal(f.submissions.length,0);
  await f.hooks.get('turn.start')(f.api,{turnId:'human'},async()=>({}));await f.poll();
  assert.equal(f.actions.filter(a=>a.kind==='poll').length,1);
  f.setReady(true);
  await f.hooks.get('turn.complete')(f.api,{turnId:'human',reason:'answer',isAborted:false,answer:'done'},async()=>({}));
  await f.poll();assert.equal(f.submissions.length,1);
});
test('a new main Turn during recheck prevents native I/O',async()=>{
  const f=await fixture(),run=f.api.process.run;
  f.api.process.run=async(argv,opts)=>{
    const result=await run(argv,opts);
    if(JSON.parse(opts.stdin).action?.kind==='recheck')
      await f.hooks.get('turn.start')(f.api,{turnId:'human'},async()=>({}));
    return result;
  };
  await f.poll();assert.equal(f.submissions.length,0);
});
test('session end during recheck prevents native I/O',async()=>{
  const f=await fixture(),run=f.api.process.run;
  f.api.process.run=async(argv,opts)=>{
    const result=await run(argv,opts);
    if(JSON.parse(opts.stdin).action?.kind==='recheck') await f.hooks.get('session.end')(f.api,{},async()=>({}));
    return result;
  };
  await f.poll();await f.poll();assert.equal(f.submissions.length,0);
});
test('changed session, corrupt digest, foreign ticket and truncated RPC fail closed',async()=>{
  for(const fault of ['session','digest','target','truncated']){
    const f=await fixture(),run=f.api.process.run;
    if(fault==='session') f.setSid('replacement');
    else f.api.process.run=async(argv,opts)=>{
      const result=await run(argv,opts);
      if(JSON.parse(opts.stdin).action?.kind==='poll'){
        const reply=JSON.parse(result.stdout);
        if(fault==='digest') reply.reservation.payload_sha256='bad';
        if(fault==='target') reply.reservation.target={...reply.reference,native_session_id:'foreign'};
        if(fault==='truncated') result.isStdoutTruncated=true;
        result.stdout=JSON.stringify(reply);
      }
      return result;
    };
    await f.poll();await f.poll();assert.equal(f.submissions.length,0,fault);
  }
});
test('overlapping timer ticks cannot create a second submission',async()=>{
  const f=await fixture();let release,entered;
  const ready=new Promise(resolve=>{entered=resolve;});
  const pending=new Promise(resolve=>{release=resolve;});
  f.api.prompt.submit=async(value)=>{f.submissions.push(value);entered();await pending;return {text:value.text};};
  const first=f.poll();await ready;await f.poll();assert.equal(f.submissions.length,1);release();await first;
});
