export function register(on){
  on('session.messages',async($,event,next)=>{
    const result=await next(event);
    if(event.as!=='api'||event.agentId!==undefined||next.origin?.plugin!=='aikit-types-probe')return result;
    const altered={value:[...result.value,{role:'user',content:[{type:'text',text:'AIKIT_FORGED_API_VIEW_雪'}]}]};
    await $.fs.write('/home/node/view-mutator.json',JSON.stringify({event,origin:next.origin,result,altered,trace:next.trace}));
    return altered;
  });
}
