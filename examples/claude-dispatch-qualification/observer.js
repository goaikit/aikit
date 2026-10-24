// Disposable native API diagnostic. Does not emit production message proofs.
const events=[];
let writes=Promise.resolve();
async function record($,kind,value){
  events.push({kind,value});const text=JSON.stringify(events);
  writes=writes.then(()=>$.fs.write('/home/node/evidence/dispatch-events.json',text));await writes;
}
async function marked($){
  if(await $.session.cwd()!=='/home/node/workspace'
      ||await $.fs.read('/home/node/workspace/.codelaya-qualification-only')!=='')
    throw Error('Marked disposable workspace required');
}
export function register(on){
  on('session.start',async($,e,next)=>{
    await marked($);
    await record($,'session_start',{session_id:await $.session.id(),origin:next.origin});
    return next(e);
  });
  on('turn.step',async function*($,e,next){
    await marked($);
    if(e.agentId!==undefined)return yield* next(e);
    const messages=await $.session.messages({as:'api'});
    await record($,'before',{session_id:await $.session.id(),event:e,origin:next.origin,messages});
    const result=yield* next(e);
    const trace=next.trace;
    await record($,'after',{session_id:await $.session.id(),event:e,result,trace,
      naive_consumption_candidate:JSON.stringify(messages).includes('AIKIT_DISPATCH_WITHOUT_ENGINE')
        &&result?.usage!==null&&result?.usage!==undefined&&result?.stopReason==='end_turn',
      matching_core:trace.filter(row=>row.plugin==='engine'&&row.tier==='core'&&row.event==='turn.step'
        &&['returned','passed'].includes(row.outcome)&&row.received?.turnId===e.turnId
        &&row.received?.index===e.index).length===1});
    return result;
  });
  on('turn.complete',async($,e,next)=>{
    const result=await next(e);
    await record($,'turn_complete',{session_id:await $.session.id(),event:e});
    return result;
  });
}
