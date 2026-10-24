# Opt-in native completion bridge candidate

Current 2.1.308 [mod budget and worker recovery probes](../examples/claude-mod-qualification/current-mod-failure-20261024.md)
reuse this source's exact constant fallback. Native budget expiry and actual
wedged-worker replacement both invoke it, propagate Block into the next request
and complete only after a second Stop delegates. This closes those two native
primitive questions; do not request a generic provider worker-loss fallback.
It does not yet qualify the full generated callback's Turn/invocation context
after replacement, protected ordering, arbitrary external kill or unload. The
two plugin environments share one native worker; retained evidence explicitly
accounts for the observer's reset. Capability assessments remain unchanged.

Later [native generated-module qualification](../examples/claude-mod-qualification/generated-stop-protocol.md)
executes the owned module with the actual SDK example handler, retained command
hook, invalid JSON and process timeout. It closes those scoped execution gaps;
Codelaya's handler and remaining native protection/delivery gates stay open.

This extends the existing `AgentToolSpec` with
`native_completion_bridge: bool`. JSON defaults to false and omits false on
serialization. Rust struct literals must supply the field. No new crate,
handler protocol, storage schema, CLI lifecycle or readiness exception is added.
The candidate is Claude-only, like the existing native tool renderer.

## Contract

Pass `native_completion_bridge: true` to the existing `plan_agent_tool`,
`plan_agent_tool_with_message_bridge` or combined update planner. The owned
installation must declare input and completion hooks. Its fixed session handler
also handles the native Stop input; callers cannot select a different executable
through the event. Standalone unowned export refuses the opt-in because it has
no installation session handler.

Enabling this candidate requires detached recorded bindings at planning and
apply/recovery through the existing durable plan guard. Owned file fingerprints,
conflict handling and removal use the current installer. Planning does not
modify files. It does not configure user/managed `prependPlugins`, install a
marketplace, authorize policy changes, attest active ordering or activate an
application. The default module has no mod Stop callback.

The generated `classic.Stop` callback waits for the existing native Turn-start
observation and publishes the existing inherited invocation context. It checks
session/workspace correlation, main-session scope, input bounds and cancellation
or changed Turn before delegating. These checks are correlation, not native
caller authentication. It invokes the owned session handler once with the
original event JSON on stdin and the configured process timeout.

Only the existing handler response shapes are accepted:

| Response | Native behavior |
| --- | --- |
| Exit 0, untruncated stdout/stderr, stdout `{}` | Delegate to downstream native hooks unchanged |
| Same process conditions, stdout `{"decision":"block","reason":"..."}` | Return the mod API's `{ block: reason }` |
| Process failure, truncation, oversized/unsupported response, changed/cancelled Turn | Return Block |
| Exception, invalid JSON or native callback budget failure | Constant `.catch` Block with no logging, process or filesystem dependency |

The input ceiling is 128 KiB, stdout 64 KiB and Block reason 32 KiB in UTF-8.
Empty reasons and additional response fields are rejected. Stdout diagnostics
cannot be mistaken for an Allow. Missing startup context blocks without invoking
the policy handler. No uncertain invocation is automatically retried.

Application policy remains in the consumer's existing handler. The module does
not create Work Notes, decide review requirements or treat Allow as completion.
The existing `turn.complete` observation still reports the final native outcome.

## Qualification limits

The controlled [catch](../examples/claude-mod-qualification/stop-catch-protocol.md)
and [ordering](../examples/claude-mod-qualification/prepend-stop-protocol.md)
fixtures establish candidate primitives, not execution of this generated module.
It needs native qualification with the actual SDK and Codelaya handler, including
failure/deadline windows, protected identity, effective ordering/configuration,
repeated decisions, cancellation and worker loss. Existing command Stop hooks
are retained: an Allow delegates through them and can cause a second independent
application validation. It must not skip unrelated hooks. Qualify that combined
path rather than assuming duplicate validations are equivalent to one.

An absent or ignored first-handler configuration can still let an earlier mod
skip the callback. An exception in the fallback, worker loss or unloading can
invalidate protection. No capability becomes Supported from rendering or unit
tests; Unknown still fails required readiness. Windows checks remain deferred.
The production Codelaya dependency and package do not acquire this option until
explicitly updated and qualified. Existing profiles and owner stores are untouched.

## Scoped implementation qualification, 2026-10-22

Offline Linux Rust 1.88 passed 48 installer tests and four renderer/serialization
tests. New cases cover legacy JSON default/omission, unowned-export refusal,
owned planning/removal preserving unrelated hooks, missing input observations,
attached bindings and a binding introduced between planning and apply. The
existing install/update/recovery tests remain included in the 48-test scope.
Production SDK library Clippy passed with warnings denied and the established
`clippy::uninlined_format_args` allowance. The seven changed Rust files passed
their scoped formatting check; the repository's pre-existing formatter-option
warnings and two existing unit-test unused-guard warnings remain visible.
No full-workspace or native Windows green result is claimed.

The changed module plus existing outcome/message module suites passed 26 tests
on Linux Node 22.23.3 and host Node 20.19.0. They check exact Block translation,
unchanged downstream delegation, input/context bounds, invalid/truncated/failed
responses, constant fallback and cancellation/new-Turn races. These use an
engine stub; no native Claude session or Codelaya handler ran in this gate.

```sh
cargo +1.88.0 test -p aikit-sdk --lib --no-default-features --features integration --locked --offline integration::install::tests
cargo +1.88.0 test -p aikit-sdk --lib --no-default-features --features integration --locked --offline integration::agent_tools::tests
cargo +1.88.0 clippy -p aikit-sdk --lib --no-default-features --features integration --locked --offline -- -D warnings -A clippy::uninlined_format_args
node --test aikit-sdk/src/integration/agent_tools_completion_claude.test.mjs aikit-sdk/src/integration/agent_tools_claude.test.mjs aikit-sdk/src/integration/agent_tools_messages_claude.test.mjs
```

The isolated source started at `686fd8e` plus this change, using image
`sha256:c917b61f52e31ea6a0534cbfcf91f4e2426de4b745ae41ca994cc3a86a46f3bc`.
The first locked attempt stopped because Git archive omitted the ignored local
lockfile; supplying that existing lockfile fixed preparation without dependency
resolution changes. Its SHA-256 is
`ef111e91aec4cc31cc1810e077dfc357d1d5010539b7d239434744792c9fbf06`.
All integration source files match the checkout after Git CRLF/LF normalization;
raw and normalized source manifests, logs and the lockfile identity are retained
under `.local/native-completion-bridge-20261022` in the parent Codelaya chat
workspace. This records the dependency set actually tested, not a claim that an
upstream checkout already contains that ignored lockfile.
