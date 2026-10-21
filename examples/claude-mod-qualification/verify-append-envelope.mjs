// Separate hypothesis from verify-append.mjs: the native row has a provider envelope.
// This checks a retained synthetic capture, not a production receipt or protected send.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';

assert(process.argv[2], 'evidence directory required');
const directory = path.resolve(process.argv[2]);
const read = name => fs.readFileSync(path.join(directory, name), 'utf8');
const events = JSON.parse(read('append-events.json'));
const lines = name => read(name).trim().split('\n').map(line => JSON.parse(line));
const rows = lines('transcript.jsonl');
const requests = lines('requests.jsonl');
const one = kind => {
  const selected = events.filter(event => event.kind === kind);
  assert.equal(selected.length, 1, kind);
  return selected[0].value;
};
const text = content => {
  if (typeof content === 'string') return content;
  assert(Array.isArray(content));
  assert(content.every(block => block.type === 'text' && typeof block.text === 'string'));
  return content.map(block => block.text).join('');
};
const payload = 'AIKIT_APPEND_CANDIDATE: preserve exact 雪\nreview';
const envelope = 'The aikit-mod-feasibility plugin sent a message:\n' + payload
  + '\n\nThis is how Claude Code surfaces a prompt a plugin submits between turns — it starts this turn in the user\'s place. Address the message above.';
const start = one('session_start');
assert.equal(start.version.version, '2.1.296');
assert.match(start.sessionId, /^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/);
for (const [index, event] of events.entries()) {
  assert.equal(event.sequence, index + 1);
  assert.equal(event.value.sessionId, start.sessionId);
}
assert.equal(events.filter(event => event.kind === 'submit_uncertain').length, 0);
const attempt = one('submit_attempt'), returned = one('submit_return'), append = one('append_return');
assert.equal(attempt.payload, payload);
assert.equal(returned.result.text, payload);
assert.deepEqual(returned.result.origin, {kind: 'plugin', name: 'aikit-mod-feasibility'});
assert.deepEqual(append.event.origin, returned.result.origin);
assert.equal(append.event.door, 'prompt');
assert.equal(append.event.agentId, undefined);
assert.equal(append.result.uuid, append.event.uuid);
assert.match(append.result.uuid, /^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/);
for (const message of [append.event.message, append.result.message]) {
  assert.equal(message.type, 'user');
  assert.equal(message.role, 'user');
  assert.equal(text(message.content), envelope);
}
const persisted = rows.filter(row => row.uuid === append.result.uuid);
assert.equal(persisted.length, 1);
const row = persisted[0];
assert.equal(row.sessionId, start.sessionId);
assert.deepEqual(row.origin, returned.result.origin);
assert.equal(row.isSidechain, false);
assert.equal(row.type, 'user');
assert.equal(row.message.role, 'user');
assert.equal(text(row.message.content), envelope);
const main = requests.filter(request => request.method === 'POST'
  && request.path.startsWith('/v1/messages') && !request.path.includes('count_tokens')
  && request.body?.tools?.some(tool => tool.name === 'mcp__aikit-mod-feasibility__append_probe'));
assert.equal(main.length, 1);
assert.equal(JSON.parse(main[0].body.metadata.user_id).session_id, start.sessionId);
// Native wire input may prepend independent system-reminder blocks. Match the
// complete envelope as one text block; do not call the entire request identical.
const sent = main[0].body.messages.filter(message => message.role === 'user')
  .flatMap(message => typeof message.content === 'string'
    ? [message.content] : message.content.filter(block => block.type === 'text').map(block => block.text))
  .filter(value => value === envelope);
assert.equal(sent.length, 1);
const serialized = JSON.stringify(main[0].body.messages);
assert.equal(serialized.split(JSON.stringify(payload).slice(1, -1)).length - 1, 1);
const turn = one('turn_start').event, completed = one('turn_complete').event;
assert.equal(turn.text, envelope);
assert.equal(completed.turnId, turn.turnId);
assert.equal(completed.reason, 'answer');
assert.equal(completed.isAborted, false);
assert.equal(completed.answer, 'Fixture answer');
assert(completed.usage.input_tokens > 0 && completed.usage.output_tokens > 0);
assert.equal(one('session_end').event.sessionId, start.sessionId);
const sha = bytes => createHash('sha256').update(bytes).digest('hex');
console.log(JSON.stringify({
  passed: true, scope: 'Claude 2.1.296 Linux native provider-envelope candidate with scripted replies',
  session_id: start.sessionId, row_uuid: row.uuid, main_requests: main.length,
  exact_payload_is_entire_stored_message: false,
  exact_payload_present_in_verified_provider_envelope: true,
  payload_sha256: sha(payload), stored_envelope_sha256: sha(envelope),
  windows_qualified: false, production_acknowledgement_qualified: false,
  production_consumption_qualified: false, sdk_receipt_mutated: false,
  hashes: Object.fromEntries(['append-events.json', 'transcript.jsonl', 'requests.jsonl'].map(name => [name, sha(read(name))]))
}, null, 2));
