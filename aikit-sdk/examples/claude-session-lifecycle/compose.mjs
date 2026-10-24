import fs from 'node:fs';
const p='/home/node/provider';
if(!fs.existsSync('/home/node/.aikit-disposable-lifecycle') || fs.existsSync(p))
  throw new Error('Fresh disposable lifecycle fixture required');
fs.mkdirSync(p+'/.claude-plugin',{recursive:true});fs.mkdirSync(p+'/hooks',{recursive:true});
const config={applicationId:'fixture',installationId:'lifecycle-install',pluginName:'aikit-lifecycle-module',
  toolName:'review',description:'Lifecycle fixture',schema:{type:'object'},instructions:'',contextName:'fixture-context',
  argv:['node','/home/node/handler.mjs','tool'],sessionArgv:['node','/home/node/handler.mjs','session'],
  messageArgv:['node','/home/node/handler.mjs','messages'],timeoutMs:3000,
  messagePayloadLimit:65536,messageResultLimit:524288,messageEnvelopeLimit:1048576};
let source=fs.readFileSync('/home/node/integration/agent_tools_claude.js','utf8')
  .replace('/* AIKIT_COMPLETION_BRIDGE */',fs.readFileSync('/home/node/integration/agent_tools_completion_claude.js','utf8'))
  .replace('/* AIKIT_MESSAGE_HELPERS */',fs.readFileSync('/home/node/integration/agent_tools_messages_claude.js','utf8'))
  .replace('/* AIKIT_MESSAGE_START */','await startMessagePump($);')
  .replace('/* AIKIT_MESSAGE_BUSY */','messageBridge?.busy();')
  .replace('/* AIKIT_MESSAGE_IDLE */','if (event.agentId === undefined && nativeTurnId === event.turnId) messageBridge?.idle();')
  .replace('/* AIKIT_MESSAGE_LIFECYCLE */','registerMessageLifecycle(on);')
  .replace("'AIKIT_NATIVE_TOOL_NAME'",JSON.stringify('mcp__aikit-lifecycle-module__review'))
  .replace('/* AIKIT_CONFIG */null',JSON.stringify(config));
fs.writeFileSync(p+'/.claude-plugin/plugin.json',JSON.stringify({name:config.pluginName,version:'0.0.1',description:'Disposable generated SDK lifecycle check'}));
fs.writeFileSync(p+'/hooks/hooks.json',JSON.stringify({modules:['./register.js']}));
fs.writeFileSync(p+'/hooks/register.js',source);
fs.writeFileSync('/home/node/handler.mjs',`import fs from 'node:fs';
let raw='';for await(const chunk of process.stdin)raw+=chunk;const frame=JSON.parse(raw);
const context=JSON.parse(process.env.AIKIT_INVOCATION_ENV_KEY??'null');
fs.appendFileSync('/home/node/evidence/handler.jsonl',JSON.stringify({kind:process.argv[2],frame,context})+'\\n');
if(process.argv[2]==='messages')console.log(JSON.stringify({schema_version:1,reference:{installation_id:'lifecycle-install',installation_revision:'fixture',
  native_session_id:frame.native_context.session_id,invocation_id:frame.native_context.invocation_id,start_cursor:1},recorded_idle:true,recorded_ready:false,reservation:null}));
else console.log('{}');
`);
console.log('Generated SDK module with fixed diagnostic handlers; no application acceptance claimed.');
