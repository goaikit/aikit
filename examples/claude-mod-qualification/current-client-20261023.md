# Current-client inspection: Claude 2.1.308

Observed 2026-10-23. The public npm registry reported `2.1.308`; this is a pinned
qualification candidate, not a floating production dependency or readiness
promotion. The existing 2.1.296 baseline and its recorded scopes are preserved.

## Reproducible public image

`Dockerfile.native` now accepts an explicit version/checksum pair while keeping
both original defaults. The build checks the downloaded native executable and
its version. Overriding the version alone does not bypass checksum validation.
The following candidate build passed on Linux x86-64:

```sh
docker build -f Dockerfile.native -t aikit-claude-native-probe:2.1.308 \
  --build-arg CLAUDE_VERSION=2.1.308 \
  --build-arg CLAUDE_SHA256=356733f185fa988e6bf3aa9a555269fdd167982a2e2a80d3c8a78037bb92ad58 .
```

The executed build context contained only a copy of this Dockerfile, with no
application code, owner credentials or settings. Image identity:
`sha256:9312badacd0cdf5496308a7292a5dc9965d25c616dbda94764ee10bbe8c30924`.
Its actual global native executable matches the checksum above and reports
`2.1.308 (Claude Code)`. The default 2.1.296 image was not rebuilt this turn;
its original image/checksum and earlier qualification remain unchanged.

## Native API inspection

An empty discovery container separately installed the same public version,
completed ordinary onboarding, then disconnected external networking before
copying the existing pass-through mod or trusting its workspace. An ordinary
interactive session loaded that mod, generated the version-specific API
declarations and exited normally without a model prompt. This is API discovery,
not transport or completion qualification.

Compared with the retained 2.1.296 declarations, the exported file has 1189 added
and 230 removed lines. Its SHA-256 is
`3c29d70e7f113f2409db12c3db20dd8385296c46aaf26984d00e5ecacd37c2c0`.
Two relevant findings from **declaration inspection**:

- `session.identity()` describes account authentication and managed organization
  identity. It is not a local process/session-invocation credential, does not
  authenticate the eventual stdio child, and does not close NativeSessionIdentity.
  Do not request owner login or change machine policy to substitute this API.
- `settings.read` is described as middleware over settings layers. Its merge can
  be changed by hooks. A settings object is not by itself an independent witness
  of the active handler chain, its ordering or protected execution. No actual
  native settings-rewrite experiment is claimed here.

The [official reference](https://code.claude.com/docs/en/plugins/mods/reference)
and [release notes](https://github.com/anthropics/claude-code/blob/main/CHANGELOG.md)
also describe post-2.1.296 changes. Trust the installed version's generated
declarations where they differ, and qualify native behavior separately.

## Actual consumer compatibility

In a distinct container from the checksum-pinned candidate image, the actual
Codelaya CLI and owned SDK completion module passed the existing name/note repair
scenario with nine main scripted loopback requests. Missing name and missing
current file note blocked completion; genuine native repairs succeeded, followed
by mod/downstream-command Allow and correlated native accepted completion.
Application records were already Completed before projecting inspection and
remained Completed after ordinary exit. Git HEAD/index and snapshot bytes were
independently verified. The existing consumer capture verifier passed unchanged
inside Linux and against retained host files.

This used the existing published SDK `f5827b8` module and retained consumer
binaries; no SDK Rust/JS runtime logic, schema or capability assessment changed.
Normal consumer status remained registered, inactive and unready (exit 1).
This healthy-path compatibility result does not transfer older failure or
competing-handler results to 2.1.308. Its identity/configuration, deadlines,
admission, messaging proof, Windows and other-provider gates remain open.
The opt-in message framing guard still refuses versions other than 2.1.296;
that guard was not relaxed on the strength of a completion run.

Raw API/build/native evidence and the predeclared protocol remain in the parent
chat workspace at `.local/claude-current-20261023`, excluded from Git. No owner
profile or real model credential was used. The scripted endpoint does not prove
model semantic compliance. No new regression-suite results are counted for
unchanged SDK runtime code.
