// Fresh disposable Linux fixture only. No credentials, agent launch or owner state.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
const workspace='/home/node/workspace',state='/home/node/message-state',evidence='/home/node/evidence';
assert(!fs.existsSync(state) && !fs.existsSync(`${workspace}/.claude`),'fresh fixture required');
fs.mkdirSync(evidence,{recursive:true});
fs.mkdirSync(workspace,{recursive:true});
const run=(args,input)=>JSON.parse(execFileSync('/generated/integration_hooks',args,
  {input:input && JSON.stringify(input),encoding:'utf8',maxBuffer:2*1024*1024}));
fs.mkdirSync(`${workspace}/.claude`);
fs.writeFileSync(`${workspace}/.claude/settings.local.json`,JSON.stringify({env:{UNRELATED_FIXTURE_SETTING:'keep'}}));
const plan=run(['plan',state],{application_id:'sdk-example',agent_key:'claude',workspace,
  handler:{executable:'/generated/integration_hooks',arguments:['hook',state,workspace,'0']},
  events:['session_started','session_ended','input_submitted','completion_proposed'],timeout_seconds:10});
assert.equal(run(['apply',state,plan.id]).status,'configured');
const tool=run(['plan-tool-message',state,plan.installation_id,'/generated/integration_message_bridge',state,plan.installation_id],{
  application_id:'sdk-example',installation_id:plan.installation_id,tool_name:'review',description:'Disposable message qualification tool',
  input_schema:{type:'object',properties:{text:{type:'string'}},additionalProperties:false},instructions:'',
  handler:{executable:'/generated/agent_tool_plugin',arguments:['inspect']},timeout_ms:5000});
assert.equal(run(['apply',state,tool.id]).status,'configured');
const module=tool.files.find(file=>file.path.endsWith('/hooks/register.js'));
assert(module,'owned generated module required');
const info={schema_version:1,workspace,state,installation_id:plan.installation_id,
  root:module.path.slice(0,-'/hooks/register.js'.length),files:tool.files,production_ready:false};
fs.writeFileSync(`${evidence}/setup.json`,JSON.stringify(info,null,2));
assert.equal(JSON.parse(fs.readFileSync(`${workspace}/.claude/settings.local.json`)).env.UNRELATED_FIXTURE_SETTING,'keep');
console.log(JSON.stringify(info));
