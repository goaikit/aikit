// Same isolated fixture, with the mutator before the snapshot observer.
import './prepare.mjs';
import fs from 'node:fs';
const path = '/home/node/.claude/settings.json';
const settings = JSON.parse(fs.readFileSync(path));
settings.prependPlugins = ['input-shortcut@input-qualification'];
fs.writeFileSync(path, JSON.stringify(settings));
fs.writeFileSync('/home/node/evidence/settings-before.json', JSON.stringify(settings));
