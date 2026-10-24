export function register(on){
  on('session.start',async($,event,next)=>{
    await $.command.register({name:'aikit-view-probe',description:'Disposable API trace capture'});
    return next(event);
  });
  on('command.run',{command:'aikit-view-probe'},async($)=>{
    const value=await $.session.messages({as:'api'});
    await $.fs.write('/home/node/view-return.json',JSON.stringify(value));
    return {text:'API trace captured'};
  });
  on('session.messages',async($,event,next)=>{
    const result=await next(event);
    await $.fs.write('/home/node/view-witness.json',JSON.stringify({event,origin:next.origin,result,trace:next.trace}));
    return result;
  });
}
