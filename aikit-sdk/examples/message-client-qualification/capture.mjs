import fs from 'node:fs';
import {execFileSync} from 'node:child_process';
const evidence=process.argv[2] || '/home/node/evidence',label=process.argv[3] || 'live';
if(!/^[a-z][a-z0-9-]*$/.test(label))throw Error('invalid capture label');
const setup=JSON.parse(fs.readFileSync(`${evidence}/setup.json`));
const staged=JSON.parse(fs.readFileSync(`${evidence}/staged.json`));
const run=args=>JSON.parse(execFileSync('/generated/integration_hooks',args,{encoding:'utf8',maxBuffer:4*1024*1024}));
const status=run(['message-status',setup.state,staged.binding.id,staged.receipt.request.operation_id]);
const events=run(['events',setup.state,setup.installation_id]);
fs.writeFileSync(`${evidence}/${label}-status.json`,JSON.stringify(status,null,2));
fs.writeFileSync(`${evidence}/${label}-events.json`,JSON.stringify(events,null,2));
console.log(JSON.stringify({state:status.receipt.state,handoff:status.unqualified_handoff!==null,
  starts:events.records.filter(r=>r.request.event==='native_turn_started').length,
  finishes:events.records.filter(r=>r.request.event==='native_turn_finished').length}));
