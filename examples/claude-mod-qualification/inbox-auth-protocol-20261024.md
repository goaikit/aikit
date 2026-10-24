# Existing inbox authentication and OS peer probe

Predeclared 2026-10-24. Inspect the documented exported per-session socket/token
before proposing a new native identity interface. This is a bounded Linux native
primitive investigation, not SDK production support or message consumption.

Use ordinary interactive Claude 2.1.308 in immutable image
`sha256:9312badacd0cdf5496308a7292a5dc9965d25c616dbda94764ee10bbe8c30924`,
network none, no mounts, fresh disposable node profile and public dummy API key.
No owner settings/credentials/policy or Windows checks. A local diagnostic's
SessionStart calls a Python standard-library child that reads its inherited
`CLAUDE_CODE_MESSAGING_SOCKET` and `CLAUDE_CODE_MESSAGING_TOKEN`. Never print,
persist or hash the token; no environment dump, real provider auth or model prompt.

Connect only to this disposable session's exported Unix socket. Capture kernel
SO_PEERCRED (PID/UID/GID), native executable hash and process-start ticks. Compare
three bounded connections: valid auth line, invalid auth line, and no auth line.
Do not send a message or idle subscription. Retain only response type/field names,
closure/timeout classification and nonsecret process/socket metadata, never raw
auth lines or arbitrary response text. Each connection times out after 750 ms.

Require actual kernel native peer identity before treating it as an OS fact.
An open connection alone is not an authentication acknowledgement. A successful
auth exchange is not original-session generation/effective handler-chain proof,
atomic admission, stored-message acknowledgement, consumption or durable lookup.
If a socket/token/runtime is absent, preserve that discovery failure unchanged;
do not silently change provider settings. Exit normally without a model prompt.

References: [official inbox/socket/token contract](https://code.claude.com/docs/en/cross-session-messaging#the-sessions-inbox-socket)
and exact installed 2.1.308 declarations (SHA-256
`3c29d70e7f113f2409db12c3db20dd8385296c46aaf26984d00e5ecacd37c2c0`).
