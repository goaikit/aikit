// Independent retained evidence check. No receipt/state write or native resend.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
assert(process.argv[2], 'evidence directory required');
const root = path.resolve(process.argv[2]);
const read = name => fs.readFileSync(path.join(root, name), 'utf8');
const json = name => JSON.parse(read(name));
const rows = name => read(name).trim().split('\n').map(JSON.parse);
const claim = json('claim.json'), prepared = json('prepared.json'), stored = json('stored.json');
const resumed = json('resumed.json'), killed = json('killed-state.json');
const payload = 'AIKIT_APPEND_INTERRUPTION: preserve exact 雪\nreview';
const envelope = 'The aikit-mod-feasibility plugin sent a message:\n' + payload
  + '\n\nThis is how Claude Code surfaces a prompt a plugin submits between turns — it starts this turn in the user\'s place. Address the message above.';
assert.equal(claim.payload, payload);
assert.equal(claim.operationId, 'synthetic-review-interruption-once');
for (const value of [prepared, stored, resumed]) {
  assert.equal(value.sessionId, claim.sessionId);
  assert.equal(value.operationId, claim.operationId);
}
assert.equal(killed.Status, 'exited'); assert.equal(killed.Running, false);
assert.equal(killed.ExitCode, 137); assert.equal(killed.OOMKilled, false);
assert.equal(prepared.event.uuid, stored.result.uuid);
assert.match(stored.result.uuid, /^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/);
assert.equal(prepared.payload, payload);
assert.deepEqual(prepared.event.origin, {kind: 'plugin', name: 'aikit-mod-feasibility'});
assert.equal(prepared.event.door, 'prompt'); assert.equal(prepared.event.agentId, undefined);
assert.deepEqual(stored.result.message, prepared.event.message);
assert.equal(stored.result.message.content.length, 1);
assert.deepEqual(stored.result.message.content[0], {type: 'text', text: envelope});
for (const name of ['after-kill-transcript.jsonl', 'after-resume-transcript.jsonl']) {
  const all = rows(name), kept = all.filter(row => row.uuid === stored.result.uuid);
  assert.equal(kept.length, 1);
  assert.equal(kept[0].sessionId, claim.sessionId);
  assert.equal(kept[0].type, 'user'); assert.equal(kept[0].message.role, 'user');
  assert.equal(kept[0].message.content, envelope);
  assert.deepEqual(kept[0].origin, prepared.event.origin);
  assert.equal(all.filter(row => row.type === 'queue-operation' && row.operation === 'enqueue'
    && row.content === payload).length, 1);
}
const before = json('before-kill-events.json'), after = json('resume-events.json');
assert.deepEqual(before.map(event => event.kind), ['session_start', 'submit_attempt']);
assert.deepEqual(after.map(event => event.kind), ['session_start', 'resumed_without_submit']);
for (const event of [...before, ...after]) assert.equal(event.value.sessionId, claim.sessionId);
assert.equal(resumed.messages.filter(message => message.role === 'user' && message.text === envelope).length, 1);
// This native view has no stable row UUID. It cannot independently do UUID lookup.
assert(resumed.messages.every(message => !Object.hasOwn(message, 'uuid')));
assert.deepEqual(json('request-presence.json'), {
  before_kill_request_log_exists: false, after_resume_request_log_exists: false,
});
const names = ['claim.json', 'prepared.json', 'stored.json', 'before-kill-events.json',
  'after-kill-transcript.jsonl', 'resumed.json', 'resume-events.json', 'after-resume-transcript.jsonl',
  'request-presence.json', 'killed-state.json'];
console.log(JSON.stringify({passed: true,
  scope: 'One Linux native SIGKILL after append storage before submit return, followed by original-session resume',
  session_id: claim.sessionId, row_uuid: stored.result.uuid,
  submit_return_observed: false, duplicate_enqueue_observed: false,
  native_messages_view_exposes_row_uuid: false, model_request_observed: false,
  production_acknowledgement_qualified: false, production_consumption_qualified: false,
  windows_qualified: false, sdk_receipt_mutated: false,
  hashes: Object.fromEntries(names.map(name => [name, createHash('sha256').update(read(name)).digest('hex')])),
}, null, 2));
