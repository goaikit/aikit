"""Verify retained disposable-fixture evidence; this never drives Claude."""
import json
import pathlib
import sys


def verify(root):
    def read(name):
        return json.loads((root / name).read_text())

    bindings = [json.loads(line) for line in (root / 'bindings.jsonl').read_text().splitlines()]
    assert len(bindings) == 1, 'this trial did not qualify native replacement'
    initial = bindings[0]
    assert initial['ordinary_refused'] is True and initial['inspected_admitted'] is True
    original, current = read('original-binding.json'), read('current-binding.json')
    assert original == current == initial['record']
    assert original['schema_version'] == 1
    assert original['process']['executable_sha256'] == '356733f185fa988e6bf3aa9a555269fdd167982a2e2a80d3c8a78037bb92ad58'
    assert original['process']['pid'] > 0 and original['process']['start_ticks'] > 0
    assert original['reference']['start_cursor'] > 0
    checks = read('command-checks.json')
    assert [row['command'] for row in checks] == ['caller-original', 'caller-current', 'caller-current']
    assert [row['exit_code'] for row in checks] == [0, 0, 1]
    assert all(row['truncated'] is False for row in checks)
    assert all(row['result'] == {'admitted': True, 'binding_id': original['binding_id']} for row in checks[:2])
    assert checks[2]['result'] is None
    assert (root / 'fixture-error.txt').read_text() == 'Detached'
    assert int((root / 'independent.exit').read_text()) != 0
    assert 'peer/ancestor identity is unavailable or inconsistent' in (root / 'independent.stderr').read_text()
    assert read('owner-detach.json') == {'binding_id': original['binding_id'], 'detached': True}
    final = read('final-binding.json')
    assert final == {'record': original, 'status': 'detached'}
    legacy = read('legacy-reader.json')
    assert legacy['old_reader_refused'] is True and legacy['sdk_logical_dump_unchanged'] is True
    debug = (root / 'native-debug.log').read_text()
    assert 'aikit-caller-binding-fixtu-ac830870@skills-dir loaded' in debug
    assert 'session.start settled' in debug
    return {'native_child_admitted': True, 'independent_refused': True,
            'revocation_enforced': True, 'history_retained': True,
            'legacy_reader_refused': True, 'native_replacement_qualified': False}


if __name__ == '__main__':
    print(json.dumps(verify(pathlib.Path(sys.argv[1])), sort_keys=True))
