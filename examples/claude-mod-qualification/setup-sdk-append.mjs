// Synthetic qualification controller. Uses SDK embedding examples; never starts
// an agent, edits an owner profile, supplies real credentials or projects proof.
import fs from 'node:fs';
import path from 'node:path';
import {execFileSync} from 'node:child_process';
const suffix = process.argv[3];
if (suffix !== undefined && !['final','crash'].includes(suffix)) throw new Error('only fixed final/crash fixture suffixes are supported');
const workspace = suffix ? `/home/node/append-client-${suffix}` : '/home/node/workspace';
const state = suffix ? `/home/node/sdk-state-${suffix}` : '/home/node/sdk-state';
const evidence = suffix ? `/home/node/evidence-${suffix}` : '/home/node/evidence', bin = '/home/node/bin';
const hooks = path.join(bin, 'integration_hooks');
const run = (args, input) => JSON.parse(execFileSync(hooks, args, {
  input: input === undefined ? undefined : JSON.stringify(input), encoding: 'utf8', maxBuffer: 8 * 1024 * 1024,
}));
const save = (name, value) => fs.writeFileSync(path.join(evidence, name), JSON.stringify(value, null, 2));
const action = process.argv[2];
if (action === 'setup') {
  fs.mkdirSync(workspace, {recursive: true}); fs.mkdirSync(evidence, {recursive: true});
  execFileSync('git', ['init', '--quiet', workspace]);
  const spec = {application_id: 'sdk-example', agent_key: 'claude', workspace,
    handler: {executable: hooks, arguments: ['hook', state, workspace, '0']},
    events: ['session_started', 'input_submitted', 'completion_proposed', 'session_ended'],
    timeout_seconds: 10};
  const plan = run(['plan', state], spec);
  run(['apply', state, plan.id]);
  const installation = plan.installation_id;
  const tool = {application_id: 'sdk-example', installation_id: installation, tool_name: 'review',
    description: 'Synthetic qualification tool; not used by the scripted endpoint',
    input_schema: {type: 'object'}, instructions: '',
    handler: {executable: path.join(bin, 'agent_tool_plugin'), arguments: ['inspect']}, timeout_ms: suffix === 'crash' ? 30000 : 5000};
  const bridge = suffix === 'crash'
    ? [process.execPath, '/home/node/probe/append-crash-bridge.mjs', state, installation]
    : [path.join(bin, 'integration_message_bridge'), state, installation];
  const owned = run(['plan-tool-message', state, installation, ...bridge], tool);
  run(['apply', state, owned.id]);
  const skills = fs.readdirSync(path.join(workspace, '.claude/skills'));
  if (skills.length !== 1) throw new Error('expected one owned module');
  const plugin = path.join(workspace, '.claude/skills', skills[0]);
  const manifest = JSON.parse(fs.readFileSync(path.join(plugin, '.claude-plugin/plugin.json'), 'utf8'));
  save('setup.json', {state, workspace, installation, plugin, plugin_name: manifest.name, plugin_version: manifest.version});
  console.log(JSON.stringify({installation, plugin}));
} else if (action === 'stage') {
  const setup = JSON.parse(fs.readFileSync(path.join(evidence, 'setup.json'), 'utf8'));
  const events = run(['events', state, setup.installation]);
  const completed = events.records.filter(record => record.request.event === 'native_turn_finished');
  if (completed.length !== 1 || completed[0].request.native_turn?.outcome?.reason !== 'answer')
    throw new Error('one actual successful initial native Turn required before staging');
  const session = completed[0].request.session_id;
  const binding = run(['bind', state, setup.installation, session]);
  const request = {operation_id: 'sdk-append-client-review', content: 'SDK_APPEND_CLIENT_REVIEW: exact 雪\nfeedback'};
  const receipt = run(['message-stage', state, binding.id], request);
  save('stage.json', {binding_id: binding.id, request, receipt, session_id: session});
  save('initial-events.json', events);
  console.log(JSON.stringify({session_id: session, binding_id: binding.id}));
} else if (action === 'recover') {
  const setup = JSON.parse(fs.readFileSync(path.join(evidence, 'setup.json'), 'utf8'));
  const stage = JSON.parse(fs.readFileSync(path.join(evidence, 'stage.json'), 'utf8'));
  const receipt = run(['message-recover', state, stage.binding_id, stage.request.operation_id]);
  save('recovered-receipt.json', receipt);
  console.log(JSON.stringify(receipt));
} else if (action === 'inspect') {
  const setup = JSON.parse(fs.readFileSync(path.join(evidence, 'setup.json'), 'utf8'));
  const stage = JSON.parse(fs.readFileSync(path.join(evidence, 'stage.json'), 'utf8'));
  const status = run(['message-status', state, stage.binding_id, stage.request.operation_id]);
  save('message-status.json', status);
  save('events.json', run(['events', state, setup.installation]));
  console.log(JSON.stringify(status));
} else throw new Error('usage: setup-sdk-append.mjs setup|stage|recover|inspect');
