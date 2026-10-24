// Fresh isolated container only. Normal terminal workspace trust remains required.
import fs from 'node:fs';
const home='/home/node',workspace=home+'/workspace',evidence=home+'/evidence';
fs.mkdirSync(workspace,{recursive:true});fs.mkdirSync(evidence,{recursive:true});
fs.writeFileSync(workspace+'/.codelaya-qualification-only','',{flag:'wx'});
if(fs.existsSync(home+'/.claude.json')||fs.existsSync(home+'/.claude/settings.json'))throw Error('fresh disposable profile required');
fs.mkdirSync(home+'/.claude',{recursive:true});
fs.writeFileSync(home+'/.claude.json',JSON.stringify({hasCompletedOnboarding:true,lastOnboardingVersion:'2.1.308',
  customApiKeyResponses:{approved:['ure-not-a-credential'],rejected:[]}}),{flag:'wx'});
fs.copyFileSync(home+'/.claude.json',evidence+'/offline-onboarding.json');
const marketplace='dispatch-qualification';
fs.mkdirSync(workspace+'/.claude-plugin',{recursive:true});
for(const name of ['dispatch-observer','dispatch-shortcut']){
  const root=workspace+'/'+name;
  fs.mkdirSync(root+'/.claude-plugin',{recursive:true});fs.mkdirSync(root+'/hooks',{recursive:true});
  fs.writeFileSync(root+'/.claude-plugin/plugin.json',JSON.stringify({name,version:'0.0.1',description:'Disposable native dispatch diagnostic'}));
  fs.writeFileSync(root+'/hooks/hooks.json',JSON.stringify({modules:['./register.js']}));
  fs.copyFileSync('/qualification/'+(name==='dispatch-observer'?'observer.js':'shortcut.js'),root+'/hooks/register.js');
}
fs.writeFileSync(workspace+'/.claude-plugin/marketplace.json',JSON.stringify({name:marketplace,owner:{name:'AIKit diagnostic'},
  plugins:['dispatch-observer','dispatch-shortcut'].map(name=>({name,source:'./'+name,description:'Disposable native dispatch diagnostic'}))}));
const settings={extraKnownMarketplaces:{[marketplace]:{source:{source:'directory',path:workspace}}},
  enabledPlugins:{['dispatch-observer@'+marketplace]:true,['dispatch-shortcut@'+marketplace]:true},
  prependPlugins:['dispatch-observer@'+marketplace]};
fs.writeFileSync(home+'/.claude/settings.json',JSON.stringify(settings));
fs.writeFileSync(evidence+'/settings-before.json',JSON.stringify(settings));
