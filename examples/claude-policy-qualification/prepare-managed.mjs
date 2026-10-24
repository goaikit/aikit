// Fresh node home only; writes no machine policy. Root installs the emitted file.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
const mode=process.argv[2];
const cases={block:['Stop','block'],missing:['Stop','missing'],fallback:['Stop','fallback'],timeout:['Stop','timeout'],
  'missing-block':['Stop','missing',true],'timeout-block':['Stop','timeout',true],
  'input-default':['UserPromptSubmit','missing'],'input-block':['UserPromptSubmit','missing',true],
  'tool-default':['PreToolUse','missing'],'tool-block':['PreToolUse','missing',true]};
assert(Object.hasOwn(cases,mode));const [event,fault,failClosed]=cases[mode];
const home='/home/node',workspace=home+'/workspace',evidence=home+'/evidence';
assert(!fs.existsSync(home+'/.claude.json'));assert(!fs.existsSync(home+'/.claude/settings.json'));
fs.mkdirSync(evidence,{recursive:true});fs.mkdirSync(home+'/.claude',{recursive:true});
fs.writeFileSync(workspace+'/.aikit-native-policy-probe','disposable managed Stop probe\n',{flag:'wx'});
fs.writeFileSync(home+'/.claude.json',JSON.stringify({hasCompletedOnboarding:true,lastOnboardingVersion:'2.1.308',
  customApiKeyResponses:{approved:['ure-not-a-credential'],rejected:[]}}),{flag:'wx',mode:0o600});
const marketplace='aikit-managed-stop',plugins=['managed-observer','managed-bypass'];
for(const name of plugins) {
  const root=workspace+'/'+name;fs.mkdirSync(root+'/.claude-plugin',{recursive:true});fs.mkdirSync(root+'/hooks',{recursive:true});
  fs.writeFileSync(root+'/.claude-plugin/plugin.json',JSON.stringify({name,version:'0.0.1',description:'Disposable native managed Stop probe'}));
  fs.writeFileSync(root+'/hooks/hooks.json',JSON.stringify({modules:['./register.js']}));
}
const observer=`const records=[];
async function save($,kind,event,next) {
  records.push({kind,event,origin:next.origin,trace:next.trace,session_id:await $.session.id()});
  await $.fs.write('/home/node/evidence/native-outcomes.json',JSON.stringify(records));
}
export function register(on) {
  on('session.start',async($,event,next)=>{
    const policy=await $.settings.read({source:'policy'});
    await $.fs.write('/home/node/evidence/native-policy.json',JSON.stringify(policy));
    await save($,'session.start',event,next);return next(event);
  });
  on('turn.start',async($,event,next)=>{await save($,'turn.start',event,next);return next(event);});
  on('turn.complete',async($,event,next)=>{const result=await next(event);await save($,'turn.complete',event,next);return result;});
}
`;
const bypass=`export function register(on) {
  on('classic.${event}',async($,event,next)=>{
    await $.fs.write('/home/node/evidence/user-stop-intercept.json',JSON.stringify({event,origin:next.origin}));
    return {};
  });
}
`;
fs.writeFileSync(workspace+'/managed-observer/hooks/register.js',observer);
fs.writeFileSync(workspace+'/managed-bypass/hooks/register.js',bypass);
fs.mkdirSync(workspace+'/.claude-plugin',{recursive:true});
fs.writeFileSync(workspace+'/.claude-plugin/marketplace.json',JSON.stringify({name:marketplace,owner:{name:'AIKit diagnostic'},
  plugins:plugins.map(name=>({name,source:'./'+name,description:'Disposable native policy experiment'}))}));
const settings={extraKnownMarketplaces:{[marketplace]:{source:{source:'directory',path:workspace}}},
  enabledPlugins:Object.fromEntries(plugins.map(name=>[name+'@'+marketplace,true]))};
fs.writeFileSync(home+'/.claude/settings.json',JSON.stringify(settings),{flag:'wx'});
const block=JSON.stringify({decision:'block',reason:'AIKIT_MANAGED_STOP_BLOCK: validation unavailable'});
const shell=fault==='block'?`printf '%s\\n' '${block}'`
  :fault==='timeout'?`printf 'started\\n' > /home/node/evidence/managed-command-started.txt; sleep 5; printf '%s\\n' '${block}'`
  :`/nonexistent/aikit-validator >/dev/null 2>/dev/null; printf '%s\\n' '${block}'`;
const command=fault==='missing'?{command:'/nonexistent/aikit-validator'}:{command:'/bin/sh',args:['-c',shell]};
const managed={hooks:{[event]:[{...(event==='PreToolUse'?{matcher:'Read'}:{}),
  hooks:[{type:'command',...command,timeout:fault==='timeout'?1:10,...(failClosed?{onFailure:'block'}:{})}]}]}};
fs.writeFileSync(evidence+'/managed-settings.json',JSON.stringify(managed),{flag:'wx'});
const hash=file=>createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const native=hash('/usr/local/bin/claude');assert.equal(native,'356733f185fa988e6bf3aa9a555269fdd167982a2e2a80d3c8a78037bb92ad58');
fs.writeFileSync(evidence+'/setup.json',JSON.stringify({mode,event,fault,settings,managed,native_sha256:native,
  source_sha256:Object.fromEntries(plugins.map(name=>[name,hash(workspace+'/'+name+'/hooks/register.js')])),
  windows_qualified:false,mandatory_policy_qualified:false}),{flag:'wx'});
console.log(JSON.stringify({prepared:mode,machine_policy_installed:false}));
