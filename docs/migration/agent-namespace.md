# Migration Guide: `aikit agent` Namespace (v0.1.110+)

## Overview

Agent-related commands have been consolidated under the `aikit agent` subcommand group. The old top-level commands still work but print a deprecation warning. They will be removed in the next major release.

## Command Mapping

| Old command | New command | Status |
|-------------|-------------|--------|
| `aikit run --agent <KEY> ...` | `aikit agent run --agent <KEY> ...` | Deprecated alias |
| `aikit agents` | `aikit agent list` | Deprecated alias |
| `aikit mcp list` | `aikit agent mcp list` | Deprecated alias |
| `aikit mcp add ...` | `aikit agent mcp add ...` | Deprecated alias |
| `aikit check` | `aikit check` (unchanged) | Not deprecated |
| _(new)_ | `aikit agent check` | Agent CLIs only |

## Deprecation Timeline

- **v0.1.110**: `aikit agent` group introduced; old top-level aliases emit deprecation warnings.
- **Next major release**: Deprecated aliases (`aikit run`, `aikit agents`, `aikit mcp list`, `aikit mcp add`) will be removed.

## Python SDK: `session_id` Parameter

`RunOptions::session_id` has existed since v0.1.x. Starting in this release the Python bindings also expose it.

**Before:**

```python
from aikit import run_agent

result = run_agent("claude", "hello")
```

**After (with session resume):**

```python
from aikit import run_agent

# Start a new session
result = run_agent("claude", "hello")

# Resume an existing session
result = run_agent("claude", "continue from where we left off", session_id="abc123")
```

The `session_id` parameter is optional and defaults to `None`. Existing call sites require no changes.

The same parameter is available on `run_agent_events`:

```python
from aikit import run_agent_events

def on_event(event):
    print(event)

result = run_agent_events("claude", "hello", on_event, session_id="abc123")
```
