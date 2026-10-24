// Disposable Linux fixture only. No owner profile, credentials or agent launch.
// Both hooks and tool installation delegate to the compiled SDK examples.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';

const evidence = process.argv[2];
assert(evidence && path.isAbsolute(evidence));
fs.mkdirSync(evidence, { recursive: true });
const workspace = '/home/node/workspace';
const state = '/home/node/owned-state';
const integration = (args, input) => JSON.parse(execFileSync('/generated/integration_hooks', args,
  { input: input && JSON.stringify(input), encoding: 'utf8', maxBuffer: 1024 * 1024 }));
const config = path.join(workspace, '.claude/settings.local.json');
assert(!fs.existsSync(config), 'use a fresh disposable workspace');
fs.mkdirSync(path.dirname(config), { recursive: true });
fs.writeFileSync(config, JSON.stringify({ env: { OTHER_FIXTURE_SETTING: 'keep' } }));
fs.writeFileSync(path.join(workspace, 'CLAUDE.md'), 'OTHER_CONTEXT_MUST_STAY');
const hooks = integration(['plan', state], {
  application_id: 'sdk-example', agent_key: 'claude', workspace,
  handler: { executable: '/generated/integration_hooks', arguments: ['hook', state, workspace, '0'] },
  events: ['session_started'], timeout_seconds: 10,
});
assert.equal(integration(['apply', state, hooks.id]).status, 'configured');
const spec = JSON.parse(fs.readFileSync('/qualification/spec.json', 'utf8'));
spec.application_id = 'sdk-example';
spec.installation_id = hooks.installation_id;
const tool = integration(['plan-tool', state, hooks.installation_id], spec);
assert.equal(tool.files.length, 3);
assert(tool.files.every(file => !fs.existsSync(file.path)));
assert.equal(integration(['apply', state, tool.id]).status, 'configured');
assert.equal(JSON.parse(fs.readFileSync(config)).env.OTHER_FIXTURE_SETTING, 'keep');
const info = JSON.parse(execFileSync('/generated/agent_tool_plugin', ['export', 'claude', path.join(evidence, 'rendered')],
  { input: JSON.stringify(spec), encoding: 'utf8' }));
const root = path.join(workspace, '.claude/skills', info.plugin_name);
for (const file of tool.files) {
  assert.equal(createHash('sha256').update(fs.readFileSync(file.path)).digest('hex'), file.after_fingerprint);
}
fs.writeFileSync(path.join(evidence, 'spec.json'), JSON.stringify(spec));
fs.writeFileSync(path.join(evidence, 'plugin-info.json'), JSON.stringify(info));
fs.writeFileSync(path.join(evidence, 'owned-install.json'), JSON.stringify({
  installation_id: hooks.installation_id, hook_plan: hooks.id, tool_plan: tool.id,
  root, files: tool.files, configured: true, nativeReady: false,
}, null, 2));
console.log(JSON.stringify({ configured: true, root, ...info, nativeReady: false }));
