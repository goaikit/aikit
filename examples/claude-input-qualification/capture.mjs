// Read-only capture after ordinary terminal exit. No native replay or mutation.
import fs from 'node:fs';
import assert from 'node:assert/strict';
const root='/home/node/evidence';
const events=JSON.parse(fs.readFileSync(root+'/input-events.json'));
const starts=events.filter(row=>row.kind==='session_start');assert.equal(starts.length,1);
const session=starts[0].value.session_id;assert.match(session,/^[a-f0-9-]{36}$/);
fs.copyFileSync(`/home/node/.claude/projects/-home-node-workspace/${session}.jsonl`,root+'/native-transcript.jsonl',fs.constants.COPYFILE_EXCL);
fs.copyFileSync('/home/node/.claude/settings.json',root+'/settings-after.json',fs.constants.COPYFILE_EXCL);
assert.deepEqual(JSON.parse(fs.readFileSync(root+'/settings-before.json')),JSON.parse(fs.readFileSync(root+'/settings-after.json')));
console.log(JSON.stringify({captured:true,session}));
