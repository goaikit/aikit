// Disposable input-boundary experiment; never emits production receipts.
const events=[];
let writes=Promise.resolve();
async function record($,kind,value){
  events.push({kind,value});const text=JSON.stringify(events);
  writes=writes.then(()=>$.fs.write('/home/node/evidence/input-events.json',text));await writes;
}
export function register(on){
  on('session.start',async($,e,next)=>{
    if(await $.session.cwd()!=='/home/node/workspace'
        ||await $.fs.read('/home/node/workspace/.codelaya-qualification-only')!=='')throw Error('Marked workspace required');
    await record($,'session_start',{session_id:await $.session.id(),origin:next.origin});
    return next(e);
  });
  on('turn.step',async function*($,e,next){
    if(e.agentId!==undefined)return yield* next(e);
    const before=await $.session.messages({as:'api'});
    await record($,'before',{session_id:await $.session.id(),event:e,origin:next.origin,messages:before});
    const result=yield* next(e);
    await record($,'after',{session_id:await $.session.id(),event:e,result,trace:next.trace,
      messages:await $.session.messages({as:'api'})});
    return result;
  });
}
