"""Disposable Linux native socket probe. Never emits the inherited token."""
import hashlib
import json
import os
import pathlib
import socket
import stat
import struct

workspace = pathlib.Path('/home/node/workspace')
assert (workspace / '.aikit-qualification-only').is_file()
assert os.getuid() == 1000
address = os.environ.get('CLAUDE_CODE_MESSAGING_SOCKET', '')
token = os.environ.get('CLAUDE_CODE_MESSAGING_TOKEN')
if address.startswith('uds:'):
    address = address[4:]
if not address or not token:
    print(json.dumps({'socket_present': bool(address), 'token_present': bool(token), 'probe': 'unavailable'}))
    raise SystemExit(0)
assert os.path.isabs(address)
socket_stat = os.stat(address, follow_symlinks=False)
assert stat.S_ISSOCK(socket_stat.st_mode)
assert socket_stat.st_uid == os.getuid()
results = []
for mode in ['valid', 'invalid', 'none']:
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
        client.settimeout(0.75)
        client.connect(address)
        pid, uid, gid = struct.unpack('3i', client.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))
        process = pathlib.Path('/proc') / str(pid)
        executable = process / 'exe'
        process_stat = (process / 'stat').read_text()
        # Fields after the closing comm ')' start at proc field 3; starttime is 22.
        start_ticks = process_stat.rsplit(')', 1)[1].split()[19]
        result = {'mode': mode, 'peer_pid': pid, 'peer_uid': uid, 'peer_gid': gid,
                  'peer_start_ticks': start_ticks,
                  'peer_executable_sha256': hashlib.sha256(executable.read_bytes()).hexdigest()}
        if mode != 'none':
            line = {'type': 'auth', 'token': token if mode == 'valid' else 'aikit-deliberately-invalid-token'}
            client.sendall((json.dumps(line) + '\n').encode())
        try:
            response = client.recv(8193)
            if not response:
                result['observation'] = 'closed'
            elif len(response) > 8192:
                result['observation'] = 'oversized-response'
            else:
                result['observation'] = 'response'
                try:
                    value = json.loads(response.split(b'\n', 1)[0])
                    result['response_fields'] = sorted(value.keys()) if isinstance(value, dict) else []
                    kind = value.get('type') if isinstance(value, dict) else None
                    # Only known protocol tags; no arbitrary provider text.
                    result['response_type'] = kind if kind in ['auth_ok', 'auth_error', 'error', 'ok'] else 'other'
                except (ValueError, UnicodeDecodeError):
                    result['observation'] = 'unclassified-response'
        except socket.timeout:
            result['observation'] = 'open-until-observer-timeout'
        results.append(result)
print(json.dumps({'socket_present': True, 'token_present': True, 'probe': 'observed',
                  'socket_uid': socket_stat.st_uid, 'connections': results}))
