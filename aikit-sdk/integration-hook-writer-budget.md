# Bounded native hook journal writer acquisition

Base `ec2e5bc`, Linux Rust 1.88.0, integration feature without defaults.

## Reproduced failure and change

A controlled Stop callback returns Allow once while a separate SQLite writer
holds the integration journal for 300 ms. The old fixed 100 ms commit acquisition
returns empty stdout/status 2 with `[state_busy]`, discarding a still-valid
decision despite a ten-second request budget. The red test is retained in the
consumer workspace `.local/native-startup-contention-20261023/decision-writer-red.log`.
This reproduces a concrete failure class, not proof of the cause of any earlier
uncategorized native error.

Hook observation/decision writer acquisition now waits at most one second, capped
by the original remaining request-work deadline. Only SQLite lock acquisition
waits: the callback, transaction body and native action are never replayed.
Allow cannot use the existing 250 ms commit/response reserve. A timeout's Block
can still use that reserve, preserving the prior native blocking response.

After acquisition, expiry and invocation/installation revision are checked before
append. Decision commits also recheck installed configuration after the wait.
Revision checks apply even to legacy requests without invocation claims; that
does not turn those claims into qualified identity. Changed configuration,
replaced revision, exhausted budget or a sustained writer refuse without a
prepared Allow. Existing recorded-admission guards still refuse immediately.
The fixed `request_expired` diagnostic includes no raw caller data.

No public method, schema, hook/module wire format, provider setting, native
timeout or capability qualification is changed. This is not a native hard-deadline
guarantee; process scheduling, I/O and actual provider enforcement remain separate.

## Evidence

- The original short-writer test fails with status 2; the fixed test succeeds
  with one callback and the original observation plus one matching decision.
- A one-second request with a two-second writer returns a blocking failure and
  retains no prepared Allow. The callback runs once.
- Disabling hooks during acquisition refuses without a prepared Allow.
- Replacing the installation revision during acquisition also refuses, including
  a request without invocation claims.
- Existing timeout/error/panic Block behavior, repeated Stop validation,
  configuration drift and immediate admission refusal regressions pass.
- Full SDK integration gate: **166 passed**. Formatting passes. Library Clippy
  completes with the same **101 existing warnings**, not a strict clean result.

```sh
cargo +1.88.0 test --locked --offline -p aikit-sdk --no-default-features --features integration --lib integration:: -- --test-threads=2
cargo +1.88.0 clippy --locked --offline -p aikit-sdk --no-default-features --features integration --lib
```

The first fix correctly refused expiry but prevented the existing timeout Block
from using reserved response time; its failing gate is retained. The final fix
preserves that Block behavior while excluding Allow from the reserve.

Consumer/native qualification and the original native startup lock remain
separate. Do not infer live readiness or Windows support from these Rust tests.
