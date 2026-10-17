# Generated native message client qualification

This uses the real Claude Linux terminal, default owned project module loading,
SDK fixed-command RPC and SQLite message journal. It does not launch an agent
through AIKit, install Codelaya, contact a real model or qualify Windows readiness.
The owner-facing portable package and Codelaya dependency pin are unchanged.

## Observed results, 2026-10-18

Claude Code 2.1.296, ordinary interactive terminal, unprivileged fresh fixture
home, public dummy API key, scripted loopback Anthropic-format responses.
External networking was removed before granting workspace trust. No host
profiles/credentials, policy changes, permission bypass or `--plugin-dir` override.
The module loaded from the SDK-owned `.claude/skills` files. Native plugin
validation recognized its timer, process, prompt, environment and lifecycle APIs.

| Case | Actual result |
| --- | --- |
| Initial `bd45614` native run | Failed: its first poll preceded application attachment, returned NotFound and stopped the pump. The later staged review remained Accepted, with no native handoff. This negative evidence is retained; that review was not resent. |
| Startup-fixed fresh native run | Passed: initial native Turn, owner-side frozen staging, one SDK reservation and native prompt call, exact complete payload once in the next actual API input, raw native return persisted. Two main API requests. |
| Normal exit and SDK reopen | Passed: the original Unknown receipt, exact request/target/attempt and raw return were unchanged. No qualified acknowledgement or consumption was invented. |
| Resume same native session | Passed: new invocation generation, old receipt/target unchanged. A separate owner prompt produced one additional request; the old payload occurred once in conversation history, without a second appended delivery. |
| Post-receipt SIGKILL | Passed within this container-supervised scope: after receipt storage and the resumed owner Turn finished, the fixture container was killed (observed exit 137), restarted and its SDK records reopened. The same Unknown/return survived; no graceful SessionEnd was invented. No Claude process survived the container restart. This is not a mid-handoff crash or standalone native Windows cleanup test. |

The startup fix makes an initial `poll` with no application binding defer without
creating a binding or reservation. An explicitly detached binding still refuses.
The test-envelope helper was also corrected: it had implicitly attached the
application and concealed the actual startup condition. The corrected regression
first asserts that the binding table remains empty, then attaches explicitly and
checks reservation and detach refusal.

Windows Rust 1.88 passed 104 SDK integration tests and scoped library/bridge/hook
example Clippy (`-D warnings`, existing SDK `uninlined_format_args` allowance).
Offline Linux passed 106 integration tests serially and built all four embedding
binaries. Six offline retained-evidence mutations were rejected by the verifiers:
missing outgoing payload, foreign native session, changed raw return, invented
acknowledgement, resume without a new invocation and changed crash attempt.
These six mutations are not six additional native fault-injection scenarios.

## Evidence identities and boundaries

Native image `aikit-claude-native-probe:2.1.296`:
`sha256:fe24c3f19fe12410b4d57bb1101def27a05a92fa7083611623157a0a2b433e65`.
Its [Dockerfile](../../../examples/claude-mod-qualification/Dockerfile.native)
checks native executable SHA-256
`24972e3bc859fab2b46ed4c1e51f7d6130f06d3bd550811a114640de3370d0de`.
The SDK binaries were built from `bd456146827ea3a2ccc485d36709499be48d265a`
plus this commit's startup fix. Retained `source-revision.json` records the exact
patched source digests; `binaries-sha256.txt` identifies each executed binary.
`setup.json` records the exact owned file fingerprints.

| Retained evidence | SHA-256 |
| --- | --- |
| Initial plus feedback actual requests | `0d833a135ef70c7c87d50cb6aa3ae6fb1199ad1bf077da56f08c1d32d147ac20` |
| Persisted original receipt and raw return | `cefe41bccaeeb0e5b684b19a02a869c32d3f2c572152d0562203994bb5ce4adb` |
| Native journal after normal exit | `8b93816b25989150223a2231938e9e17b8deeecc5843703f51215e6b67528307` |
| Requests including resumed owner prompt | `c3e975344ed22e0cb4c3f281d4753fe986576e2bba4b63786e58ce46f4923145` |
| Native journal after resume and crash/reopen | `3de4cc0b6e16e444999d19ff254bd0e48f328b87bba229e9c06bda7adc956bae` |

