// Disposable native policy diagnostic. Run only inside its isolated container.
import fs from 'node:fs';
import path from 'node:path';

const mode = process.argv[2];
if (!['user', 'managed'].includes(mode)) throw new Error('user or managed case required');
const root = '/opt/aikit-prepend-probe';
const plugin = path.join(root, 'plugins/stop-guard');
fs.mkdirSync(path.join(root, '.claude-plugin'), { recursive: true });
fs.mkdirSync(path.join(plugin, '.claude-plugin'), { recursive: true });
fs.mkdirSync(path.join(plugin, 'hooks'), { recursive: true });
fs.writeFileSync(path.join(root, '.claude-plugin/marketplace.json'), JSON.stringify({
  name: 'aikit-prepend-probe', owner: { name: 'AIKit diagnostic' },
  plugins: [{ name: 'stop-guard', source: './plugins/stop-guard', description: 'Disposable Stop ordering diagnostic' }],
}));
fs.writeFileSync(path.join(plugin, '.claude-plugin/plugin.json'), JSON.stringify({
  name: 'stop-guard', version: '0.0.1', description: 'Disposable Stop ordering diagnostic', author: { name: 'AIKit' },
}));
fs.writeFileSync(path.join(plugin, 'hooks/hooks.json'), JSON.stringify({ modules: ['./stop-catch.js'] }));
fs.copyFileSync('/home/node/probe/hooks/stop-catch.js', path.join(plugin, 'hooks/stop-catch.js'));
const competitor = '/home/node/competitor';
fs.mkdirSync(path.join(competitor, '.claude-plugin'), { recursive: true });
fs.mkdirSync(path.join(competitor, 'hooks'), { recursive: true });
fs.writeFileSync(path.join(competitor, '.claude-plugin/plugin.json'), JSON.stringify({
  name: 'stop-competitor', version: '0.0.1', description: 'Disposable short-circuit competitor', author: { name: 'AIKit' },
}));
fs.writeFileSync(path.join(competitor, 'hooks/hooks.json'), JSON.stringify({ modules: ['./short-circuit-stop.js'] }));
fs.copyFileSync('/home/node/probe/hooks/short-circuit-stop.js', path.join(competitor, 'hooks/short-circuit-stop.js'));
const settings = {
  extraKnownMarketplaces: { 'aikit-prepend-probe': { source: { source: 'directory', path: root } } },
  enabledPlugins: { 'stop-guard@aikit-prepend-probe': true },
  prependPlugins: ['stop-guard@aikit-prepend-probe', ...(mode === 'managed' ? ['sec-default@builtin'] : [])],
};
fs.mkdirSync('/home/node/.claude', { recursive: true });
if (mode === 'managed') {
  fs.mkdirSync('/etc/claude-code', { recursive: true });
  fs.writeFileSync('/etc/claude-code/managed-settings.json', JSON.stringify(settings));
  fs.writeFileSync('/home/node/.claude/settings.json', '{}');
} else fs.writeFileSync('/home/node/.claude/settings.json', JSON.stringify(settings));
fs.writeFileSync('/home/node/evidence/case.txt', 'throw');
fs.writeFileSync(`/home/node/evidence/${mode}-settings.json`, JSON.stringify(settings));
