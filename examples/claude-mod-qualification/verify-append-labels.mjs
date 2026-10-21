// Independent check of six retained native ASCII framing cases.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
assert(process.argv[2], 'capture root required');
const root = path.resolve(process.argv[2]);
const requests = fs.readFileSync(path.join(root, 'label-requests.jsonl'), 'utf8').trim().split('\n').map(JSON.parse);
const cases = [];
for (const length of [21,22,23,24,25,35]) {
  const name = 'a'.repeat(length), filename = path.join(root, 'label-evidence', name + '.json');
  const bytes = fs.readFileSync(filename, 'utf8'), events = JSON.parse(bytes);
  assert.equal(events.length, 2);
  const append = events.find(e => e.kind === 'append_return').value;
  const returned = events.find(e => e.kind === 'submit_return').value;
  assert.equal(append.sessionId, returned.sessionId);
  assert.equal(returned.payload, `AIKIT_LABEL_BOUNDARY:${name}`);
  assert.equal(returned.result.text, returned.payload);
  assert.deepEqual(append.event.origin, {kind: 'plugin', name});
  assert.equal(append.result.uuid, append.event.uuid);
  const label = length > 24 ? 'a'.repeat(23) + '…' : name;
  const envelope = `The ${label} plugin sent a message:\n${returned.payload}`
    + "\n\nThis is how Claude Code surfaces a prompt a plugin submits between turns — it starts this turn in the user's place. Address the message above.";
  assert.deepEqual(append.result.message.content, [{type:'text',text:envelope}]);
  const transcript = path.join(root, 'label-transcripts', append.sessionId + '.jsonl');
  const rows = fs.readFileSync(transcript,'utf8').trim().split('\n').map(JSON.parse).filter(row => row.uuid === append.result.uuid);
  assert.equal(rows.length,1); assert.equal(rows[0].message.content,envelope);
  assert.deepEqual(rows[0].origin,{kind:'plugin',name});
  const sent = requests.filter(request => request.body?.tools?.length
    && request.body.messages?.filter(message => message.role === 'user').at(-1)?.content
      ?.some?.(block => block.type === 'text' && block.text === envelope));
  assert.equal(sent.length,1);
  assert.equal(JSON.parse(sent[0].body.metadata.user_id).session_id,append.sessionId);
  cases.push({name_length:length,native_label:label,session_id:append.sessionId,row_uuid:append.result.uuid,
    events_sha256:createHash('sha256').update(bytes).digest('hex')});
}
console.log(JSON.stringify({passed:true,scope:'Six Claude 2.1.296 Linux ASCII plugin-label cases',cases,
  other_versions_or_unicode_names_qualified:false,production_delivery_qualified:false},null,2));
