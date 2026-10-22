# Native dispatch origin and trace candidate

Predeclared 2026-10-22. Use the pinned ordinary interactive Linux Claude 2.1.296
fixture with `origin-probe.js`, normal onboarding and no owner mounts/credentials.
The loopback gateway supplies scripted replies; disconnect external networking
before workspace trust. Submit `AIKIT_NATIVE_TOOL_PROBE` once and exit normally.

The mod registers a synthetic echo tool. At native Turn start it calls that tool
itself, then raises `turn.complete` with the same Turn ID and a distinct synthetic
answer before delegating the real start. The model endpoint separately calls
the same tool and answers normally. Record each dispatch's host `next.origin`,
plugin metadata and trace before/after downstream delegation.

Hypothesis: engine and plugin dispatches have distinct host-set origins, even
when the raised lifecycle event has the real Turn ID and a success-shaped body.
Do not infer physical process authentication or successful application completion
from that body. The fixture has no Codelaya installation or authorization.

The version-matched declarations state that origin is set from the host's
MessagePort/seat, while trace describes links reached beneath the latest `next`
and is empty before delegation. Check actual observations; neither an empty
trace nor a plugin manifest proves active global ordering. No capability may be
promoted from this diagnostic. Retain premise failures and raw synthetic events.

## Guarded SDK comparison, predeclared after initial observations

The native observation shows distinct plugin/user and engine/core tool origins.
The synthetic `$.turn.complete` attempt throws `not a function` on this native
surface; it does not establish a forged native outcome. The initial whole-object
`$.plugin` read also failed static scanning. Retain both premises, without
relabeling either as successful event injection.

The SDK tool callback now requires both `next.origin.plugin === 'engine'` and
`next.origin.tier === 'core'` before application I/O. Missing metadata refuses.
Run a fresh ordinary native session with the actual SDK-rendered module and a
separate `origin-spy` plugin. The spy calls the owned tool from its Turn-start
hook; the scripted model independently calls the same tool. The real SDK
`agent_tool_plugin inspect` reader handles permitted input through a recording
wrapper. No application installation or actor authority is granted.

Hypothesis: the spy receives Deny without application-handler I/O; the genuine
engine call succeeds, exactly one handler input is recorded, and native
completion remains successful. Use the existing gateway's optional bridge
metadata and port 8788 with a distinct evidence directory. Original port behavior
remains 8787. No real credential or external network is involved.

This is an in-process dispatch guard, not physical child-process authentication,
active configuration attestation, native admission or a capability promotion.
Unowned export is a diagnostic artifact; it is not an owned production install.

## Observed result

On ordinary Linux x86-64 Claude 2.1.296, the diagnostic tool recorded a direct
plugin call as `{ plugin: 'aikit-mod-feasibility', tier: 'user' }` and the scripted
model's call as `{ plugin: 'engine', tier: 'core' }`. Genuine lifecycle callbacks
also carried engine/core. Stop/completion trace was empty before delegation and
contained one descendant engine/core link afterward. This is not a full chain
or effective-configuration attestation.

The first source failed static scanning because it read `$.plugin` as an object.
After a fresh-session correction to literal `.name` and `.root`, origin recording
worked. Its attempted `$.turn.complete` call threw `not a function` on the native
surface: no fake completion event or return was observed. The real native
completion still recorded `answer`. Those errors are retained; they do not
establish native lifecycle forgery or a Codelaya false completion.

The SDK regression then failed before the fix: the module did not refuse a
foreign/missing dispatch origin before handler I/O. The narrow fix checks the
host-supplied `next`, not any `origin` in request JSON. Four renderer Rust tests
and 27 module tests pass, the latter on host Node 20 and Linux Node 22. No Rust
logic/public type, schema, capability support or activation rule changed.

The actual rebuilt SDK renderer exported the guarded module for a separate
native comparison. A distinct `origin-spy` plugin's tool call returned Deny;
the scripted engine call reached the real SDK envelope reader, exactly one
handler input was recorded, and its exact result reached the second model input.
The terminal answered and exited normally. This tool-only export does not emit
a durable native-outcome journal; no application accepted-completion claim is
made for that comparison.

An initial comparison used two paths after one `--plugin-dir`; this executable
treats the second path as the prompt. Its capture is retained as a failed setup
premise. The corrected fresh session used repeated flags and confirmed both
modules loaded before the marker. The final comparison used two main and three
total requests; all replies were scripted. Native verification passes on Linux
and retained host bytes. No real credential or owner profile was used.

## Reproduce the guarded comparison

Build the existing SDK `agent_tool_plugin` example with the committed module
source. Copy that binary to `/home/node/bin/agent_tool_plugin` and these fixtures
to `/home/node/probe` inside the isolated native container, then:

```sh
node /home/node/probe/setup-origin-guard.mjs
node /home/node/probe/gateway.mjs /home/node/guarded-evidence /home/node/guarded-evidence/setup.json 8788
```

In another terminal start ordinary Claude with the public fixture key and
`ANTHROPIC_BASE_URL=http://127.0.0.1:8788`, networking disabled after onboarding:

```sh
claude --plugin-dir /home/node/guarded-plugin --plugin-dir /home/node/origin-spy --debug-file /home/node/guarded-evidence/guarded-final-debug.log
```

Submit `AIKIT_NATIVE_BRIDGE_PROBE`. Retain `spy-result.json`, `handler-calls.jsonl`,
requests and transcript alongside the rendered `generated-module.js` and
`plugin.json`. Keep the earlier origin observation and guarded comparison in
separate capture folders as consumed by `verify-origin.mjs`.

Starting SDK revision: `070c8f435a28b54f1b8e45ac42f94f28fb968ed4` plus this fix.
Native image: `sha256:fe24c3f19fe12410b4d57bb1101def27a05a92fa7083611623157a0a2b433e65`.
Executable: `24972e3bc859fab2b46ed4c1e51f7d6130f06d3bd550811a114640de3370d0de`.
Renderer build used Rust 1.88, the established offline validation image and the
same retained ignored SDK lockfile as the earlier completion-bridge gate.
Raw captures, failed premises, red/green logs, renderer and source hashes remain
at `.local/native-origin-20261022` in the parent workspace.

| Artifact | SHA-256 |
| --- | --- |
| Origin observations | `a5460b25c28ba1418adb005b526a90aedc463c740388fa8661b8662602b295e6` |
| Guarded renderer | `66ab1250dcfeede7ee7d17bce3676afb0fe04864e2aadbaebaf4e247a38a9836` |
| Guarded module | `781abe7572f76906af33fbd319b87934da920a8f60a26f3134da0739763c2f2a` |
| Handler input | `9b8579634f182a281056077aa5edfb8583dbed9b0eeff06bc7e371eb8a45260c` |
| Guarded requests | `da2874dcafdbab185eda62df70e21fadc70cbf1bdf7a454a96d92b8a9cdde62d` |

The [official reference](https://code.claude.com/docs/en/plugins/mods/reference)
describes dispatch origin. Use the generated declarations and actual native
results for the pinned surface, not a broader EventCalls type or offline engine
as proof of native method availability. The in-process guard does not authenticate
the stdio child or prevent an out-of-band forged CLI invocation. Physical native
identity, live ordering/configuration, admission and message guarantees stay
unqualified; Windows checks remain deferred.
