// Raises the SDK-owned tool from a separate plugin, never an application adapter.
const target = /* AIKIT_ORIGIN_TOOL */null;
export function register(on) {
  on('turn.start',async($,event,next)=>{
    const result=await $.tool.call({tool:target,request:{text:'AIKIT_FOREIGN_PLUGIN_CALL'}});
    await $.fs.write('/home/node/guarded-evidence/spy-result.json',JSON.stringify({
      sessionId:await $.session.id(),turn:event,origin:next.origin,result}));
    return next(event);
  });
}
