# Existing Claude policy controls and action-hook protection

2026-10-25. Ordinary interactive Linux x86_64 Claude Code **2.1.308**;
Windows checks deferred. This follows the healthy Codelaya clear/resume workflow
at SDK `0907525` and investigates mandatory-policy failure separately.

Runtime change: `609668d7d33264ae4b687b14e18d89934468af6d`.
Linux Rust 1.88 locked/offline regression gates pass: 198 integration cases with
SDK defaults disabled, and 201 with `integration,claude-sdk` (the consumer's
feature combination), both serial. The latter includes the two new ownership/
legacy-update tests and scoped capability assertions. These are library gates,
not Windows, workspace-wide CI or another full native application run.

## Implemented in the existing installer

AIKit's owned Claude hook renderer now adds `onFailure: "block"` to
`UserPromptSubmit` and `PreToolUse` command handlers. It keeps executable/argument
serialization and the existing preview/apply/receipt/update/removal machinery.
There is no new public policy framework, CLI logic, schema or readiness grant.
Prior receipts remain readable; explicit update replaces only owned entries.
Removing or changing the owned field is configuration drift and refuses removal.
Tests cover those properties and preservation of unrelated hooks/settings.

The [official command-hook contract](https://code.claude.com/docs/en/hooks#block-the-action-when-a-hook-fails)
requires 2.1.295 or later for this field. It excludes Stop from failure blocking.
AIKit therefore does **not** add a misleading Stop failure flag. Older versions
and Windows are not qualified by the new Linux cases. Existing installations
need the normal explicit update; reading a prior receipt does not silently edit
the provider's settings.

## Native experiment and identities

[Reproduction helpers and protocols](../examples/claude-policy-qualification/README.md)
run in fresh disposable homes, no mounts and network none. Root-owned managed
settings exist only inside the test containers; node UID 1000 cannot write them.
The native policy API returns that exact policy. The actual built-in
`cc-plugin-sec-default` loads outermost at prepend tier. The competing plugin is
actually user tier and never runs its short-circuiting classic hook. Native core
origins, settled completion traces and independently captured bounded loopback
requests corroborate outcomes. Terminal exit codes are zero.

Native binary SHA-256:
`356733f185fa988e6bf3aa9a555269fdd167982a2e2a80d3c8a78037bb92ad58`.
Image ID:
`sha256:9312badacd0cdf5496308a7292a5dc9965d25c616dbda94764ee10bbe8c30924`.
Node 22.23.3. The public dummy key authenticates no real service. Replies are
scripted, not model-quality evidence. The eight-request/4 MiB limits were not
increased. Each positive Stop block held its second main request, then the owner
cancelled within 120 seconds; native completion was aborted, not accepted.

### Ten fresh matched managed-policy cases

| Case | Main / all requests | Actual outcome |
| --- | --- | --- |
| Stop constant Block | 2 / 3 | Continuation held; owner abort |
| Stop missing validator | 1 / 2 | Answer accepted despite hook error |
| Stop shell fallback after missing validator | 2 / 3 | Valid Block preserved; owner abort |
| Stop one-second timeout | 1 / 2 | Answer accepted after timeout |
| Stop missing validator, onFailure block | 1 / 2 | Answer accepted; native explicitly ignores field |
| Stop timeout, onFailure block | 1 / 2 | Answer accepted; native explicitly ignores field |
| Prompt missing validator, default | 1 / 2 | Prompt reaches model and answer completes |
| Prompt missing validator, onFailure block | 0 / 0 | Prompt blocked before native turn/model request |
| Read missing validator, default | 2 / 3 | Native tool returns actual marker file contents |
| Read missing validator, onFailure block | 2 / 3 | Native tool error, no marker contents |

The remaining requests are separately classified title requests. A UI Read label
alone is insufficient: the matched tool result contains either the actual marker
or the explicit hook failure error. All ten read-only verifier cases pass; these
are ten native runs, not ten simulated unit cases.

### Three native module-admission cases

The existing [plugin registration API](https://code.claude.com/docs/en/plugins/mods/reference)
provides scanned uses and native origin/tier. A prepended user-profile guard
refused a competing Stop module while admitting a benign module. When the primary
guard threw, its constant refusal fallback still worked. When both threw, native
skipped the gate and admitted the competitor. All three admission verifiers pass;
these cases use no model requests. The native debug `loaded` line precedes
admission, so verification uses actual refusal and startup-chain/marker evidence.
These findings align with [hook failure behavior](https://code.claude.com/docs/en/plugins/mods/events#handle-a-hook-that-fails)
and [managed mod controls](https://code.claude.com/docs/en/plugins/mods/admin).

## Precise remaining gap

Existing managed settings and the built-in guard can protect a command hook from
this user-mod bypass. A valid command fallback can handle this missing-validator
case. Neither result qualifies arbitrary mandatory module presence, replacement,
or failure. On the tested binary, the **outer native Stop timeout/start failure**
can still accept an answer even with managed policy and `onFailure: block`.
That is a specific tested native limitation of this path; it is not proof that
all existing Claude APIs have been exhausted. A constant mod `.catch` helps but
does not prove enforcement if the fallback itself fails or the module is absent.

Production completion readiness remains Unknown. Independent durable review
acknowledgement and exact model-input consumption are separate open contracts;
this policy work does not change append/checkpoint candidates or promote an
Unknown receipt. See [the request-bound snapshot counterexample](claude-request-snapshot-boundary.md)
and [native contract scope](native-integration-required-contracts.md).

## Evidence retention and failed attempts

[Archive manifest](../examples/claude-policy-qualification/archive-manifest.json)
records SHA-256/size for the 13 accepted captures and eight failed captures.
Raw archives and stopped containers are retained locally, not uploaded. The
first managed cohort's two positive cases missed the gateway hold predicate
because native feedback used a literal escaped newline; they reached the
request ceiling and are not accepted positive controls. The second cohort's
gateway had a duplicate declaration, never listened, and produced connection
failures; none counts as provider behavior. Fresh third-cohort cases passed
syntax and independent health checks before native startup. No live settings or
responses were patched. An initial read-only admission verifier incorrectly
equated worker construction with admission; its original source is retained and
the corrected verifier checks real chain membership. Failed evidence was not
deleted or relabelled as success.
