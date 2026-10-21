// Checks the explicit no-next competitor on top of the independent envelope check.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
assert(process.argv[2], 'evidence directory required');
const directory = path.resolve(process.argv[2]);
const verifier = fileURLToPath(new URL('./verify-append-envelope.mjs', import.meta.url));
const result = spawnSync(process.execPath, [verifier, directory], {encoding: 'utf8', timeout: 10000});
assert.ifError(result.error);
assert.equal(result.signal, null);
assert.equal(result.status, 0, result.stderr);
const base = JSON.parse(result.stdout);
const bytes = fs.readFileSync(path.join(directory, 'append-fabricated.json'), 'utf8');
const competitor = JSON.parse(bytes);
const events = JSON.parse(fs.readFileSync(path.join(directory, 'append-events.json'), 'utf8'));
const append = events.filter(event => event.kind === 'append_return');
assert.equal(append.length, 1);
assert.equal(competitor.called_next, false);
assert.equal(competitor.sessionId, base.session_id);
assert.deepEqual(competitor.event, append[0].value.event);
assert.equal(competitor.event.uuid, base.row_uuid);
assert.equal(competitor.returned.uuid, '00000000-0000-0000-0000-000000000000');
assert.notEqual(competitor.returned.uuid, base.row_uuid);
assert.deepEqual(competitor.returned.message, competitor.event.message);
const rows = fs.readFileSync(path.join(directory, 'transcript.jsonl'), 'utf8').trim().split('\n').map(JSON.parse);
assert.equal(rows.filter(row => row.uuid === competitor.returned.uuid).length, 0);
console.log(JSON.stringify({
  ...base, scope: 'One Claude 2.1.296 Linux no-next fabricated append receipt scenario',
  fabricated_uuid_accepted: false,
  hashes: {...base.hashes, 'append-fabricated.json': createHash('sha256').update(bytes).digest('hex')},
}, null, 2));
