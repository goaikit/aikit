// Record synthetic handler I/O, then reuse the SDK's real envelope reader.
import fs from 'node:fs';
import {execFileSync} from 'node:child_process';
let input='';
for await(const chunk of process.stdin){input+=chunk;if(Buffer.byteLength(input)>131072)throw Error('input ceiling');}
fs.appendFileSync('/home/node/guarded-evidence/handler-calls.jsonl',input+'\n');
process.stdout.write(execFileSync('/home/node/bin/agent_tool_plugin',['inspect'],{input,encoding:'utf8',maxBuffer:1024*1024}));
