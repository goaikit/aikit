// Opt-in native transport feasibility only; not durable SDK messaging.
// File rendezvous is confined to a disposable fixture, never a production queue.
const events=[];
let writes=Promise.resolve(),active=false,attempted=false,polling=false;
async function record($,kind,value) {
  events.push({kind,value});const bytes=JSON.stringify(events);
  writes=writes.then(()=>$.fs.write('/home/node/evidence/feedback-events.json',bytes));await writes;
}
async function poll($) {
  if(active || attempted || polling) return;
  polling=true;
  try {
    const file='/home/node/evidence/feedback-request.json';
    if(!await $.fs.exists(file)) return;
    const request=JSON.parse(await $.fs.read(file));
    const sessionId=await $.session.id();
    if(request.schema_version!==1 || request.native_session_id!==sessionId
      || typeof request.payload!=='string' || new TextEncoder().encode(request.payload).length>65536)
      throw new Error('Fixture feedback scope/size mismatch');
    const hash=[...new Uint8Array(await crypto.subtle.digest('SHA-256',new TextEncoder().encode(request.payload)))]
      .map(byte=>byte.toString(16).padStart(2,'0')).join('');
    if(hash!==request.payload_sha256) throw new Error('Fixture payload digest mismatch');
    attempted=true; // One attempt; a throw/unknown result never resets this.
    await record($,'submit_attempt',{operationId:request.operation_id,sessionId,payloadSha256:hash});
    const result=await $.prompt.submit({text:request.payload});
    await record($,'submit_return',{operationId:request.operation_id,sessionId,result});
  } catch {
    attempted=true;await record($,'submit_unavailable',{uncertain:true});
  } finally {polling=false;}
}
export function register(on) {
  on('session.start',async($,event,next)=>{
    await record($,'session_start',{sessionId:await $.session.id(),event});
    $.clock.every(250,async()=>poll($));
    return next(event);
  });
  on('turn.start',async($,event,next)=>{
    if(event.agentId===undefined) active=true;
    await record($,'turn_start',event);return next(event);
  });
  on('turn.complete',async($,event,next)=>{
    const result=await next(event);
    await record($,'turn_complete',event);
    if(event.agentId===undefined) active=false;
    return result;
  });
}
