"""Disposable Linux parent/peer fact probe; no credentials or messages."""
import hashlib
import json
import os
import pathlib
import socket
import stat
import struct
import sys

assert os.getuid() == 1000
assert pathlib.Path('/home/node/workspace/.codelaya-qualification-only').is_file()
address = os.environ.get('CLAUDE_CODE_MESSAGING_SOCKET', '')
cache = pathlib.Path('/home/node/.aikit-child-socket')
if sys.argv[1:] == ['independent']:
    address = cache.read_text()
else:
    with open(os.open(cache, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), 'w') as file:
        file.write(address)
if address.startswith('uds:'):
    address = address[4:]
assert os.path.isabs(address)

def process(pid):
    root = pathlib.Path('/proc') / str(pid)
    fields = (root / 'stat').read_text().rsplit(')', 1)[1].split()
    return {'pid': pid, 'parent_pid': int(fields[1]), 'start_ticks': fields[19]}

try:
    metadata = os.stat(address, follow_symlinks=False)
    assert stat.S_ISSOCK(metadata.st_mode) and metadata.st_uid == os.getuid()
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
        client.settimeout(0.25)
        client.connect(address)
        pid, uid, gid = struct.unpack('3i', client.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))
        peer = process(pid)
        executable = pathlib.Path('/proc') / str(pid) / 'exe'
        peer.update(uid=uid, gid=gid, executable_sha256=hashlib.sha256(executable.read_bytes()).hexdigest())
        chain = []
        current = os.getpid()
        for _ in range(32):
            value = process(current)
            chain.append(value)
            if current == pid or value['parent_pid'] == 0:
                break
            current = value['parent_pid']
        assert peer['start_ticks'] == process(pid)['start_ticks']
        ancestor = any(row['pid'] == pid and row['start_ticks'] == peer['start_ticks'] for row in chain)
        print(json.dumps({'peer': peer, 'caller_ancestry': chain, 'peer_is_ancestor': ancestor, 'token_read': False, 'message_sent': False}))
except (OSError, AssertionError) as error:
    print(json.dumps({'peer_unavailable': True, 'error_type': type(error).__name__, 'token_read': False, 'message_sent': False}))
