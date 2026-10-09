# Cursor native hook dispatch probe

Run this Windows PowerShell 7 probe with an installed, authenticated Cursor CLI
and the current library-only `integration_hooks` example. It uses the existing
Cursor authentication and may consume provider usage. It creates a new disposable
Git workspace with punctuation and Unicode in its path. It does not modify the
owner's workspace or install user-level hooks.

Build from the AIKit checkout:

```powershell
cargo build --manifest-path aikit-sdk/Cargo.toml --features integration --example integration_hooks --locked --jobs 2
```

Pass the installed Node executable and Cursor entrypoint explicitly:

```powershell
./aikit-sdk/examples/cursor-qualification/run.ps1 `
  -NodePath C:/path/to/cursor-agent/version/node.exe `
  -CursorCliPath C:/path/to/cursor-agent/version/index.js `
  -SdkPath ./target/debug/examples/integration_hooks.exe `
  -OutputDirectory C:/path/to/new-disposable-evidence
```

Add `-OrdinaryWorkspace` to use a path without punctuation or Unicode. Keep the
same installed CLI, SDK executable and prompt to distinguish native shell-wrapper
failures from SDK input decoding. The default retains the literal-path stress case;
an ordinary-path pass does not qualify it.

Each native process has a bounded timeout. Timeout terminates its process tree;
the script retains stdout/stderr, exit/timeout/file results, installation plan,
SDK event replay and executable digest. The owned hooks are removed in `finally`;
configuration conflicts fail explicitly. Retain the evidence directory for review
and keep it outside committed source. Credentials are not copied into fixtures.

The script exits unsuccessfully if the baseline write fails, either process times
out/exits unsuccessfully, the gated file exists, or no SDK BeforeTool Block was
recorded. Evidence remains available on failure. A passing probe qualifies only
this bounded native dispatch/denial scenario.

The first native run requests one write without hooks. The second installs the
SDK's supported Cursor prompt/tool/session events and denies tool execution through
the reusable application callback. Examine native output and SDK request/decision
records together. An absent file or `BLOCKED` answer alone does not establish that
the SDK ran or denied the tool: native wrapper failures can produce the same result.
Likewise, exit code 0 is not accepted completion evidence. This probe does not
qualify completion enforcement, successful-completion observation, user-started
session identity, interactive mode or existing-session messaging.
