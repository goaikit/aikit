# Native child and inbox peer correlation

Predeclared before execution, 2026-10-24. Use ordinary interactive Linux Claude
2.1.308, the existing checksum-pinned image, network none, no mounts and a fresh
disposable node profile. Override the image entrypoint explicitly to a shell.
Do not submit a model prompt or forward/read the messaging token. Windows is
deferred. This does not attest effective hook configuration or a module nonce.

At SessionStart a diagnostic invokes a Python child through the existing native
`process.run` API. The child connects to the inherited inbox address without a
message/auth line, reads kernel SO_PEERCRED and its own bounded /proc ancestry,
and checks whether the peer PID/start time/executable is an actual ancestor.
Record sanitized facts only, with no command arguments or environment dumps.

Controls while the same CLI remains alive: invoke the same probe through an
independent Docker exec using the same socket address, and require that the
native peer matches but is not an ancestor. Exit Claude normally; the same
address must no longer identify a live native peer. Never post a message,
subscribe, create a second agent or infer session/generation identity from PID
alone. Retain every failure; verify both positive and negative relationships.

Follow-on predeclared before its separate native session: load the same fixture
with the Rust observer and compiled `native_caller_inspect` diagnostic. Compare
its peer PID/UID/birth time/hash to the independent Python child facts. A wrong
expected executable hash must fail inside an otherwise genuine native child;
an independent caller using the correct hash must fail while the native peer is
alive. After normal exit the address must fail. No capability or session receipt
is promoted by any of these process relationships.
