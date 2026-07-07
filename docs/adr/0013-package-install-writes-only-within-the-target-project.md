# Package install writes only within the target project

## Status

accepted (companion to [0012](0012-sandbox-is-the-trust-boundary-network-perimeter-is-the-control.md))

## Context

`aikit install <source>` fetches a package and copies its artifacts into a project
according to the package's `aikit.toml` `[artifacts]` table. The destination strings come
from an untrusted manifest and were used as `project_root.join(dest_str)` with no
validation (`aikit-sdk/src/install.rs:153`). `Path::join` with an absolute value replaces
the base, and `..` was never rejected, so a package could write outside the project — e.g.
`"payload/**" = "/home/victim/.ssh"` or `"../../../etc/cron.d"` (audit SEC-1). Related
traversal exists via skill/subagent names and `source` paths (SEC-5).

Unlike `aikit serve` (ADR 0012), `install` does **not** run in a sandbox. It runs on a
developer's own machine with their real `~/.ssh`, cloud credentials, and shell rc files
present. The container trust boundary does not apply here, so the "assume sandboxed"
stance does not transfer: install is a distinct, un-sandboxed boundary. The npm/cargo
"install runs arbitrary code" precedent is a known flaw to avoid, not a licence to copy —
and a *declarative* artifact manifest never legitimately needs to write outside the
project it is scaffolding.

## Decision

A package may write **only within the target project root**. Enforcement:

1. **Parse-time validation.** Every artifact destination (and skill/subagent name and
   `source`) is validated when the manifest is deserialized, before any file is written.
   A single unsafe mapping **aborts the entire install** with a clear error — no partial
   writes, one error site.
2. **Reject lexically:** absolute paths and any `..` component are refused
   (`safe_join(base, untrusted)`).
3. **No-follow-symlinks on the write path.** Extraction/copy refuses to follow symlinks,
   closing the escape where one artifact writes `link -> /etc` and a later mapping targets
   `link/…` (which a lexical-only check would pass).
4. Flat identifiers used to build filenames (package `name`/`version` → cache dir, SEC-4;
   client `session_id` → session file, SEC-10) are validated with a strict id charset
   (`is_safe_id`), not `safe_join`, since they are not path fragments.

This logic lives once in `aikit-sdk` and is shared by every install/extract path,
consistent with the ARCH-1 direction of unifying the duplicated install/fetch stacks.

## Consequences

- SEC-1 and SEC-5 are closed at the source: a hostile or malformed package cannot escape
  the project directory.
- Legitimate templates are unaffected — they only ever write within the project they
  scaffold.
- Zip-slip on *extraction* was already mitigated; this closes the separate
  artifact-*mapping* layer that sat above it.
- The `safe_join` / `is_safe_id` helpers become the single validation seam reused by
  SEC-1/4/5/10, so future install/session code inherits the guarantee by construction.
