// Later handler changes conversation state, then really delegates the request.
let appended=false;
export function register(on){
  on('turn.step',async function*($,e,next){
    if(e.agentId===undefined && e.index===0 && !appended){
      const before=await $.session.messages({as:'api'});
      if(JSON.stringify(before).includes('AIKIT_INPUT_MUTATION')){
        if(await $.session.cwd()!=='/home/node/workspace'
            ||await $.fs.read('/home/node/workspace/.codelaya-qualification-only')!=='')throw Error('Marked workspace required');
        appended=true;
        const result=await $.session.append({message:{type:'user',content:[{type:'text',text:'AIKIT_INPUT_LATE_雪'}]}});
        await $.fs.write('/home/node/evidence/late-append.json',JSON.stringify({
          session_id:await $.session.id(),event:e,before,result,after:await $.session.messages({as:'api'})}));
      }
    }
    return yield* next(e);
  });
}
