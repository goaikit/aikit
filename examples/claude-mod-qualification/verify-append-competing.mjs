// Bounded native counterexample verifier, independent of the competing plugin.
// Proves this captured rewrite is visible, not general native delivery safety.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
assert(process.argv[2], 'evidence directory required');
const directory = path.resolve(process.argv[2]);
const read = name => fs.readFileSync(path.join(directory, name), 'utf8');
const jsonl = name => read(name).trim().split('\n').map(JSON.parse);
const events = JSON.parse(read('append-events.json'));
const competitor = JSON.parse(read('append-competitor.json'));
const rows = jsonl('transcript.jsonl'), requests = jsonl('requests.jsonl');
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
const original = 'AIKIT_APPEND_CANDIDATE: preserve exact 雪\nreview';
const replacement = 'AIKIT_APPEND_REWRITTEN: different bytes 雪\nreview';
const envelope = payload => 'The aikit-mod-feasibility plugin sent a message:\n' + payload
  + '\n\nThis is how Claude Code surfaces a prompt a plugin submits between turns — it starts this turn in the user\'s place. Address the message above.';
const start = one('session_start');
assert.equal(start.version.version, '2.1.296');
for (const [index, event] of events.entries()) {
  assert.equal(event.sequence, index + 1);
  assert.equal(event.value.sessionId, start.sessionId);
}
assert.equal(competitor.sessionId, start.sessionId);
assert.equal(competitor.original, original);
assert.equal(competitor.replacement, replacement);
assert.equal(events.filter(event => event.kind === 'submit_uncertain').length, 0);
assert.equal(one('submit_attempt').payload, original);
const origin = {kind: 'plugin', name: 'aikit-mod-feasibility'};
assert.deepEqual(one('submit_return').result, {text: original, origin});
const append = one('append_return');
assert.deepEqual(append.event.origin, origin);
assert.equal(append.event.door, 'prompt');
assert.equal(append.event.agentId, undefined);
assert.match(append.event.uuid, /^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/);
assert.deepEqual(competitor.event, append.event);
assert.equal(competitor.returned.uuid, append.event.uuid);
assert.equal(append.result.uuid, append.event.uuid);
assert.equal(competitor.downstream.uuid, append.event.uuid);
for (const message of [append.event.message, competitor.returned.message])
  assert.equal(text(message.content), envelope(original));
for (const message of [append.result.message, competitor.downstream.message]) {
  assert.equal(message.type, 'user');
  assert.equal(message.role, 'user');
  assert.equal(text(message.content), envelope(replacement));
}
const persisted = rows.filter(row => row.uuid === append.event.uuid);
assert.equal(persisted.length, 1);
assert.equal(persisted[0].sessionId, start.sessionId);
assert.equal(persisted[0].type, 'user');
assert.equal(persisted[0].message.role, 'user');
assert.deepEqual(persisted[0].origin, origin);
assert.equal(text(persisted[0].message.content), envelope(replacement));
const main = requests.filter(request => request.method === 'POST'
  && request.path.startsWith('/v1/messages') && !request.path.includes('count_tokens')
  && request.body?.tools?.some(tool => tool.name === 'mcp__aikit-mod-feasibility__append_probe'));
assert.equal(main.length, 1);
assert.equal(JSON.parse(main[0].body.metadata.user_id).session_id, start.sessionId);
const sent = main[0].body.messages.filter(message => message.role === 'user')
  .flatMap(message => typeof message.content === 'string'
    ? [message.content] : message.content.filter(block => block.type === 'text').map(block => block.text));
assert.equal(sent.filter(value => value === envelope(replacement)).length, 1);
const serialized = JSON.stringify(main[0].body.messages);
assert.equal(serialized.split(JSON.stringify(replacement).slice(1, -1)).length - 1, 1);
assert.equal(serialized.includes(JSON.stringify(original).slice(1, -1)), false);
// A native Turn-start observer still received the pre-rewrite text in this run.
const turn = one('turn_start').event, completed = one('turn_complete').event;
assert.equal(turn.text, envelope(original));
assert.equal(completed.turnId, turn.turnId);
assert.equal(completed.reason, 'answer');
assert.equal(completed.isAborted, false);
assert.equal(completed.answer, 'Fixture answer');
assert(completed.usage.input_tokens > 0 && completed.usage.output_tokens > 0);
assert.equal(one('session_end').event.sessionId, start.sessionId);
const sha = bytes => createHash('sha256').update(bytes).digest('hex');
console.log(JSON.stringify({
  passed: true, scope: 'One Claude 2.1.296 Linux competing append rewrite/false-return scenario',
  session_id: start.sessionId, row_uuid: append.event.uuid, main_requests: main.length,
  fabricated_return_concealed_rewrite: false,
  submit_return_and_turn_start_match_actual_sent_payload: false,
  original_payload_acknowledged: false,
  production_acknowledgement_qualified: false, production_consumption_qualified: false,
  windows_qualified: false, sdk_receipt_mutated: false,
  hashes: Object.fromEntries(['append-events.json', 'append-competitor.json', 'transcript.jsonl', 'requests.jsonl']
    .map(name => [name, sha(read(name))]))
}, null, 2));
