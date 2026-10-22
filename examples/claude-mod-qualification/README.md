# Claude mod event feasibility probe

The later [guarded native Stop failure comparison](stop-failure-protocol.md)
observes missing and timed-out managed command hooks allow successful completion
despite the actual built-in guard and `onFailure: "block"`. This adds an exact
native failure diagnostic, not readiness. The preceding
[guard comparison](managed-stop-protocol.md) resisted one short-circuit mod;
that protection alone does not establish failure handling.

The later [combined SDK crash recovery](native-sdk-append-crash.md) exercises a
real post-storage/lost-reply SIGKILL, cold SQLite inspection, original-session
resume under a new invocation and immutable no-resend recovery. It qualifies one
Linux failure window and retains Unknown; production native proof stays open.

The [combined native SDK append run](native-sdk-append-client.md) now exercises
the real installed module, fixed-command receiver and SQLite checkpoint methods
in one ordinary Linux session. It retains an initial strict-framing failure,
six native label cases and a corrected successful capture. Delivery/consumption
remain unqualified, and Windows remains deferred.

The later [native append interruption](native-append-interruption.md) captures
SIGKILL after storage but before submit return, then original-session resume
without resend. The separate reusable SDK checkpoint implementation retains
candidate row correlation; the combined native module/journal path is pending.

The later [native append competitors](native-append-adversarial.md) exercise a
real downstream content rewrite/false return and a no-next fabricated receipt.
Both preserve actual stored-row evidence in the tested Linux load order; submit
return and Turn-start text alone fail to identify rewritten outgoing content.

See the later [native adversarial probes](native-adversarial-probes.md) for
abort-after-Allow and a real competing-wrapper run. The native run did not
reproduce the synthetic engine's denial-erasure prediction. These additional
fixtures are explicitly selected and are not loaded by the default manifest.

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

The later [append acknowledgement candidate](append-acknowledgement-candidate.md)
adds ten offline storage-contract tests. Its later isolated Linux interactive
capture established a matching native UUID, provider-framed stored row and
outgoing envelope block. The original unframed-message hypothesis failed and is
retained. This is bounded scripted-endpoint evidence; production acknowledgement,
protected consumption and Windows remain unqualified.

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

The later actual command-hook probe observed `{ block: string }` downstream from
native Stop handlers. Returning `{}` did not erase 22 native Blocks. This differs
from the offline synthetic result above; see the adversarial report for the
failed prediction, exact executed sources and bounded scope.

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
