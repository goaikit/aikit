# Main-input snapshot boundary comparison

Predeclared 2026-10-24 before native execution. Linux Claude 2.1.308 only;
Windows remains deferred. No production capability or receipt is promoted.

Question: does an API-view snapshot at an outer `turn.step` handler, together
with a matching settled core trace, describe the actual main request when a
later handler appends a conversation row before calling core?

Use the existing checksum-pinned native image in a fresh network-none container,
without host mounts or owner configuration/credentials. Seed only disposable
offline onboarding, accept normal terminal workspace trust, and start the agent
as an ordinary interactive CLI. A deterministic loopback endpoint handles at
most 12 requests, 4 MiB each; it never proxies. No real-model semantics claimed.

The observer loads at prepend tier and captures API-view messages immediately
before delegation, then captures the settled core trace and API view afterward.
The second user-tier module delegates the control unchanged. On the mutation
prompt it appends one small UTF-8 user row through `session.append`, records its
actual result, then delegates the original step once. It does not rewrite step
metadata, use a managed session, send peer messages or change permission policy.

Submit `AIKIT_INPUT_CONTROL`, await completion, then `AIKIT_INPUT_MUTATION`.
After completion exit normally. Require native debug to verify actual ordering,
one matching core dispatch per prompt, one genuine append, exact session/Turn
correlation and independently captured main HTTP bodies. Classify title/other
requests separately. Preserve failures, source identities and transcript.

Compare pre/post API snapshots, the core messageCount and actual request's text
blocks. Report whether the late row entered that request and whether count or
snapshot differences detect it. An unchanged control is required. Neither
possible outcome alone proves complete consumption, durable lookup or native
admission; use it to decide what the existing APIs actually establish.
