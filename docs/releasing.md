# Releasing

`main` is protected (required checks, admins included), so no workflow pushes to it.
A release is cut by merging a version bump PR; the release is built from the tag on
that merge commit.

- Workflows: `.github/workflows/auto-release.yml`, `.github/workflows/release.yml`
- Version source: `version` under `[package]` in the root `Cargo.toml`
- Tags: `v<version>`, e.g. `v0.1.197`

## The automatic flow

1. **A change merges to `main`.** Auto Release sees that `v<Cargo.toml version>` is
   already tagged, so this is not a release commit. It opens (or rebuilds) the bump PR
   on `release/next-version`, raising the patch version by one from `main`'s tip.
2. **A maintainer merges the bump PR** once its checks pass. Several changes can
   land first; the PR is rebuilt on each push and releases all of them together.
3. **Auto Release runs on the merge commit.** Its `Cargo.toml` version has no tag yet,
   so it tags that exact commit `v<version>`.
4. **Release runs** (`workflow_run`, only when Auto Release concluded `success`). It
   releases only if the triggering commit carries the tag matching its `Cargo.toml`
   version. It builds all binaries from the tagged commit, publishes the GitHub
   release, and updates the Homebrew (`goaikit/homebrew-cli`) and Scoop
   (`goaikit/scoop-bucket`) manifests.

Pushes that only touch docs or other `paths-ignore` paths, or carry `[skip release]`,
`[no release]`, `[skip ci]`, or a `no-release` label on the PR, do not run Auto
Release at all.

## Guarantees

- **A failed Auto Release never publishes.** Release skips unless the run succeeded.
- **Binaries always match their tag.** Every job checks out the commit the tag
  resolved to, never a branch head. If the tag moves mid-run, publishing fails.
- **A published release is immutable.** If `v<version>` is already published,
  Release fails with an error instead of deleting and re-uploading it. Only a leftover
  *draft* from an earlier failed run is replaced. To ship a fix, release a new version.

## Manual release

Use this to bump something other than the patch version, or if the bump PR automation
is unavailable.

1. Open a normal PR that sets the root `Cargo.toml` `version` (e.g. `0.2.0`). Merge it.
2. Auto Release tags the merge commit and Release publishes it, exactly as above.

If Auto Release cannot run (e.g. the GitHub App token is broken), tag and dispatch by
hand. The tag must point at a commit on `main` whose `Cargo.toml` has that version:

```bash
git tag v0.2.0 <merge-commit-sha>
git push origin v0.2.0
gh workflow run release.yml -f version=0.2.0
```

`workflow_dispatch` also retries a release whose run failed before publishing. It
refuses versions that are already published.

## Prerequisites

- Secrets `GH_APP_ID` and `GH_APP_PRIVATE_KEY`: a GitHub App installed on
  `goaikit/aikit` with **Contents: write** and **Pull requests: write**. It pushes
  `release/next-version` and opens the PR. It is also used on `homebrew-cli` and
  `scoop-bucket`. A PR opened with `GITHUB_TOKEN` would never trigger CI, so its
  required checks would never report.
- The `GITHUB_TOKEN` must be allowed to push tags (no tag ruleset blocking
  `github-actions[bot]`).
