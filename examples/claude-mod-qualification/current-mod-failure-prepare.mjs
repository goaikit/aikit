// Disposable node home only; no installation in an owner's profile.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
const mode=process.argv[2];assert(['pending','busy'].includes(mode));
assert.equal(process.getuid(),1000);const root='/home/node';
assert(!fs.existsSync(root+'/.claude.json'));
assert.equal(execFileSync('claude',['--version'],{encoding:'utf8'}).trim(),'2.1.308 (Claude Code)');
fs.writeFileSync(root+'/workspace/.aikit-qualification-only','',{flag:'wx'});
execFileSync('git',['init','-q',root+'/workspace']);
fs.writeFileSync(root+'/.claude.json',JSON.stringify({hasCompletedOnboarding:true,lastOnboardingVersion:'2.1.308',
  customApiKeyResponses:{approved:['ure-not-a-credential'],rejected:[]}}),{flag:'wx'});
const source=fs.readFileSync(root+'/completion-source.js','utf8').replaceAll('\r\n','\n');
const suffix=source.match(/\}\)\.catch\((\(\) => \(\{ block: '[^']+' \}\))\);\s*$/);
assert(suffix,'exact constant SDK fallback required');
const fallback=suffix[1];
for(const name of ['guard','observer']) {
  fs.writeFileSync(root+'/'+name+'/.claude-plugin/plugin.json',JSON.stringify({name:'aikit-mod-'+name,version:'0.0.1'}));
  fs.writeFileSync(root+'/'+name+'/hooks/hooks.json',JSON.stringify({modules:['./register.js']}));
}
const guard=`export function register(on) {
  on('classic.Stop',async($,event,next)=>{
    const calls=JSON.parse(await $.fs.read('/home/node/evidence/guard-calls.json'));
    calls.push({event,at:Date.now(),budget:{ms:next.budget.ms,remainingMs:next.budget.remainingMs}});
    await $.fs.write('/home/node/evidence/guard-calls.json',JSON.stringify(calls));
    if(calls.length>1)return next(event);
    ${mode==='pending'?'await new Promise(()=>{});':'while(true) {}'}
  }).catch(${fallback});
}
`;
fs.writeFileSync(root+'/guard/hooks/register.js',guard);
fs.writeFileSync(root+'/evidence/guard-calls.json','[]',{flag:'wx'});
fs.writeFileSync(root+'/evidence/case.json',JSON.stringify({mode,version:'2.1.308',fallback,
  sdk_source_sha256:createHash('sha256').update(source).digest('hex'),
  guard_sha256:createHash('sha256').update(guard).digest('hex'),production_activated:false,owner_settings_changed:false}));
fs.writeFileSync(root+'/evidence/native-version.txt',execFileSync('claude',['--version'],{encoding:'utf8'}));
