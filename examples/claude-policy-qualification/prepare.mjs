// Fresh disposable Linux home only. Does not modify an owner profile.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
const mode=process.argv[2];assert(['healthy','catch','catch-fails'].includes(mode));
const home='/home/node',workspace=home+'/workspace',evidence=home+'/evidence';
assert(!fs.existsSync(home+'/.claude.json'));assert(!fs.existsSync(home+'/.claude/settings.json'));
fs.mkdirSync(evidence,{recursive:true});fs.mkdirSync(home+'/.claude',{recursive:true});
fs.writeFileSync(workspace+'/.aikit-native-policy-probe','disposable native policy probe\n',{flag:'wx'});
fs.writeFileSync(home+'/.claude.json',JSON.stringify({hasCompletedOnboarding:true,lastOnboardingVersion:'2.1.308',
  customApiKeyResponses:{approved:['ure-not-a-credential'],rejected:[]}}),{flag:'wx',mode:0o600});
const marketplace='aikit-policy-probe',plugins=['policy-guard','policy-benign','policy-competing'];
for(const name of plugins) {
  const root=workspace+'/'+name;fs.mkdirSync(root+'/.claude-plugin',{recursive:true});fs.mkdirSync(root+'/hooks',{recursive:true});
  fs.writeFileSync(root+'/.claude-plugin/plugin.json',JSON.stringify({name,version:'0.0.1',description:'Disposable native policy qualification'}));
  fs.writeFileSync(root+'/hooks/hooks.json',JSON.stringify({modules:['./register.js']}));
}
const guard=`const mode=${JSON.stringify(mode)};
const seen=[];
export function register(on) {
  on('plugin.register',async($,event,next)=>{
    const risky=event.tier==='user'&&event.uses.events.includes('classic.Stop');
    seen.push({event,origin:next.origin,risky});
    await $.fs.write('/home/node/evidence/admission.json',JSON.stringify(seen));
    if(risky) {
      if(mode!=='healthy')throw Error('Declared primary gate fault');
      return {refuse:'AIKit probe: competing completion handler'};
    }
    return next(event);
  }).catch(()=>{
    if(mode==='catch-fails')throw Error('Declared fallback fault');
    return {refuse:'AIKit probe: constant fallback refusal'};
  });
  on('session.start',async($,event,next)=>{
    await $.fs.write('/home/node/evidence/guard-started.json',JSON.stringify({session_id:await $.session.id()}));
    return next(event);
  });
}
`;
const benign=`export function register(on) {
  on('session.start',async($,event,next)=>{
    await $.fs.write('/home/node/evidence/benign-started.json',JSON.stringify({session_id:await $.session.id()}));
    return next(event);
  });
}
`;
const competing=`export function register(on) {
  on('session.start',async($,event,next)=>{
    await $.fs.write('/home/node/evidence/competing-started.json',JSON.stringify({session_id:await $.session.id()}));
    return next(event);
  });
  on('classic.Stop',async()=>({}));
}
`;
for(const [name,source] of [['policy-guard',guard],['policy-benign',benign],['policy-competing',competing]])
  fs.writeFileSync(workspace+'/'+name+'/hooks/register.js',source);
fs.mkdirSync(workspace+'/.claude-plugin',{recursive:true});
fs.writeFileSync(workspace+'/.claude-plugin/marketplace.json',JSON.stringify({name:marketplace,owner:{name:'AIKit qualification'},
  plugins:plugins.map(name=>({name,source:'./'+name,description:'Disposable native module admission probe'}))}));
const settings={extraKnownMarketplaces:{[marketplace]:{source:{source:'directory',path:workspace}}},
  enabledPlugins:Object.fromEntries(plugins.map(name=>[name+'@'+marketplace,true])),prependPlugins:['policy-guard@'+marketplace]};
fs.writeFileSync(home+'/.claude/settings.json',JSON.stringify(settings),{flag:'wx'});
const hash=file=>createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const native=hash('/usr/local/bin/claude');
assert.equal(native,'356733f185fa988e6bf3aa9a555269fdd167982a2e2a80d3c8a78037bb92ad58');
fs.writeFileSync(evidence+'/setup.json',JSON.stringify({mode,settings,native_sha256:native,
  source_sha256:Object.fromEntries(plugins.map(name=>[name,hash(workspace+'/'+name+'/hooks/register.js')])),
  windows_qualified:false,mandatory_policy_qualified:false}),{flag:'wx'});
console.log(JSON.stringify({prepared:mode,model_prompts:0}));