Raw synthetic contexts are retained locally under the Codelaya implementation
chat's `.local/message-client-generated/{startup-negative,success,resumed-post-receipt-crash}`.
Both owned native containers were removed after retaining regular evidence files.
Docker's diagnostic `latest` symlink initially prevented a directory-wide Windows
copy; selected regular files were then copied and independently verified on the
Windows host. No missing symlink was treated as missing runtime evidence.

The raw result contains exact text and plugin origin, but this candidate still
stores **Unknown**, without qualified acknowledgement/consumption. The fixture
independently observes actual outgoing input at its endpoint; production has no
equivalent protected proof in this implementation. Caller authentication,
native admission across competing handlers, delivery reconciliation, prompt
rewrite/drop and mid-handoff crash races, real-model semantics, Codelaya dispatch
orchestration, native Windows and other providers remain required. No capability
assessment or consumer dependency was promoted by these results.

## Reproduction

Build these existing thin SDK examples with Rust 1.88 and
`--no-default-features --features integration --locked`:

```text
cargo +1.88.0 build -p aikit-sdk --example integration_hooks --example integration_message_bridge --example integration_messages --example agent_tool_plugin --no-default-features --features integration --locked
```

Use Linux binaries in the pinned native image. Bind them read-only at
`/generated`, and bind this fixture directory read-only at `/qualification`.
Use a new unprivileged container/home; do not mount any host profile or credential.
Keep a foreground owner-started terminal available separately from the fixture
commands. The SDK state at `/home/node/message-state` must be disposable.

1. Run `node /qualification/setup.mjs`. It delegates hook and opt-in module
   installation to SDK examples, preserving an unrelated fixture setting.
2. Validate the plugin root recorded in `/home/node/evidence/setup.json` using
   `claude plugin validate ROOT`. Start
   `node /qualification/gateway.mjs /home/node/evidence` in a separate process.
3. Start ordinary `claude --debug-file /home/node/evidence/native-startup.log`
   with `ANTHROPIC_BASE_URL=http://127.0.0.1:8787`,
   `ANTHROPIC_API_KEY=sk-ant-aikit-local-fixture-not-a-credential`,
   `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1` and
   `CLAUDE_CODE_DISABLE_OFFICIAL_MARKETPLACE_AUTOINSTALL=1`.
4. Complete normal onboarding. **Before accepting workspace trust**, disconnect
   the container's `bridge` network and connect only `none`; retain `docker
   inspect ... --format '{{json .NetworkSettings.Networks}}'`. Trust only this
   disposable workspace. Do not use managed launch, demo or permission bypass.
5. Submit `AIKIT_DURABLE_CLIENT_INITIAL` in the native terminal. After its actual
   completion, run `node /qualification/stage.mjs` separately. It stages one
   immutable operation, not a second manually typed prompt. Do not rerun it.
6. After the generated client completes its feedback Turn, run
   `node /qualification/capture.mjs /home/node/evidence live`. Require Unknown,
   retained raw return, two native starts and two finishes. Use `/exit` normally;
   capture `after-exit`, then run `verify.mjs /home/node/evidence`, saving its
   stdout as `verification.json`. Retain this baseline before changing the session.
7. Manually run `claude --resume SID --debug-file /home/node/evidence/native-resume.log`
   under the same environment/network, using the original SID in `staged.json`.
   Capture `resumed-idle`. Submit `AIKIT_OWNER_AFTER_RESUME` yourself. When it
   finishes, capture `resumed-live`. Do not stage another review.
8. For the scoped post-receipt crash, inspect container ownership first. Kill
   **only that fixture container**, retain its exit state before restarting it,
   restart its idle keeper without relaunching Claude, and capture `after-crash`.
   Save `docker top` and run `verify-resume.mjs /home/node/evidence`.
9. Copy selected regular evidence files; avoid provider diagnostic symlinks.
   Run both verifiers independently against the retained baseline/resume evidence,
   then `check-verifiers.mjs HOST_EVIDENCE_ROOT`. Retain failures separately.
   Remove only the owned fixture containers when finished.

The gateway accepts only the public dummy key, caps actual requests at 24 and
body size at 4 MiB, and never proxies externally. Its scripted answer proves
mechanical transport/correlation, not real model interpretation or daily usability.
