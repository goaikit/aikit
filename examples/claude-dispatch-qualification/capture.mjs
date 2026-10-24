// Run after normal terminal exit; reads native evidence only.
import fs from 'node:fs';
import assert from 'node:assert/strict';
const evidence='/home/node/evidence';
const events=JSON.parse(fs.readFileSync(evidence+'/dispatch-events.json'));
const starts=events.filter(r=>r.kind==='session_start');assert.equal(starts.length,1);
const session=starts[0].value.session_id;assert.match(session,/^[0-9a-f-]{36}$/);
fs.copyFileSync(`/home/node/.claude/projects/-home-node-workspace/${session}.jsonl`,
  evidence+'/native-transcript.jsonl',fs.constants.COPYFILE_EXCL);
fs.copyFileSync('/home/node/.claude/settings.json',evidence+'/settings-after.json',fs.constants.COPYFILE_EXCL);
assert.deepEqual(JSON.parse(fs.readFileSync(evidence+'/settings-before.json')),
  JSON.parse(fs.readFileSync(evidence+'/settings-after.json')));
console.log(JSON.stringify({captured:true,session}));
