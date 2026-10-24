// Uses the actual SDK owned planner. Disposable home only; never starts Claude.
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
const mode = process.argv[2];
if (!['baseline', 'invalid', 'timeout'].includes(mode)) throw new Error('invalid case');
const workspace = `/home/node/workspace-${mode}`, state = `/home/node/state-${mode}`;
const evidence = `/home/node/evidence/${mode}`, hooks = '/home/node/bin/integration_hooks';
fs.mkdirSync(workspace, { recursive: true });
fs.mkdirSync(evidence, { recursive: true });
execFileSync('git', ['init', '--quiet', workspace]);
const run = (args, value) => JSON.parse(execFileSync(hooks, args, { encoding: 'utf8',
  input: value === undefined ? undefined : JSON.stringify(value), maxBuffer: 8 * 1024 * 1024 }));
const spec = { application_id: 'sdk-example', agent_key: 'claude', workspace,
  handler: { executable: process.execPath, arguments: ['/home/node/probe/generated-stop-handler.mjs', state, workspace, evidence, mode] },
  events: ['session_started', 'input_submitted', 'completion_proposed', 'session_ended'], timeout_seconds: 10 };
const plan = run(['plan', state], spec);
run(['apply', state, plan.id]);
const tool = { application_id: 'sdk-example', installation_id: plan.installation_id,
  tool_name: 'review', description: 'Synthetic qualification operation', input_schema: { type: 'object' }, instructions: '',
  handler: { executable: '/home/node/bin/agent_tool_plugin', arguments: ['inspect'] },
  timeout_ms: 1000, native_completion_bridge: true };
const owned = run(['plan-tool', state, plan.installation_id], tool);
run(['apply', state, owned.id]);
const skills = fs.readdirSync(path.join(workspace, '.claude/skills'));
if (skills.length !== 1) throw new Error('one owned plugin required');
const plugin = path.join(workspace, '.claude/skills', skills[0]);
const manifest = JSON.parse(fs.readFileSync(path.join(plugin, '.claude-plugin/plugin.json'), 'utf8'));
fs.mkdirSync(path.join(workspace, '.claude-plugin'), { recursive: true });
const marketplace = `generated-stop-${mode}`;
fs.writeFileSync(path.join(workspace, '.claude-plugin/marketplace.json'), JSON.stringify({
  name: marketplace, owner: { name: 'AIKit diagnostic' },
  plugins: [{ name: manifest.name, source: `./.claude/skills/${skills[0]}`, description: 'SDK-generated Stop qualification' }],
}));
const id = `${manifest.name}@${marketplace}`;
const settings = { extraKnownMarketplaces: { [marketplace]: { source: { source: 'directory', path: workspace } } },
  enabledPlugins: { [id]: true }, prependPlugins: [id] };
fs.mkdirSync('/home/node/.claude', { recursive: true });
fs.writeFileSync('/home/node/.claude/settings.json', JSON.stringify(settings));
fs.writeFileSync(path.join(evidence, 'setup.json'), JSON.stringify({ workspace, state, installation: plan.installation_id,
  plugin, plugin_name: manifest.name, plugin_version: manifest.version, settings, spec, tool, plan, owned }, null, 2));
console.log(JSON.stringify({ workspace, plugin, id }));
