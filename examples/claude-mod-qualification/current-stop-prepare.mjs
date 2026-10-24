// Fresh marked container only; never use against an owner profile.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
const mode=process.argv[2];assert(['baseline','missing','timeout'].includes(mode));
const home='/home/node',evidence=home+'/evidence';
assert.equal(process.getuid(),1000);assert(!fs.existsSync(home+'/.claude.json'));
assert.equal(execFileSync('claude',['--version'],{encoding:'utf8'}).trim(),'2.1.308 (Claude Code)');
fs.writeFileSync(home+'/workspace/.aikit-qualification-only','',{flag:'wx'});
execFileSync('git',['init','-q',home+'/workspace']);
fs.mkdirSync(home+'/.claude',{recursive:true});
fs.writeFileSync(home+'/.claude.json',JSON.stringify({hasCompletedOnboarding:true,lastOnboardingVersion:'2.1.308',
  customApiKeyResponses:{approved:['ure-not-a-credential'],rejected:[]}}),{flag:'wx'});
fs.writeFileSync(home+'/probe/.claude-plugin/plugin.json',JSON.stringify({name:'aikit-mod-feasibility',version:'0.0.1',description:'Disposable guarded Stop diagnostic'}));
fs.writeFileSync(home+'/probe/hooks/hooks.json',JSON.stringify({modules:['./register.js']}));
const script={baseline:'stop-chain-command.mjs',missing:'intentionally-missing-policy.mjs',timeout:'stop-timeout-command.mjs'}[mode];
const settings={hooks:{Stop:[{hooks:[{type:'command',command:'node /home/node/probe/'+script,timeout:1,onFailure:'block'}]}]}};
fs.writeFileSync(evidence+'/expected-settings.json',JSON.stringify(settings));
fs.writeFileSync(evidence+'/case.json',JSON.stringify({mode,version:'2.1.308',production_activated:false,owner_settings_changed:false}));
fs.writeFileSync(evidence+'/native-version.txt',execFileSync('claude',['--version'],{encoding:'utf8'}));
