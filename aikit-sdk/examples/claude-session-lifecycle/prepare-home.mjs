import fs from 'node:fs';
const root='/home/node/probe';
if(fs.existsSync('/home/node/.claude.json'))throw new Error('Fresh disposable home required');
fs.writeFileSync('/home/node/.aikit-disposable-lifecycle','offline qualification only',{flag:'wx'});
fs.mkdirSync(root+'/.claude-plugin',{recursive:true});
fs.mkdirSync(root+'/hooks',{recursive:true});
fs.mkdirSync('/home/node/evidence',{recursive:true});
fs.writeFileSync(root+'/.claude-plugin/plugin.json',JSON.stringify({name:'aikit-lifecycle-probe',version:'0.0.1',description:'Offline disposable lifecycle event probe'}));
fs.writeFileSync(root+'/hooks/hooks.json',JSON.stringify({modules:['./register.js']}));
fs.writeFileSync(root+'/hooks/register.js',`const observations=[];
async function record($,name,event,phase) {
  const native_session_id=await $.session.id();
  observations.push({index:observations.length,name,phase,reason:event.reason,source:event.source,
    event_session_id:event.session_id,native_session_id});
  await $.fs.write('/home/node/evidence/lifecycle.json',JSON.stringify(observations));
}
export function register(on) {
  on('session.start',async($,event,next)=>{await record($,'session.start',event,'before');const result=await next(event);await record($,'session.start',event,'after');return result;});
  on('session.end',async($,event,next)=>{await record($,'session.end',event,'before');const result=await next(event);await record($,'session.end',event,'after');return result;});
  on('classic.SessionStart',async($,event,next)=>{await record($,'classic.SessionStart',event,'before');const result=await next(event);await record($,'classic.SessionStart',event,'after');return result;});
  on('classic.SessionEnd',async($,event,next)=>{await record($,'classic.SessionEnd',event,'before');const result=await next(event);await record($,'classic.SessionEnd',event,'after');return result;});
}
`);
fs.writeFileSync('/home/node/.claude.json',JSON.stringify({hasCompletedOnboarding:true,lastOnboardingVersion:'2.1.308',customApiKeyResponses:{approved:['ure-not-a-credential'],rejected:[]}}),{flag:'wx',mode:0o600});
