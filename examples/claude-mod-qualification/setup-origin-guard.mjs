// Actual SDK renderer; exports only into a fresh directory, starts no agent.
import fs from 'node:fs';
import {execFileSync} from 'node:child_process';
const evidence='/home/node/guarded-evidence';
fs.mkdirSync(evidence,{recursive:true});
const spec={application_id:'origin-guard',installation_id:'origin-qualification',tool_name:'echo',
  description:'Synthetic SDK origin dispatch echo',input_schema:{type:'object'},instructions:'',
  handler:{executable:process.execPath,arguments:['/home/node/probe/origin-handler.mjs']},
  timeout_ms:1000,native_completion_bridge:false};
const exported=JSON.parse(execFileSync('/home/node/bin/agent_tool_plugin',
  ['export','claude','/home/node/guarded-plugin'],{input:JSON.stringify(spec),encoding:'utf8'}));
const spy='/home/node/origin-spy';
fs.mkdirSync(spy+'/.claude-plugin',{recursive:true});fs.mkdirSync(spy+'/hooks',{recursive:true});
fs.writeFileSync(spy+'/.claude-plugin/plugin.json',JSON.stringify({name:'origin-spy',version:'0.0.1',
  description:'Synthetic foreign dispatch probe',author:{name:'AIKit'}}));
fs.writeFileSync(spy+'/hooks/hooks.json',JSON.stringify({modules:['./origin-spy.js']}));
fs.writeFileSync(spy+'/hooks/origin-spy.js',fs.readFileSync('/home/node/probe/hooks/origin-spy.js','utf8')
  .replace('/* AIKIT_ORIGIN_TOOL */null',JSON.stringify(exported.tool_name)));
fs.writeFileSync(evidence+'/setup.json',JSON.stringify({spec,...exported},null,2));
console.log(JSON.stringify(exported));
