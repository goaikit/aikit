// Disposable native framing setup only. Creates a fresh fixed workspace; does
// not start an agent, touch owner settings, or modify SDK message receipts.
import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
const here=path.dirname(fileURLToPath(import.meta.url));
const workspace='/home/node/labels-workspace';
const evidence='/home/node/label-evidence';
if(fs.existsSync(workspace)||fs.existsSync(evidence))throw new Error('fresh disposable framing workspace required');
fs.mkdirSync(evidence);
for(const length of [21,22,23,24,25,35]){
  const name='a'.repeat(length),root=path.join(workspace,'.claude/skills',name);
  fs.mkdirSync(path.join(root,'.claude-plugin'),{recursive:true});
  fs.mkdirSync(path.join(root,'hooks'));
  fs.writeFileSync(path.join(root,'.claude-plugin/plugin.json'),JSON.stringify({name,version:'0.1.0',description:'Explicit synthetic native framing case'}));
  fs.writeFileSync(path.join(root,'hooks/hooks.json'),JSON.stringify({modules:['./register.js']}));
  fs.copyFileSync(path.join(here,'hooks/append-label-boundary.js'),path.join(root,'hooks/register.js'));
}
console.log(JSON.stringify({workspace,evidence,lengths:[21,22,23,24,25,35]}));
