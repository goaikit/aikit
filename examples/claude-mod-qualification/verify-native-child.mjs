import fs from 'node:fs';
import assert from 'node:assert/strict';
export function verify(root, rust = false) {
  const read = name => JSON.parse(fs.readFileSync(`${root}/${name}`, 'utf8'));
  const expected = '356733f185fa988e6bf3aa9a555269fdd167982a2e2a80d3c8a78037bb92ad58';
  const native = read(rust ? 'native-child-rust.json' : 'native-child.json');
  assert.deepEqual(read('settings-before.json'), read('settings-after.json'));
  const debug = fs.readFileSync(`${root}/native-debug.log`, 'utf8');
  assert(/hooks module input-observer@input-qualification loaded \(worker, environment [0-9]+, tier prepend\)/.test(debug));
  assert(!debug.includes('hook failed: input-observer'));
  assert(!debug.includes('input-observer: session.start hook skipped'));
  assert.match(native.session_id, /^[a-f0-9-]{36}$/);
  const child = rust ? native.python : native.facts;
  assert.equal(child.peer_is_ancestor, true);
  assert.equal(child.peer.executable_sha256, expected);
  assert.equal(child.peer.uid, 1000);
  assert(child.peer.pid > 0);
  assert(BigInt(child.peer.start_ticks) > 0n);
  assert(child.caller_ancestry.length >= 2 && child.caller_ancestry.length <= 32);
  const ancestor = child.caller_ancestry.at(-1);
  assert.equal(ancestor.pid, child.peer.pid);
  assert.equal(ancestor.start_ticks, child.peer.start_ticks);
  const independent = read('independent-caller.json');
  assert.equal(independent.peer_is_ancestor, false);
  assert.deepEqual(independent.peer, child.peer);
  assert(!independent.caller_ancestry.some(row => row.pid === child.peer.pid));
  const after = read('after-exit.json');
  assert.equal(after.peer_unavailable, true);
  for (const row of [child, independent, after]) {
    assert.equal(row.token_read, false);
    assert.equal(row.message_sent, false);
  }
  if (rust) {
    assert.equal(native.accepted.exit_code, 0);
    const actual = native.accepted.facts;
    assert.equal(actual.pid, child.peer.pid);
    assert.equal(actual.uid, child.peer.uid);
    assert.equal(String(actual.start_ticks), child.peer.start_ticks);
    assert.equal(actual.executable_sha256, expected);
    assert.notEqual(native.wrong_hash.exit_code, 0);
    assert.equal(native.wrong_hash.facts, null);
    for (const file of ['independent-rust.json', 'after-exit-rust.json']) {
      const value = read(file);
      assert.notEqual(value.exit_code, 0);
      assert.equal(value.accepted, false);
    }
  }
  return { passed: true, session: native.session_id, rust_helper: rust,
    actual_native_ancestor: true, unrelated_caller_refused: true, ended_peer_unavailable: true,
    native_generation_qualified: false, effective_policy_qualified: false, windows_qualified: false };
}
if (process.argv[1]?.replaceAll('\\', '/').endsWith('/verify-native-child.mjs'))
  console.log(JSON.stringify(verify(process.argv[2], process.argv[3] === '--rust')));
