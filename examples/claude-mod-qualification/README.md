# Claude mod event feasibility probe

This plugin only registers pass-through observers. It performs no file write,
network request, model call or application policy decision. Validation does not
install it into the user's profile or launch a session.

Run with the executable whose exact version/platform you intend to inspect:

```powershell
claude --version
claude plugin validate ./examples/claude-mod-qualification
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
