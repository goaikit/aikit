# Native stored-row read shape

Predeclared 2026-10-24. The installed 2.1.308 declarations expose a stable UUID
on `session.append`, but not on `SessionMessage`/`ApiMessage`. Inspect actual
runtime values before concluding an ID lookup is absent. Fresh ordinary native
Linux session in the pinned 2.1.308 image, network none, no mounts, disposable
profile, public dummy key; no owner policy/credentials or Windows checks.

At SessionStart a diagnostic appends exactly one text-only user note through
`$.session.append`. It records the returned value and one ordinary plus one API
conversation read, then delegates startup normally. No prompt submission, peer
message, inbox auth probe, operation retry or model invocation. Retain actual
keys, row UUID if available, exact synthetic Unicode text and native same-session
transcript after normal exit. Check whether the read retains an ID matching the
stored row; do not assume declaration omissions imply runtime absence.

Inspect the immediate return, subsequent API views and durable transcript
separately. None alone establishes authenticated source, generation admission,
durable lookup by operation/row ID, or actual main-loop consumption. Preserve
failures without repairing/relabeling a submitted trial or promoting readiness.
