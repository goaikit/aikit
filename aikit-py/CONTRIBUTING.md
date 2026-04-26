# Contributing to aikit-py

`aikit-py` is a thin Python binding over `aikit-sdk`.
Keep behavior aligned with SDK semantics and avoid Python-only divergence.

## Local setup

```bash
cd aikit-py
python -m venv .venv
source .venv/bin/activate
pip install -U pip maturin
maturin develop
```

## Validation

Run from workspace root:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test -p aikit-py
```

## Guidelines

- Preserve API naming and output shapes already exposed in `aikit_py`.
- Keep error mapping explicit and predictable.
- When SDK behavior changes, update Python binding docs and tests in the same PR.
- Keep docs concise and code-path accurate.

## References

- `README.md`
- `../aikit-sdk/README.md`
- `../CONTRIBUTING.md`
