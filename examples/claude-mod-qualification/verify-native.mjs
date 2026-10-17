// Verify actual retained native/client evidence, not an offline engine stub.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
const directory = process.argv[2];
const events = JSON.parse(fs.readFileSync(path.join(directory, 'native-events.json'), 'utf8'));
const requests = fs.readFileSync(path.join(directory, 'requests.jsonl'), 'utf8').trim().split('\n').map(JSON.parse);
const session = events.find(event => event.kind === 'session_start').value;
assert.equal(session.event.surface, 'terminal');
assert.equal(session.event.isInteractive, true);
const calls = events.filter(event => event.kind === 'process_stdin').map(event => event.value);
for (const text of ['Native command stdin 雪\n', 'Native tool stdin 雪\n']) {
  const input = JSON.stringify({ text, sessionId: session.sessionId });
  assert.equal(calls.filter(call => call.input === input && call.stdout === input && call.exitCode === 0).length, 1);
}
const start = events.find(event => event.kind === 'turn_start').value;
const end = events.find(event => event.kind === 'turn_complete').value;
assert.equal(start.text, 'AIKIT_NATIVE_TOOL_PROBE');
assert.equal(end.turnId, start.turnId);
assert.equal(end.reason, 'answer');
assert.equal(end.isAborted, false);
assert.equal(end.answer, 'Fixture answer');
const model = requests.filter(request => request.method === 'POST' && request.body?.tools?.some(tool => tool.name === 'mcp__aikit-mod-feasibility__echo'));
assert.equal(model.length, 2);
for (const request of model) assert.equal(JSON.parse(request.body.metadata.user_id).session_id, session.sessionId);
const results = model[1].body.messages.flatMap(message => Array.isArray(message.content) ? message.content : [])
  .filter(block => block.type === 'tool_result' && block.tool_use_id === 'fixture-native-echo');
assert.equal(results.length, 1);
assert.equal(results[0].content, JSON.stringify({ text: 'Native tool stdin 雪\n', sessionId: session.sessionId }));
assert.equal(model[0].body.messages.flatMap(message => Array.isArray(message.content) ? message.content : [])
  .filter(block => block.type === 'tool_result' && block.tool_use_id === 'fixture-native-echo').length, 0);
const hashes = Object.fromEntries(['native-events.json', 'requests.jsonl'].map(file =>
  [file, createHash('sha256').update(fs.readFileSync(path.join(directory, file))).digest('hex')]));
console.log(JSON.stringify({ passed: true, scope: 'Linux native terminal client with deterministic local gateway',
  sessionId: session.sessionId, turnId: start.turnId, nativeStdinCalls: calls.length, mainRequests: model.length,
  windowsQualified: false, realModelQualified: false, reviewDeliveryQualified: false, hashes }, null, 2));
