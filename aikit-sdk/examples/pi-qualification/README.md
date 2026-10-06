# Native Pi qualification

This opt-in harness exercises an installed Pi runtime and the SDK's actual hook
executable. Model responses come from Pi's own `fauxProvider`; no model account or
network response is required. It does not mock Pi's extension loader, tool engine,
continuation loop or SDK transport. It is not a semantic model evaluation or full
provider readiness test.

## Prerequisites and execution

- PowerShell 7, Node compatible with the selected Pi release, and Rust.
- A separately installed `@earendil-works/pi-coding-agent` package. The recorded
  original run used version **1.0.4**, Node **24.19.0**, Windows x86_64 and SDK **0f9c971**.
  The expanded boundary-counterexample run used SDK **fff6aa7** on the same runtime.
- Build the SDK example from the repository root:

```text
cargo build -p aikit-sdk --no-default-features --features integration --example integration_hooks --locked
```

Install Pi in a disposable directory if needed; do not change the owner profile:

```text
npm install --prefix <fixture> --ignore-scripts --no-audit --no-fund --save-exact @earendil-works/pi-coding-agent@1.0.4
```

Run from the repository root, replacing the paths with actual installation paths:

```powershell
& ./aikit-sdk/examples/pi-qualification/run.ps1 `
  -NodePath '<node executable>' `
  -PiCliPath '<fixture>/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js' `
  -PiAiModulePath '<fixture>/node_modules/@earendil-works/pi-ai/dist/index.js' `
  -SdkPath './target/debug/examples/integration_hooks.exe' `
  -OutputDirectory '<new evidence directory outside the repository>'
```

Use the extensionless example binary on Unix. Only the Windows run is qualified
here; portability of the harness does not establish other platforms' behavior.

The output directory must not exist. It holds a private Pi profile, a workspace
whose name includes punctuation and Unicode, SDK state, and all evidence. The
harness explicitly approves its own disposable project resources, disables startup
networking/telemetry, installs no user configuration, and bounds each process to
45 seconds. It terminates its own process tree on timeout. Each Pi invocation is
a fresh process, so changed extensions are loaded again. It removes the SDK-owned
extension through the SDK even if a scenario fails; evidence remains on disk.

## Assertions

1. Without SDK hooks, a native Write creates the expected file and reports success.
2. With hooks, three completion Blocks cause three continuations; the fourth
   proposal is Allowed. Each journaled proposal contains the expected Final Answer.
3. An allowed Write creates the expected file and records AfterTool.
4. Writing to a directory fails and records ToolFailed.
5. A denied Write creates no file; both the SDK Block and native error result exist.
6. A provider error records CompletionFailed, with no CompletionProposed or Final
   Answer. Native process exit alone is insufficient: this case exits zero in Pi
   1.0.4 print mode.
7. A competing extension aborts after confirming an SDK Allow through the actual
   SDK example's journal query. Pi settles with the same `type`-only event as the
   normal completion case. No abort signal is available at these boundaries and
   no CompletionFailed is recorded. This proves the missing observation contract.
8. A later extension sees the SDK Block and `continue:true`, then returns
   `continue:false`. Pi settles after one model call without an Allow. This proves
   that continuation requests can be overridden in the qualified context.

The two ordering scenarios disable automatic discovery and explicitly load the
installed SDK source once, ahead of `boundary-probe.mjs`. The other six scenarios
still exercise automatic project discovery. Naming the SDK source explicitly
while leaving discovery enabled loaded it twice via Windows path aliases in the
initial setup attempt; invocation guards blocked admission, and that attempt was
not counted as boundary evidence. The probe requires a recorded SDK decision
before taking action, so the wrong handler order fails the scenario.

Session start/end records are matched against the runtime's session ID for each
installed scenario. Successful removal is required before `summary.json` is
written. The summary includes runtime versions and executable/provider hashes.
Native streams, event logs, SDK journals and stderr are retained for diagnosis.
Do not commit generated evidence directories or npm installations.

Pi 1.0.4 does not dispatch `tool_result` to extensions for a denied tool, although
its model-facing tool result is an error. Consumers should use the recorded
BeforeTool Block for that case; an executed tool failure is a different event.

## Limits

The harness uses a trusted project bridge and a deterministic provider, plus one
controlled competing extension for the two counterexamples. It does not qualify
arbitrary extension combinations, interactive mode, compaction/resume,
subagents, native process identity, effective managed settings, hook outages,
hard deadlines, semantic Final Answer quality, or messaging. Settlement does not
provide an authoritative accepted-proposal identifier. These native requirements
remain Unknown or Unsupported; see `../../integration-qualification.md`.
The requested native contracts are in `../../integration-enhancements.md`.
