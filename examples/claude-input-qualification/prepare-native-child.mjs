// Reuse the fresh isolated installer; replace diagnostics before validation/load.
import './prepare.mjs';
import fs from 'node:fs';
fs.copyFileSync('/qualification/native-child-observer.js', '/home/node/workspace/input-observer/hooks/register.js');
fs.writeFileSync('/home/node/workspace/input-shortcut/hooks/register.js', 'export function register() {}\n');
