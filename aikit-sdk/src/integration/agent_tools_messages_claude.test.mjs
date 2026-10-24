// State-machine fault tests only; no native lease, caller or receipt qualification.
import {test} from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import {createHash} from 'node:crypto';
const base = await fs.readFile(new URL('./agent_tools_claude.js',import.meta.url),'utf8');
const helper = await fs.readFile(new URL('./agent_tools_messages_claude.js',import.meta.url),'utf8');
let sequence=0;
function traced(next,run) {
  return Object.defineProperty(run,'trace',{get:()=>next.trace});
}
async function fixture({version='2.1.296',pluginName='fixture-plugin',nativeLabel=pluginName}={}) {
  const hooks=new Map(),actions=[],submissions=[],timers=[];
  let invocation, sid='native', ready=true, pending=false, busy=false, checkpoint;
  const config={applicationId:'fixture',installationId:'install',pluginName,toolName:'review',description:'Fixture',
    schema:{type:'object'},instructions:'',sessionArgv:['session-handler'],argv:['tools'],messageArgv:['messages','fixed'],timeoutMs:1000};
  const source=base.replace('/* AIKIT_MESSAGE_HELPERS */',helper)
    .replace('/* AIKIT_MESSAGE_LIFECYCLE */','registerMessageLifecycle(on);')
    .replace('/* AIKIT_MESSAGE_START */','await startMessagePump($);')
    .replace('/* AIKIT_MESSAGE_BUSY */','messageActive=true;')
    .replace('/* AIKIT_MESSAGE_IDLE */','if(event.agentId===undefined && nativeTurnId===event.turnId) messageActive=false;')
    .replace('/* AIKIT_CONFIG */null',JSON.stringify(config));
  const module=await import('data:text/javascript,'+encodeURIComponent(source)+`#${++sequence}`);
  module.register((name,...args)=>{hooks.set(name,args.at(-1));return {catch(){}};});
  const content='Exact\n雪\u0000feedback';
  const api={session:{id:async()=>sid,cwd:async()=>'/workspace',version:async()=>({version})},
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
      if(frame.action.kind==='prepare_append') {
        const submitted=submissions.at(-1).text;
        checkpoint={schema_version:1,reservation:{binding_id:'binding',target:reference,
          reservation_id:frame.action.reservation_id,request:{operation_id:frame.action.operation_id,content:submitted},
          payload_sha256:createHash('sha256').update(submitted).digest('hex')},
          intent:structuredClone(frame.action.intent),prepared_at_unix_ms:1,stored:null,stored_at_unix_ms:null};
        reply.append_checkpoint=structuredClone(checkpoint);
      }
      if(frame.action.kind==='record_append_stored') {
        checkpoint.stored=structuredClone(frame.action.stored);checkpoint.stored_at_unix_ms=2;
        reply.append_checkpoint=structuredClone(checkpoint);
      }
      return {exitCode:0,isStdoutTruncated:false,isStderrTruncated:false,stdout:JSON.stringify(reply)};
    }},
    prompt:{submit:async(value)=>{
      submissions.push(value);
      const text=`The ${nativeLabel} plugin sent a message:\n${value.text}`
        + "\n\nThis is how Claude Code surfaces a prompt a plugin submits between turns — it starts this turn in the user's place. Address the message above.";
      const event={uuid:crypto.randomUUID(),door:'prompt',origin:{kind:'plugin',name:config.pluginName},
        message:{type:'user',role:'user',content:[{type:'text',text}]}};
      const next=async(e)=>{
        const result={uuid:e.uuid,message:e.message};
        next.trace=[{plugin:'engine',tier:'core',event:'session.append',outcome:'returned',
          received:structuredClone(e),returned:structuredClone(result)}];
        return result;
      };
      await hooks.get('session.append')(api,event,next);
      return {text:value.text,origin:{kind:'plugin',name:config.pluginName}};
    }},
    tool:{register:async()=>{}},ui:{invalidate(){}},clock:{every:(ms,fn)=>{assert.equal(ms,250);timers.push(fn);}}};
  await hooks.get('session.start')(api,{},async()=>({}));
  return {api,hooks,actions,submissions,content,poll:()=>timers[0](),setSid:value=>{sid=value;},
    setReady:value=>{ready=value;},setBusy:value=>{busy=value;}};
}
test('one exact native call retains raw return without consumption or resend',async()=>{
  const f=await fixture();await f.poll();await f.poll();
  assert.deepEqual(f.submissions,[{text:f.content}]);
  assert.deepEqual(f.actions.map(a=>a.kind),['poll','recheck','prepare_append','record_append_stored','record_handoff','poll']);
  const report=f.actions[4];assert.equal(report.operation_id,'review');assert.equal(report.reservation_id,'attempt');
  assert.deepEqual(report.outcome,{kind:'returned',result:{text:f.content,origin:{kind:'plugin',name:'fixture-plugin'}}});
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
  const event={turnId:'human',reason:'answer',isAborted:false,answer:'done'};
  const next=async()=>{next.trace=[{plugin:'engine',tier:'core',event:'turn.complete',outcome:'returned',received:event}];return {};};
  next.origin={plugin:'engine',tier:'core'};
  await f.hooks.get('turn.complete')(f.api,event,next);
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

test('retired exact attempts are never sent and do not block a later review',async()=>{
  for(const state of ['unknown','failed','acknowledged']){
    const f=await fixture(),run=f.api.process.run;let polls=0,checks=0;
    f.api.process.run=async(argv,opts)=>{
      const result=await run(argv,opts),action=JSON.parse(opts.stdin).action;
      if(!action)return result;
      const reply=JSON.parse(result.stdout);
      if(action.kind==='recheck')reply.recorded_state=++checks===1?state:'dispatching';
      if(action.kind==='poll' && ++polls===2){
        const content='Second immutable review 雪';
        reply.reservation={binding_id:'binding',target:reply.reference,reservation_id:'next-attempt',
          request:{operation_id:'next-review',content},payload_sha256:createHash('sha256').update(content).digest('hex')};
      }
      result.stdout=JSON.stringify(reply);return result;
    };
    await f.poll();assert.equal(f.submissions.length,0,state);
    await f.poll();await f.poll();assert.deepEqual(f.submissions,[{text:'Second immutable review 雪'}],state);
    assert.equal(f.actions.find(a=>a.kind==='record_handoff').operation_id,'next-review');
  }
});

test('a reserved attempt unexpectedly reported Accepted stops without native I/O',async()=>{
  const f=await fixture(),run=f.api.process.run;
  f.api.process.run=async(argv,opts)=>{
    const result=await run(argv,opts);
    if(JSON.parse(opts.stdin).action?.kind==='recheck'){
      const reply=JSON.parse(result.stdout);reply.recorded_state='accepted';result.stdout=JSON.stringify(reply);
    }
    return result;
  };
  await f.poll();await f.poll();assert.equal(f.submissions.length,0);assert.equal(f.actions.length,2);
});

test('append hashes correlate the exact native envelope separately from the logical payload',async()=>{
  const f=await fixture();await f.poll();
  const prepared=f.actions.find(a=>a.kind==='prepare_append'),stored=f.actions.find(a=>a.kind==='record_append_stored');
  assert.equal(prepared.intent.native_row_id,stored.native_row_id);
  assert.equal(prepared.intent.native_source,'claude-code/2.1.296/session.append');
  assert.notEqual(prepared.intent.framed_payload_sha256,createHash('sha256').update(f.content).digest('hex'));
  assert.equal(prepared.intent.framed_payload_sha256,stored.stored.stored_payload_sha256);
  assert.match(stored.stored.evidence_reference,new RegExp(`^claude-code:session:native:row:${stored.native_row_id}$`));
});

test('a downstream rewrite is recorded as a mismatch and cannot trigger another native attempt',async()=>{
  const f=await fixture(),hook=f.hooks.get('session.append');let delegated=0;
  f.hooks.set('session.append',(api,event,next)=>hook(api,event,traced(next,async(e)=>{
    delegated++;return next({...e,message:{...e.message,content:[{type:'text',text:'changed downstream'}]}});
  })));
  await f.poll();await f.poll();
  const prepared=f.actions.find(a=>a.kind==='prepare_append'),stored=f.actions.find(a=>a.kind==='record_append_stored');
  assert.notEqual(prepared.intent.framed_payload_sha256,stored.stored.stored_payload_sha256);
  assert.equal(f.submissions.length,1);assert.equal(delegated,1);
  assert.equal(f.actions.at(-1).kind,'record_handoff');
});

test('unverified core storage cannot create a stored checkpoint or resend',async()=>{
  for(const fault of ['missing','duplicate','foreign','skipped','rejected','expired','caught','uuid','door','agent','origin','input','output','result']){
    const f=await fixture(),hook=f.hooks.get('session.append');let delegated=0;
    f.hooks.set('session.append',(api,event,next)=>{
      const wrapped=async(e)=>{
        delegated++;
        const result=await next(e),core=structuredClone(next.trace[0]);
        wrapped.trace=[core];
        if(fault==='missing')wrapped.trace=[];
        if(fault==='duplicate')wrapped.trace.push(structuredClone(core));
        if(fault==='foreign')core.plugin='competitor';
        if(['skipped','rejected','expired','caught'].includes(fault))core.outcome=fault;
        if(fault==='uuid')core.received.uuid=crypto.randomUUID();
        if(fault==='door')core.received.door='note';
        if(fault==='agent')core.received.agentId='child';
        if(fault==='origin')core.received.origin.name='competitor';
        if(fault==='input')core.received.message.content[0].text='changed input';
        if(fault==='output')core.returned.message.content[0].text='changed stored row';
        if(fault==='result')return {...result,message:{...result.message,content:[{type:'text',text:'forged return'}]}};
        return result;
      };
      return hook(api,event,wrapped);
    });
    await f.poll();await f.poll();
    assert.equal(delegated,1,fault);assert.equal(f.submissions.length,1,fault);
    assert.equal(f.actions.filter(a=>a.kind==='prepare_append').length,1,fault);
    assert.equal(f.actions.filter(a=>a.kind==='record_append_stored').length,0,fault);
    assert.equal(f.actions.at(-1).kind,'record_handoff',fault);
  }
});

test('matching passed core storage preserves the candidate without delivery promotion',async()=>{
  const f=await fixture(),hook=f.hooks.get('session.append');
  f.hooks.set('session.append',(api,event,next)=>{
    const wrapped=async(e)=>{
      const result=await next(e);
      wrapped.trace=structuredClone(next.trace);wrapped.trace[0].outcome='passed';
      return result;
    };
    return hook(api,event,wrapped);
  });
  await f.poll();await f.poll();
  assert.equal(f.actions.filter(a=>a.kind==='record_append_stored').length,1);
  assert.equal(f.actions.filter(a=>a.kind==='record_handoff').length,1);
  assert.equal(f.submissions.length,1);
});

test('lost checkpoint responses never redelegate storage or resubmit the frozen review',async()=>{
  for(const phase of ['prepare_append','record_append_stored']){
    const f=await fixture(),run=f.api.process.run,hook=f.hooks.get('session.append');let delegated=0;
    f.hooks.set('session.append',(api,event,next)=>hook(api,event,traced(next,async(e)=>{delegated++;return next(e);})));
    f.api.process.run=async(argv,opts)=>{
      const result=await run(argv,opts);
      if(JSON.parse(opts.stdin).action?.kind===phase)throw Error('lost checkpoint commit reply');
      return result;
    };
    await f.poll();await f.poll();assert.equal(f.submissions.length,1,phase);assert.equal(delegated,1,phase);
    assert.equal(f.actions.filter(a=>a.kind===phase).length,1,phase);
    assert.equal(f.actions.at(-1).kind,'record_handoff',phase);
  }
});

test('foreign checkpoint replies stop capture without claiming another reservation',async()=>{
  for(const field of ['native_row_id','reservation_id','payload']){
    const f=await fixture(),run=f.api.process.run;
    f.api.process.run=async(argv,opts)=>{
      const result=await run(argv,opts);
      if(JSON.parse(opts.stdin).action?.kind==='prepare_append'){
        const reply=JSON.parse(result.stdout),checkpoint=reply.append_checkpoint;
        if(field==='native_row_id')checkpoint.intent.native_row_id='foreign';
        if(field==='reservation_id')checkpoint.reservation.reservation_id='foreign';
        if(field==='payload')checkpoint.reservation.request.content='foreign';
        result.stdout=JSON.stringify(reply);
      }
      return result;
    };
    await f.poll();await f.poll();assert.equal(f.submissions.length,1,field);
    assert.equal(f.actions.filter(a=>a.kind==='record_append_stored').length,0,field);
  }
});

test('foreign-origin and subagent rows are not associated with the in-flight review',async()=>{
  for(const fault of ['origin','agent','framing']){
    const f=await fixture(),hook=f.hooks.get('session.append');let delegated=0;
    f.hooks.set('session.append',(api,event,next)=>{
      if(fault==='origin')event={...event,origin:{kind:'plugin',name:'different-plugin'}};
      if(fault==='agent')event={...event,agentId:'child'};
      if(fault==='framing')event={...event,message:{...event.message,content:[{type:'text',text:'different envelope'}]}};
      return hook(api,event,async(e)=>{delegated++;return next(e);});
    });
    await f.poll();await f.poll();assert.equal(f.submissions.length,1,fault);assert.equal(delegated,1,fault);
    assert.equal(f.actions.filter(a=>a.kind==='prepare_append').length,0,fault);
  }
});

test('a successful submit return without an append event remains unqualified and is not retried',async()=>{
  const f=await fixture();
  f.api.prompt.submit=async(value)=>{f.submissions.push(value);return {text:value.text};};
  await f.poll();await f.poll();assert.equal(f.submissions.length,1);
  assert.deepEqual(f.actions.map(a=>a.kind),['poll','recheck','record_handoff']);
});

test('unknown native framing versions refuse message-pump startup',async()=>{
  for(const version of ['2.1.297','2.1.309','2.2.0'])
    await assert.rejects(fixture({version}),/not qualified/);
});

for(const version of ['2.1.296','2.1.308']) test(`qualified ${version} shortened native labels keep full plugin provenance and exact payload hashing`,async()=>{
  const f=await fixture({version,pluginName:'aikit-sdk-example-dfc261c6',nativeLabel:'aikit-sdk-example-dfc26…'});
  await f.poll();await f.poll();
  assert.equal(f.actions.filter(a=>a.kind==='prepare_append').length,1);
  assert.equal(f.actions.filter(a=>a.kind==='record_append_stored').length,1);
  assert.equal(f.submissions.length,1);
  const returned=f.actions.find(a=>a.kind==='record_handoff').outcome.result;
  assert.equal(returned.origin.name,'aikit-sdk-example-dfc261c6');
});
