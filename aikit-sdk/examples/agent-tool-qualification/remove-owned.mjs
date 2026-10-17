// Run after exiting the fixture's interactive Claude session.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
const evidence = process.argv[2];
assert(evidence && path.isAbsolute(evidence));
const installed = JSON.parse(fs.readFileSync(path.join(evidence, 'owned-install.json')));
const integration = args => JSON.parse(execFileSync('/generated/integration_hooks', args,
  { encoding: 'utf8', maxBuffer: 1024 * 1024 }));
const list = () => JSON.parse(execFileSync('claude', ['plugin', 'list', '--json'], { encoding: 'utf8' }));
const before = list();
const discovered = before.find(plugin => plugin.installPath === installed.root);
assert(discovered?.enabled);
assert.equal(discovered.scope, 'project');
const unrelated = '/home/node/workspace/.claude/skills/user-owned.txt';
fs.writeFileSync(unrelated, 'preserve exact');
const plan = integration(['remove-plan', '/home/node/owned-state', installed.installation_id]);
assert.equal(plan.files.length, 3);
assert.equal(integration(['apply', '/home/node/owned-state', plan.id]).status, 'absent');
assert(!fs.existsSync(installed.root));
assert.equal(fs.readFileSync(unrelated, 'utf8'), 'preserve exact');
assert.deepEqual(JSON.parse(fs.readFileSync('/home/node/workspace/.claude/settings.local.json')),
  { env: { OTHER_FIXTURE_SETTING: 'keep' } });
assert.equal(fs.readFileSync('/home/node/workspace/CLAUDE.md', 'utf8'), 'OTHER_CONTEXT_MUST_STAY');
const after = list();
assert(!after.some(plugin => plugin.installPath === installed.root));
fs.writeFileSync(path.join(evidence, 'owned-removal.json'), JSON.stringify({
  passed: true, before, after, unrelated_files_preserved: true,
  nativeCallerAuthenticated: false, codelayaOperationsQualified: false, windowsQualified: false,
}, null, 2));
console.log(JSON.stringify({ passed: true, discovered_without_flags: true, removed: true,
  unrelated_files_preserved: true, windowsQualified: false }));
