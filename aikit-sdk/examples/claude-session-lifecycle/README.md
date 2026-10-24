# Disposable Claude session lifecycle check

This probes the generated SDK module in ordinary interactive Linux Claude
2.1.308. The journal handler and model gateway are diagnostic fixtures. It does
not qualify Codelaya, native handler authority, durable consumption, or Windows.
Read [the protocol](protocol.md) before starting; preserve failed trials.

Use a fresh container with Node 22 and the independently pinned Claude binary,
`--network none`, user `node`, and no host mounts. Its working directory must be
`/home/node/workspace`. Do not run these scripts against an owner profile.
The preparation refuses an existing `/home/node/.claude.json`.

1. Copy this directory to `/home/node/qualification` and the SDK's
   `src/integration` directory to `/home/node/integration` in the fresh container.
2. Run `node /home/node/qualification/prepare-home.mjs`, then
   `node /home/node/qualification/compose.mjs`, checking both exit codes.
   Initialize the disposable workspace with `git init /home/node/workspace`.
3. Start `node /home/node/qualification/gateway.mjs` as a separate container
   process. It listens on loopback port 8787 and allows twelve bounded requests.
4. Validate `/home/node/provider` with `claude plugin validate`. Start ordinary
   interactive `claude --plugin-dir /home/node/provider --plugin-dir /home/node/probe`
   using `ANTHROPIC_BASE_URL=http://127.0.0.1:8787` and public dummy
   `ANTHROPIC_API_KEY=sk-ant-aikit-local-fixture-not-a-credential`.
   Set `DISABLE_AUTOUPDATER=1` and `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`.
   Trust only this disposable workspace at its native prompt.
5. Submit `LIFECYCLE_INITIAL`; wait for completion. Read its session ID from
   the first `SessionStart` in `/home/node/evidence/handler.jsonl`.
6. Run `/clear`, submit `LIFECYCLE_AFTER_CLEAR`, and wait for completion.
7. Run `/resume INITIAL_SESSION_ID`, submit `LIFECYCLE_AFTER_RESUME`, wait for
   completion, and run `/exit`.
8. Run `node /home/node/qualification/verify.mjs`. Retain evidence, generated
   source, transcript and exact binary/source hashes. Stop the container without
   deleting it. Never count an intermediate or failed trial as final evidence.

The fixed diagnostic handlers retain only their synthetic frames and their
own invocation context. They do not read provider tokens or owner credentials.
