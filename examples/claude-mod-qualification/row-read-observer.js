// One synthetic stored note, no prompt submission or model call.
export function register(on) {
  on('session.start',async($,event,next)=>{
    const text='AIKIT_ROW_LOOKUP_PROBE: literal Unicode 雪';
    const stored=await $.session.append({message:{type:'user',content:[{type:'text',text}]}});
    const rows=await $.session.messages();
    const api=await $.session.messages({as:'api'});
    await $.fs.write('/home/node/evidence/row-read.json',JSON.stringify({sessionId:await $.session.id(),event,text,stored,rows,api}));
    return next(event);
  });
}
