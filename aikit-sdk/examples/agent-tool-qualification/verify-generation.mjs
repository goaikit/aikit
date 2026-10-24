// Run after the ordinary native fixture and verify.mjs, before removal.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
const directory = process.argv[2];
const installed = JSON.parse(fs.readFileSync(path.join(directory, 'owned-install.json')));
const info = JSON.parse(fs.readFileSync(path.join(directory, 'plugin-info.json')));
const requests = fs.readFileSync(path.join(directory, 'requests.jsonl'), 'utf8').trim().split('\n').map(JSON.parse)
  .filter(row => row.body?.tools?.some(tool => tool.name === info.tool_name));
const result = requests[1].body.messages.flatMap(message => Array.isArray(message.content) ? message.content : [])
  .find(block => block.type === 'tool_result' && block.tool_use_id === 'fixture-native-bridge');
const call = JSON.parse(result.content);
const run = args => JSON.parse(execFileSync('/generated/integration_hooks', args, { encoding: 'utf8' }));
const journal = run(['events', '/home/node/owned-state', installed.installation_id]);
const starts = journal.records.filter(record => record.request.event === 'session_started');
assert.equal(starts.length, 1);
assert.equal(starts[0].request.invocation_id, call.native_context.invocation_id);
assert.equal(starts[0].request.session_id, call.native_context.session_id);
const binding = run(['bind', '/home/node/owned-state', installed.installation_id, call.native_context.session_id]);
assert.equal(binding.reference.invocation_id, call.native_context.invocation_id);
assert.equal(binding.reference.start_cursor, starts[0].cursor);
assert.equal(binding.status, 'observed');
fs.writeFileSync(path.join(directory, 'generation-verification.json'), JSON.stringify({
  passed: true, recorded_start_matches_native_call: true, reference: binding.reference,
  nativeCallerAuthenticated: false, windowsQualified: false,
}, null, 2));
console.log(JSON.stringify({ passed: true, recorded_start_matches_native_call: true,
  nativeCallerAuthenticated: false, windowsQualified: false }));
