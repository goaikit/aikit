# SDK-generated native tool qualification

See the [public contract](../../agent-tools.md). This fixture uses the library's
generated plugin and compiled Rust decoder, not a hand-written echo module.
The gateway scripts model replies; it does not test model judgment or Codelaya
operations. Use a disposable workspace without owner configuration/credentials.

## Recorded evidence, 2026-10-17

Claude Code 2.1.296 Linux x86_64 executable SHA-256:
`24972e3bc859fab2b46ed4c1e51f7d6130f06d3bd550811a114640de3370d0de`.
Native client image:
`sha256:fe24c3f19fe12410b4d57bb1101def27a05a92fa7083611623157a0a2b433e65`.
Rust inspector executable:
`6642804474103940b585ad97e24748c36bce5f730d332ba20023acd55cfb6d90`.
Actual API request evidence:
`4ce3bc36744ceadb6552f9050a6ebf29506c2637b526cc51ab248a9c114e37d5`.

The final ordinary interactive session ran on Docker's `none` network. Plugin
validation and six offline engine cases passed. The real terminal invoked the
Rust handler; `verify.mjs` matched the exact Unicode result once in the next
actual API input, the native session ID in both requests, module UUID, checkout,
operation schema, application instructions and an unrelated CLAUDE.md sentinel.
Two main requests were examined; auxiliary requests are excluded by declared tool.
The owned container was removed after retaining selected regular evidence files.

Raw request bodies and generated vendor declarations remain local. They contain
complete synthetic contexts and are not published with this fixture. Source tests,
verifier, spec and hashes are retained; independent reproduction generates fresh
session IDs and therefore different request hashes.

## Build

From the AIKit root on a Linux x86_64 Rust 1.88 builder. In a fresh isolated
checkout first copy `aikit-sdk/qualification/20261017/validation-cargo.lock` to
the root as `Cargo.lock`; preserve any existing local lock. This baseline ignores
the root lock, and fresh resolution otherwise encounters the root CLI's yanked
ZIP dependency. The retained lock SHA-256 is
`ef111e91aec4cc31cc1810e077dfc357d1d5010539b7d239434744792c9fbf06`.

```sh
cargo +1.88.0 test -p aikit-sdk --lib --no-default-features --features integration --locked integration::
cargo +1.88.0 build -p aikit-sdk --example agent_tool_plugin --no-default-features --features integration --locked
```

Put the resulting `target/debug/examples/agent_tool_plugin` into a new host
directory named `generated`. Preserve its executable permission on Linux. This
must be a Linux binary, even when the Docker host is Windows. Our controlled
builder used Rust 1.88, Node 22, cached dependencies and no external network.
The selected integration gate passed 84 Windows / 86 Linux tests. Build the
native client image with the pinned
[Dockerfile](../../../examples/claude-mod-qualification/Dockerfile.native):

```powershell
$probe = (Resolve-Path examples/claude-mod-qualification).Path
$qualification = (Resolve-Path aikit-sdk/examples/agent-tool-qualification).Path
$generated = (Resolve-Path generated).Path
docker build -f "$probe/Dockerfile.native" -t aikit-claude-native-probe:2.1.296 $probe
```

## Run the offline engine and ordinary terminal

Use a fresh, task-owned container name. The example name below must be unused.
It mounts only public fixtures and the compiled inspector, not a provider home:

```powershell
docker run -d --name aikit-sdk-tool-fixture --label com.codelaya.task=sdk-tool-fixture --cpus 2 --memory 3g --mount "type=bind,source=$probe,target=/fixture,readonly" --mount "type=bind,source=$qualification,target=/qualification,readonly" --mount "type=bind,source=$generated,target=/generated,readonly" --entrypoint sh aikit-claude-native-probe:2.1.296 -c 'sleep infinity'
docker exec aikit-sdk-tool-fixture sh -ec 'mkdir -p /home/node/evidence; /generated/agent_tool_plugin export claude /home/node/plugin < /qualification/spec.json > /home/node/evidence/plugin-info.json; cp /qualification/spec.json /home/node/evidence/spec.json; mkdir /home/node/plugin/tests; cp /qualification/bridge.test.ts /home/node/plugin/tests/; printf OTHER_CONTEXT_MUST_STAY > /home/node/workspace/CLAUDE.md'
docker exec aikit-sdk-tool-fixture claude plugin validate /home/node/plugin
docker exec aikit-sdk-tool-fixture claude plugin test /home/node/plugin
docker exec -d aikit-sdk-tool-fixture node /fixture/gateway.mjs /home/node/evidence /home/node/evidence/plugin-info.json
docker exec -it --env ANTHROPIC_BASE_URL=http://127.0.0.1:8787 --env ANTHROPIC_API_KEY=sk-ant-aikit-local-fixture-not-a-credential --env CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 --env CLAUDE_CODE_DISABLE_OFFICIAL_MARKETPLACE_AUTOINSTALL=1 aikit-sdk-tool-fixture claude --plugin-dir /home/node/plugin --debug-file /home/node/evidence/native-startup.log
```

Normal first-run onboarding may need temporary connectivity. Select a theme and
approve only the displayed public dummy key. At the workspace trust screen,
disconnect external networking in another terminal **before accepting trust**:

```powershell
docker network disconnect bridge aikit-sdk-tool-fixture
docker network connect none aikit-sdk-tool-fixture
```

Accept trust only for this fixture workspace. Submit `AIKIT_NATIVE_BRIDGE_PROBE`.
The gateway requests the declared SDK tool, then replies `Fixture answer`.
No demo mode, managed session or permission bypass is needed. Run:

```powershell
docker exec aikit-sdk-tool-fixture node /qualification/verify.mjs /home/node/evidence
docker inspect aikit-sdk-tool-fixture --format '{{json .NetworkSettings.Networks}}'
```

Exit Claude with `/exit`. Copy named regular files `requests.jsonl`, `spec.json`,
`plugin-info.json` and `native-startup.log` from `/home/node/evidence` to a new
local evidence directory, then run `node verify.mjs EVIDENCE_DIRECTORY` there.
Copy individual files to avoid a debug `latest` symlink on Windows. Retain binary
and source revisions/hashes and network inspection alongside the result. Remove
only this owned container with `docker rm -f aikit-sdk-tool-fixture`.

The static namespace in `bridge.test.ts` matches the checked-in spec's application
and installation identities. Changing either requires updating that fixture.
The offline engine mocks process/session actions; the separate interactive run
is the real process proof. Neither authenticates a Codelaya caller, installs owned
settings, proves real model instruction following, qualifies completion enforcement
or demonstrates original-session review delivery. Windows acceptance stays open.
