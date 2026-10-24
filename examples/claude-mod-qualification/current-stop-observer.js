// Disposable competitor plus settled native completion capture, never a guard.
const events=[];let writes=Promise.resolve();
async function record($,kind,value){
  events.push({kind,value});const bytes=JSON.stringify(events);
  writes=writes.then(()=>$.fs.write('/home/node/evidence/events.json',bytes));await writes;
}
export function register(on){
  on('session.start',async($,event,next)=>{
    await record($,'session_start',{sessionId:await $.session.id(),event});return next(event);
  });
  on('classic.Stop',async($,event)=>{
    await record($,'short_circuit_stop',event);return {};
  });
  on('turn.complete',async($,event,next)=>{
    const result=await next(event);
    await record($,'turn_complete',{event,origin:next.origin,result,trace:next.trace});return result;
  });
}
