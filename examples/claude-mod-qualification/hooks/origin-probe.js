// Native origin/trace diagnostic only. No application or readiness claims.
const events=[];
let writes=Promise.resolve();
async function record($,kind,value) {
  events.push({kind,value});const bytes=JSON.stringify(events);
  writes=writes.then(()=>$.fs.write('/home/node/evidence/origin-events.json',bytes));await writes;
}
export function register(on) {
  on('session.start',async($,event,next)=>{
    await $.tool.register({name:'echo',description:'Synthetic origin probe echo',isDeferred:false,
      inputSchema:{type:'object',properties:{text:{type:'string'}},required:['text'],additionalProperties:false}});
    await record($,'session_start',{sessionId:await $.session.id(),event,origin:next.origin,plugin:{name:$.plugin.name,root:$.plugin.root}});
    return next(event);
  });
  on('tool.call',{tool:'mcp__aikit-mod-feasibility__echo'},async($,event,next)=>{
    await record($,'tool_call',{text:event.text,origin:next.origin});
    return {result:JSON.stringify({text:event.text})};
  });
  on('turn.start',async($,event,next)=>{
    await record($,'turn_start',{event,origin:next.origin,trace:next.trace});
    const tool=await $.tool.call({tool:'mcp__aikit-mod-feasibility__echo',text:'AIKIT_PLUGIN_RAISED_TOOL'});
    await record($,'plugin_tool_return',tool);
    const result=await $.turn.complete({turnId:event.turnId,answer:'AIKIT_PLUGIN_RAISED_OUTCOME',
      durationMs:1,isAborted:false,reason:'answer'});
    await record($,'plugin_completion_return',result);
    return next(event);
  });
  on('classic.Stop',async($,event,next)=>{
    const before=[...next.trace];
    const result=await next(event);
    await record($,'stop',{event,origin:next.origin,before,after:next.trace});return result;
  });
  on('turn.complete',async($,event,next)=>{
    const before=[...next.trace];
    const result=await next(event);
    await record($,'turn_complete',{event,origin:next.origin,before,after:next.trace});return result;
  });
}
