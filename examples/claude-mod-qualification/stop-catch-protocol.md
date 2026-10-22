# Native Stop mod failure-handler candidate

Predeclared 2026-10-22. This diagnostic tests one documented mod `.catch`
primitive in ordinary interactive Linux Claude 2.1.296, not production readiness.
Use the pinned native image and normal onboarding from
[the gateway protocol](native-gateway-probe.md#reproduce). Copy the fixture,
select `stop-catch.js` in `hooks/hooks.json`, and run the existing bounded
loopback gateway. No managed policy, competing mod, owner mounts or credentials.
Disconnect external networking before workspace trust. Replies are scripted.

In each fresh session write one case name to `/home/node/evidence/case.txt`,
submit its unique `AIKIT_STOP_CATCH_<CASE>` marker, await native completion,
then exit normally. Save the native events, transcript and debug log before
starting another session. The fixture fails only the first Stop; subsequent
Stops delegate normally. It records the native catch error and whether `next`
was called, then returns `{ block: 'AIKIT_STOP_CATCH_BLOCK' }` unless the case
intentionally fails the fallback too.

| Case | Injection | Hypothesis |
| --- | --- | --- |
| throw | First Stop throws | Catch Block reaches native continuation and second outgoing request |
| timeout | First Stop never settles | Native budget expires; catch Block reaches continuation and second request |
| catch-failure | First Stop throws, catch also throws | Native behavior must be observed; no fail-closed claim assumed |

If catch Block works, this establishes only the tested callback failure path.
It does not establish protected precedence, callback caller identity, effective
configuration, worker loss, reload, cancellation, native idle admission,
acknowledgement/consumption or Windows support. Do not promote capabilities.
Retain failures without repairing/relabeling a session. The gateway's 32-request
and 4 MiB body ceilings remain in effect.

## Observed result

All three ordinary sessions exited normally. Eight total requests comprised
five main requests and three auxiliary requests. The throw and timeout cases
each recorded first Stop, native catch with `next.called: false`, returned
Block, second main request containing the exact reason, second Stop and one
successful `turn.complete`. The timeout catch ran 10012 ms after the first
Stop record and reported `error.kind: timeout`, with the fresh 1000 ms grace.
The throw catch reported the injected error. Both native debug logs say the
catch answered; both transcripts retain a meta user `Stop hook feedback`
message containing the exact Block reason.

In the catch-failure case the first handler and fallback both threw. The native
debug/UI says the Stop hook was skipped, one main request ran, and native
completion reported `reason: answer`, `isAborted: false`. There was no Block or
second Stop. This is an intentionally injected fallback failure, not evidence
that a constant-return production fallback spontaneously fails. No worker kill,
managed/prepend protection, competing handler or Codelaya policy was exercised.

Thus a mod catch is a demonstrated recovery candidate for these two callback
failures. It is not by itself a qualified protected completion mechanism. A
production candidate should return its constant denial without relying on I/O
in the fallback; worker loss, ordering, unload and cancellation still require
native proof. Earlier guarded command failures remain separate evidence.

The first capture verifier incorrectly expected a command-hook attachment.
It failed against the native mod transcript; the retained failure led to
checking the exact meta user feedback message instead, alongside independent
outgoing-request propagation. No native run was repaired or relabeled.
The final capture verifier and six negative evidence checks passed on Linux
Node 22 and retained bytes on host Node 20. Negative checks reject foreign
caller, missing timeout, missing fallback failure, erased outgoing reason,
erased durable feedback and aborted outcome; they are not six extra sessions.

```sh
node verify-stop-catch.mjs /home/node/evidence
node tests/verify-stop-catch.test.mjs /home/node/evidence
```

Starting AIKit revision: `0c231220b11fd11dbfdd53c551f7e405e95574b5`.
Image: `sha256:fe24c3f19fe12410b4d57bb1101def27a05a92fa7083611623157a0a2b433e65`.
Executable: `24972e3bc859fab2b46ed4c1e51f7d6130f06d3bd550811a114640de3370d0de`.
Raw captures, initial verifier failure, final verification, source hashes and
network inventory are retained under `.local/stop-catch-20261022` in the parent
Codelaya chat workspace. No readiness, schema or production adapter changes.

| Evidence | SHA-256 |
| --- | --- |
| Requests | `58158717c0ebfbdf231cad329d3a6aa842790439cfbd74ef438627669c874333` |
| Throw events | `9a321b7931389e3527746bf9eaafe2276241f3ca7c3aa2e04e498d9410055392` |
| Timeout events | `83a1705b615a1ddb04474b5b9b8585a36d53a13752e0b7125d617275310a5c2b` |
| Fallback-failure events | `4d1842344c91a702a782f705dbae98c09db1ad1f6034fc390bcf41ec3d8c3b60` |
