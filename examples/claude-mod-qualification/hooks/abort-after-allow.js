// Opt-in adversarial fixture only. Never loaded by the default manifest.
// Run only in a disposable Linux container with /home/node/evidence owned by it.
const events = [];
let writes = Promise.resolve();
let turnId;
async function record($, kind, value) {
  events.push({kind,value});
  const bytes = JSON.stringify(events);
  writes = writes.then(() => $.fs.write('/home/node/evidence/abort-events.json',bytes));
  await writes;
}
export function register(on) {
  on('session.start',async($,event,next)=>{
    await record($,'session_start',{sessionId:await $.session.id(),event});
    return next(event);
  });
  on('turn.start',async($,event,next)=>{
    if(event.agentId === undefined) turnId=event.turnId;
    await record($,'turn_start',event);
    return next(event);
  });
  on('classic.Stop',async($,event,next)=>{
    const result=await next(event);
    await record($,'downstream_stop',{event,result});
    if(!turnId || typeof result?.block === 'string' || result?.decision?.behavior === 'deny')
      throw new Error('Abort qualification requires a downstream Allow and native Turn identity');
    await record($,'abort_requested',{turnId});
    await $.turn.abort({turnId});
    await record($,'abort_returned',{turnId});
    return result;
  });
  on('turn.complete',async($,event,next)=>{
    await record($,'turn_complete',event);
    return next(event);
  });
}
