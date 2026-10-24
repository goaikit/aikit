import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
const directory = process.argv[2];
const spec = JSON.parse(fs.readFileSync(path.join(directory, 'spec.json'), 'utf8'));
const info = JSON.parse(fs.readFileSync(path.join(directory, 'plugin-info.json'), 'utf8'));
const requests = fs.readFileSync(path.join(directory, 'requests.jsonl'), 'utf8').trim().split('\n').map(JSON.parse)
  .filter(row => row.body?.tools?.some(tool => tool.name === info.tool_name));
assert.equal(requests.length, 2);
const blocks = requests[0].body.messages.flatMap(message => Array.isArray(message.content) ? message.content : []);
assert(blocks.some(block => block.type === 'text' && block.text.includes(spec.instructions)));
assert(blocks.some(block => block.type === 'text' && block.text.includes('OTHER_CONTEXT_MUST_STAY')));
const tool = requests[0].body.tools.find(tool => tool.name === info.tool_name);
assert.deepEqual(tool.input_schema.properties.request, spec.input_schema);
const results = requests[1].body.messages.flatMap(message => Array.isArray(message.content) ? message.content : [])
  .filter(block => block.type === 'tool_result' && block.tool_use_id === 'fixture-native-bridge');
assert.equal(results.length, 1);
const call = JSON.parse(results[0].content);
assert.equal(call.schema_version, 1);
assert.equal(call.application_id, spec.application_id);
assert.equal(call.installation_id, spec.installation_id);
assert.equal(call.agent_key, 'claude');
assert.equal(call.tool_name, spec.tool_name);
assert.equal(call.native_context.session_id, JSON.parse(requests[0].body.metadata.user_id).session_id);
assert.equal(call.native_context.session_id, JSON.parse(requests[1].body.metadata.user_id).session_id);
assert.match(call.native_context.invocation_id, /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
assert.equal(call.native_context.cwd, '/home/node/workspace');
assert.deepEqual(call.request, { text: 'Native SDK bridge stdin 雪\n' });
console.log(JSON.stringify({ passed: true, scope: 'SDK-generated bridge in Linux native terminal, scripted gateway',
  native_tool: info.tool_name, main_requests: requests.length, instructions_present: true,
  other_context_preserved: true, envelope_and_native_session_match: true,
  requests_sha256: createHash('sha256').update(fs.readFileSync(path.join(directory, 'requests.jsonl'))).digest('hex'),
  nativeCallerAuthenticated: false, codelayaOperationsQualified: false, windowsQualified: false }, null, 2));
