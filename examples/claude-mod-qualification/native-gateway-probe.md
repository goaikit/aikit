# Native terminal / local gateway probe

This is a controlled Linux x86_64 qualification fixture, not a production
Codelaya adapter. Claude owns the real terminal session, mod loader, tool
dispatch, child process, conversation and HTTP requests. A local server supplies
deterministic Anthropic-format replies; no real model or provider credential is
used. No required Windows capability is promoted.

## Recorded result, 2026-10-17

- Native client: Claude Code 2.1.296; executable SHA-256
  `24972e3bc859fab2b46ed4c1e51f7d6130f06d3bd550811a114640de3370d0de`.
- Docker image: `sha256:fe24c3f19fe12410b4d57bb1101def27a05a92fa7083611623157a0a2b433e65`.
  The Dockerfile verifies the executable's digest during build.
- Normal onboarding approved the public fixture API key and this disposable
  workspace. First-run service connectivity needed temporary network access;
  networking was disabled again before accepting workspace trust/mod loading.
  An earlier demo-mode attempt did not grant workspace trust or load the mod.
- The final run was a new ordinary interactive session, without demo mode,
  managed-session launch, sign-in, real credentials or permission bypass flags.
  The container had only the `none` Docker network throughout that run.
- Generated `claude-code/index.d.ts` SHA-256:
  `ea634205532548aa90020ff3a941e570bff5bec8ae242fb96d09dbcc8fc1bfad`.
  Declarations and raw request evidence are retained locally, not redistributed.
- A native slash command and a registered model tool each round-tripped exact
  Unicode JSON through `$.process.run` stdin/stdout. Their native session ID
  matched the session-start observation and the requests' session metadata.
- The client sent two main API requests. The second contained the exact tool
  result once. A title helper also made an auxiliary fixture request; it is not
  counted as a main request or a tool call.
- `turn.start` / `turn.complete` shared a Turn ID. Completion carried
  `reason: answer`, `isAborted: false` and exact text `Fixture answer`.
- `verify-native.mjs` passed against independently retained native events and
  actual API request bodies. Four offline engine cases also passed after the
  aborted-completion fixture was updated to include the generated `reason` field.

The final request evidence SHA-256 is
`efdff9cafc2764048cb71a6a983464b78519d578fa4e1bcd0d4c53c15257ac9a`;
native event evidence SHA-256 is
`9710a39fab375b50ed7e8ee15233ae8afa79be94b7599692cf53fdc920b3d372`.
The local evidence directory is `.local/claude-native-gateway-evidence/final`
under the Codelaya implementation chat. Its `verification.json` records scope,
IDs and hashes. The canary is retained separately. Its gateway initially also
scripted a tool response for the title helper; the final source gates that
response on the declared tool, and the final ordinary-session run passed again.
The owned container was removed after retaining the evidence.

## What this resolves

The generated API confirms `process.run(argv, { stdin, timeoutMs })` and explicit
stdout/stderr truncation flags. A provider bridge can call Codelaya's existing
bounded-stdin operation service without adding an argument/file transport.
`tool.register` takes a description and input schema; the native client exposes
it through its own internal tool transport. This is separate from Codelaya's
optional read-only MCP server.

Generated completion reasons are `answer`, `aborted`, `refusal` and `error`.
`isAborted: false` alone therefore cannot establish successful completion.
The generated `turn.step` input has a count, not the complete API input; transcript
inspection is not an exact input proof. This fixture inspected actual outgoing
request bodies at its local endpoint to verify its tool-result input.

This does not qualify protected handler precedence, abort-after-Allow, native
caller authentication, original-session review delivery/reconciliation, effective
deadlines, semantic instruction following or Windows operation. It registers an
echo, not Codelaya's authoring operations, and installs nothing on the owner host.

## Reproduce

From the AIKit root, build the pinned image and use a new container name:

```powershell
$probe = (Resolve-Path examples/claude-mod-qualification).Path
docker build -f "$probe/Dockerfile.native" -t aikit-claude-native-probe:2.1.296 $probe
docker run -d --name aikit-native-fixture --label com.codelaya.task=local-native-gateway --cpus 2 --memory 3g --mount "type=bind,source=$probe,target=/input,readonly" --entrypoint sh aikit-claude-native-probe:2.1.296 -c 'sleep infinity'
docker exec aikit-native-fixture sh -c 'cp -a /input /home/node/probe; mkdir /home/node/evidence'
docker exec aikit-native-fixture node -e 'require("fs").writeFileSync("/home/node/probe/hooks/hooks.json",JSON.stringify({modules:["./native-probe.js"]}))'
docker exec -d aikit-native-fixture node /input/gateway.mjs /home/node/evidence
docker exec -it --env ANTHROPIC_BASE_URL=http://127.0.0.1:8787 --env ANTHROPIC_API_KEY=sk-ant-aikit-local-fixture-not-a-credential --env CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 --env CLAUDE_CODE_DISABLE_OFFICIAL_MARKETPLACE_AUTOINSTALL=1 aikit-native-fixture claude --plugin-dir /home/node/probe --debug-file /home/node/evidence/native-startup.log
```

In the new fixture home, select a theme, approve only the displayed public
fixture key and continue to the workspace trust screen. Before accepting trust,
use another terminal to remove external networking:

```powershell
docker network disconnect bridge aikit-native-fixture
docker network connect none aikit-native-fixture
```

Trust the fixture workspace. At the Claude prompt, run `/aikit-native-probe`,
then submit `AIKIT_NATIVE_TOOL_PROBE`. The local fixture scripts one echo tool
call followed by `Fixture answer`. After it finishes:

```powershell
docker exec aikit-native-fixture node /input/verify-native.mjs /home/node/evidence
docker inspect aikit-native-fixture --format '{{json .NetworkSettings.Networks}}'
```

Use `/exit` in Claude. Copy the selected regular evidence files with `docker cp`
and run the verifier on that retained directory before removing this owned
container. Copying a whole debug directory may try to create its `latest`
symlink on Windows; copy the named regular files instead. No Windows symlink
privilege is needed. On repeat runs in the same disposable home, a fresh Claude
session starts normally with networking disabled and without demo mode.

The gateway listens only on container loopback, does not proxy requests, accepts
only the public fixture key on POST, and bounds requests/body sizes. Its logs
contain complete synthetic API contexts, so use only a disposable workspace.

Official API references: [gateway endpoint configuration](https://code.claude.com/docs/en/env-vars),
[generated mod declarations](https://code.claude.com/docs/en/plugins/mods/create),
[native mod API](https://code.claude.com/docs/en/plugins/mods/api).
