# Contributing to aikit-sdk

`aikit-sdk` is the stable integration surface for catalog/path/deploy/run behavior.
Changes here impact both the `aikit` CLI and `aikit-py`.

## Validation

Run from workspace root:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test -p aikit-sdk
```

If your change touches event streaming or platform-specific runner behavior, also run:

```bash
cargo test -p aikit-sdk -- --ignored
```

## Guidelines

- Keep APIs deterministic and explicit.
- Avoid hidden fallback logic that changes output shape or path resolution silently.
- Preserve compatibility of event payload contracts and error semantics.
- Update `README.md` when public behavior changes.
- Add tests for new path rules, deployment logic, and run/event behavior.

## References

- `README.md`
- `../aikit-py/README.md`
- `../CONTRIBUTING.md`
