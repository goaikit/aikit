# Earlier-handler input snapshot comparison

Predeclared before execution, 2026-10-24. Reuse the existing bounded input
comparison and ordinary interactive Claude 2.1.308. Windows remains deferred.

Reverse only the diagnostic handler tiers: `input-shortcut` is prepend and
`input-observer` is user. The mutator appends the same late UTF-8 row through the
real API before the observer reads its pre-delegation snapshot. Neither handler
rewrites request metadata or supplies a synthetic answer.

Submit the same control and mutation prompts. Require two real main requests,
genuine stored row UUID, original session/Turn correlation, unchanged settings,
the intended native handler order, and no hook failures. Independently compare
the late row in the observer's pre/post snapshots with actual main HTTP bodies.
Do not classify title traffic as main-loop consumption.

Hypothesis: the earlier append can appear in the pre snapshot while remaining
absent from that request, because Claude froze request content before this
handler chain. Either outcome is retained. A counterexample would rule out an
arbitrary pre-read plus settled core trace as a universal exact-input witness;
it would not demonstrate a false receipt in the current SDK or prove that every
other existing Claude API lacks a solution.

Use the pinned existing image, network none, no mounts, fresh disposable node
profile, public dummy key and loopback endpoint. Existing ceilings remain 12
requests / 4 MiB per body, with no external model call or proxy. Exit normally,
capture, then run `node verify.mjs EVIDENCE --early`. Preserve failed attempts.
