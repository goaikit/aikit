# Claude mod event feasibility probe

This plugin only registers pass-through observers. It performs no file write,
network request, model call or application policy decision. Validation does not
install it into the user's profile or launch a session.

That describes the default `hooks/register.js`. A separate, explicitly selected
`hooks/native-probe.js` performs child-process and evidence writes only inside
the disposable [native/local-gateway fixture](native-gateway-probe.md). It is not
loaded by the default manifest and is not a production adapter.

Run with the executable whose exact version/platform you intend to inspect:

```powershell
claude --version
claude plugin validate ./examples/claude-mod-qualification
claude plugin test ./examples/claude-mod-qualification
```

On Linux Claude Code 2.1.296, isolated in Docker without a user profile or model
credentials, validation recognizes `turn.start`, `turn.complete`, `classic.Stop`
and `session.receive`. The pass-through settings/receive observers can produce
static warnings about missing catch handlers. They do not enforce a policy.

On Windows, an existing machine policy helper can prevent even validation from
starting. Resolve that policy login with its owner; this probe does not disable
or rewrite it. A Linux Docker result does not qualify Windows loading.

Next steps and native limitations are in
[external-session feasibility](../../aikit-sdk/external-session-feasibility-20261017.md).
No SDK capability is promoted by a static validation pass. Real interactive
completion, abort/competing-handler behavior, accepted messaging, consumption and
reconciliation require separate executed evidence.

## Offline engine results, Claude 2.1.296 Linux x86_64

Four tests run inside the executable's native plugin test engine, with every
native action stubbed, no session/profile/credentials and networking disconnected:

- Twelve independently fired Stop events retain a downstream denial. These are
  separate hook calls; the normal turn-loop continuation cap is not exercised.
- An aborted Turn preserves its ID, exact Unicode final text and abort flag.
- A receive event may be suppressed by a downstream handler before model input.
- A wrapper can await a valid downstream Stop denial and return `{}`, erasing it.
  The original hypothesis that denial survives this wrapper failed; the test now
  retains the observed counterexample. A passing counterexample test does not
  qualify protected native enforcement.

The tested Linux executable SHA-256 is
`24972e3bc859fab2b46ed4c1e51f7d6130f06d3bd550811a114640de3370d0de`.

The engine's mod Stop result uses `decision: { behavior: 'deny', message }`;
returning the command-hook JSON's scalar `decision: 'block'` is invalid and the
handler is skipped. A native bridge must translate the result correctly and
qualify error behavior. The test engine supplied this validation feedback; no
generated declarations from a signed-in interactive session were obtained.

The later [native gateway probe](native-gateway-probe.md) generated the target
version's declarations without a real account by using a local deterministic
gateway and normal workspace trust. It then verified real process stdin, native
tool registration, a correlated answer and the tool result in the subsequent
actual API input. Its model responses are scripted and it qualifies no Windows
or Codelaya delivery capability. The offline aborted fixture now uses the
generated `reason: aborted` field instead of treating the interrupt flag as the
complete outcome contract.

Claude's [environment reference](https://code.claude.com/docs/en/env-vars)
documents `CLAUDE_CODE_STOP_HOOK_BLOCK_CAP=0` as disabling the continuation cap.
The SDK [owned completion policy](../../aikit-sdk/completion-policy.md) provides
an explicit installation plan for it. Effective settings and the ordinary native
turn loop still require qualification; the override counterexample is a separate
gap. None of these results proves delivery, model-input consumption or Windows
behavior. See the official [test-kit scope](https://code.claude.com/docs/en/plugins/mods/test).
