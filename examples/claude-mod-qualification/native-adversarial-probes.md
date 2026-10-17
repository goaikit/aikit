# Native abort and competing Stop probes, 2026-10-18

These are opt-in diagnostic mods, never loaded by the default manifest. They
ran in fresh ordinary Claude Linux x86_64 2.1.296 terminal sessions with the
installed Codelaya consumer (`b83c633` production source, SDK `3b22c60`). Replies
were scripted at container loopback, no real model/account. Networking was
removed before workspace trust/plugin loading. No owner profile or policy helper
was changed. All disposable runtime containers were removed after retention.

## Abort after Allow

`hooks/abort-after-allow.js` awaits the downstream Stop result, records it, then
calls `$.turn.abort({ turnId })`. Native validation recognized the API. The actual
session named the Turn, wrote UTF-8 bytes, obtained fresh file context and wrote
an exact-revision Work Note. Its SDK Allow preceded the actual native aborted
outcome, with matching session/generation/Turn. Codelaya preserved Incomplete,
a recovery snapshot and no ending snapshot, including after normal session exit.

The independent verifier checked outgoing API inputs, durable application/SDK
records and the separate probe's native events, in-container and on retained
host evidence. Six main API requests; actual abort API returned successfully.

At execution, the probe source SHA-256 was
`2c3471e31906e2209039edce94b59927f6753fd9b5d942ca289b6e75eb51f221`.
The published guard subsequently also refuses a native `{ block: string }` result;
the successful Allow path is unchanged. That guard tightening is not a separate
native rerun. Retained input/event verification also requires an SDK Allow and
no downstream block, independently of that guard.

## Competing wrapper: original prediction not observed

The offline engine had allowed a wrapper to erase a synthetic
`decision.behavior: deny` result. The first native attempt assumed that same
shape; it threw when the real command-hook result was `{ block: string }`.
Codelaya kept blocking 22 times before the bounded fixture ran out of replies;
the session was interrupted. This failed fixture assumption is retained.

The corrected `hooks/erase-stop-denial.js` explicitly awaited each actual native
`block`, recorded it, and returned `{}`. The prediction that this would end the
native Turn was **not observed**: 22 SDK Blocks and 22 empty mod returns were
followed by continued native model requests. There were 23 main API requests,
zero SDK Allows and no accepted native answer. The fixture's request ceiling
ended the run with a native error; Codelaya remained Incomplete with no ending
snapshot. Its installed completion-cap setting was `"0"`, and HEAD/index stayed
unchanged. Normal exit did not upgrade the work.

The verifier for the original denial-erasure prediction failed and its source
and stderr are retained. A separate exploratory verifier checks the observed
repeated-block result. This is not a predeclared full enforcement qualification,
nor evidence that all competing handlers are safe. In particular, short-circuiting
a handler before `next`, failures/timeouts, effective-policy changes and Windows
still require executed qualification. The offline test result cannot be treated
as native command-hook enforcement evidence in either direction.

Corrected wrapper source SHA-256:
`c31483e3317ec523e24e23d93696943efdbee16ea87cad0bd38c59628391a2ca`.

## Evidence identities

Same pinned native image/executable as [the normal native probe](native-gateway-probe.md).

| Evidence | SHA-256 |
| --- | --- |
| Abort outgoing requests | `cf09526734300269c86c02ce2f74792e373524da74f8e0aecef8055ec91530eb` |
| Abort native probe events | `1430d8ce664da9d4d3147896959d1bc28fa7aec69a1da95bed913f84747658b4` |
| Competing wrapper outgoing requests | `42ee171f1ac4ff84385f6e844fb1d86a583b6bebc4864421f6af8567a03a215f` |
| Competing wrapper probe events | `fb4f8bea0e8b9c68c1d9b120e3cf24dfa6bf98a77886d1ed31b08c8b25bc49df` |

Raw contexts remain local in the implementation chat's
`.local/native-completion-generated/{abort-after-allow,override-shape-negative,competing-stop-preserved}`.
No capability is promoted; native Windows, protected caller/deadline guarantees,
other handler strategies, review delivery/consumption and real-model behavior
remain unqualified.

## Reproduce

Use the consumer's registered-native fixture setup with the pinned binaries and
fresh disposable Git checkout/state. Mount this SDK example read-only, copy it
to `/home/node/adversarial-probe`, and explicitly change that copy's
`hooks/hooks.json` to `{"modules":["./abort-after-allow.js"]}` or
`{"modules":["./erase-stop-denial.js"]}`. No ordinary installation is modified.

For abort, use the consumer `notes` scenario with
`expect_native_outcome: "aborted"`, `expect_accepted_completion: false` and its
`verify-abort.mjs`. For the competing wrapper, use `scenario: "unnamed"`, leave
accepted completion false, retain `.claude/settings.local.json` as
`owned-settings.json`, and use `verify-competing-stop.mjs` after the bounded run.
Both use the same scripted local gateway and `CODELAYA_REGISTERED_PROBE` marker.

Start ordinary interactive Claude with the additional explicit
`--plugin-dir /home/node/adversarial-probe`. Complete normal onboarding/trust;
disconnect external networking before accepting workspace trust. Retain native
events, requests, source/binary identities, application inspection, HEAD/index
and exit inspection. Verifiers must fail if actual outcomes differ; do not
rename an unexpected result into qualification. Exit normally and remove only
the owned disposable container.
