// Verifier sensitivity checks against a separately retained native capture.
// Usage: node tests/verify-append-envelope.test.mjs EVIDENCE_DIRECTORY
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import test from 'node:test';

assert(process.argv[2], 'native evidence directory required');
const source = path.resolve(process.argv[2]);
const verifier = fileURLToPath(new URL('../verify-append-envelope.mjs', import.meta.url));
const names = ['append-events.json', 'transcript.jsonl', 'requests.jsonl'];
const parseLines = bytes => bytes.trim().split('\n').map(JSON.parse);
const cases = [
  ['unchanged capture passes', null],
  ['foreign transcript session fails', files => {
    const rows = parseLines(files['transcript.jsonl']);
    rows.find(row => row.type === 'user' && row.uuid).sessionId = 'foreign';
    files['transcript.jsonl'] = rows.map(JSON.stringify).join('\n');
  }],
  ['forged acknowledgement UUID fails', files => {
    const events = JSON.parse(files['append-events.json']);
    events.find(event => event.kind === 'append_return').value.result.uuid = '00000000-0000-0000-0000-000000000000';
    files['append-events.json'] = JSON.stringify(events);
  }],
  ['rewritten stored payload fails', files => {
    const rows = parseLines(files['transcript.jsonl']);
    const row = rows.find(row => row.type === 'user' && row.uuid);
    assert.equal(typeof row.message.content, 'string');
    row.message.content = row.message.content.replace('preserve exact 雪', 'changed payload');
    files['transcript.jsonl'] = rows.map(JSON.stringify).join('\n');
  }],
  ['duplicate outgoing payload fails', files => {
    const requests = parseLines(files['requests.jsonl']);
    const messages = requests.find(request => request.body?.tools).body.messages;
    messages.push(structuredClone(messages.find(message => message.role === 'user')));
    files['requests.jsonl'] = requests.map(JSON.stringify).join('\n');
  }],
  ['missing wire payload fails', files => {
    files['requests.jsonl'] = files['requests.jsonl'].replaceAll('AIKIT_APPEND_CANDIDATE', 'REMOVED');
  }],
  ['aborted turn fails', files => {
    const events = JSON.parse(files['append-events.json']);
    events.find(event => event.kind === 'turn_complete').value.event.isAborted = true;
    files['append-events.json'] = JSON.stringify(events);
  }],
];
for (const [name, mutate] of cases) test(name, () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'aikit-envelope-verifier-'));
  try {
    const files = Object.fromEntries(names.map(file => [file, fs.readFileSync(path.join(source, file), 'utf8')]));
    mutate?.(files);
    for (const [file, bytes] of Object.entries(files)) fs.writeFileSync(path.join(directory, file), bytes);
    const result = spawnSync(process.execPath, [verifier, directory], {encoding: 'utf8', timeout: 10000});
    assert.ifError(result.error);
    assert.equal(result.signal, null);
    if (mutate) {
      assert.equal(result.status, 1, result.stdout + result.stderr);
      assert.match(result.stderr, /AssertionError/);
    } else {
      assert.equal(result.status, 0, result.stderr);
      const report = JSON.parse(result.stdout);
      assert.equal(report.passed, true);
      assert.equal(report.exact_payload_is_entire_stored_message, false);
      assert.equal(report.production_acknowledgement_qualified, false);
    }
  } finally {
    assert.equal(path.dirname(path.resolve(directory)), path.resolve(os.tmpdir()));
    assert(path.basename(directory).startsWith('aikit-envelope-verifier-'));
    fs.rmSync(directory, {recursive: true});
  }
});
