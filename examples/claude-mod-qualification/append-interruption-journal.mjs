// Disposable diagnostic journal only, not an SDK transport or application store.
// Fixed paths, exclusive durable checkpoints; no network, credentials or retries.
import fs from 'node:fs';
import path from 'node:path';
const root = '/home/node/evidence';
const action = process.argv[2];
if (action === 'hold') {
  await new Promise(resolve => setTimeout(resolve, 60000));
} else {
  if (!['claim', 'prepared', 'stored', 'resumed'].includes(action)) throw new Error('unknown checkpoint');
  const input = fs.readFileSync(0);
  if (input.length > 131072) throw new Error('checkpoint exceeds limit');
  const value = JSON.parse(input.toString('utf8'));
  fs.mkdirSync(root, {recursive: true});
  const file = path.join(root, action + '.json');
  const fd = fs.openSync(file, 'wx', 0o600);
  try { fs.writeFileSync(fd, JSON.stringify(value)); fs.fsyncSync(fd); }
  finally { fs.closeSync(fd); }
  const directory = fs.openSync(root, 'r');
  try { fs.fsyncSync(directory); } finally { fs.closeSync(directory); }
  console.log(JSON.stringify({written: action}));
}
