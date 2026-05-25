# Magic-tool output capture is text + retry, not function-calling

Magic tools capture their structured `Draft` by parsing a ` ```json ` block from the agent's text reply and
validating it against the tool's `outputSchema`, retrying with the validation errors on failure (via
`aikit_sdk::Pipeline`). We deliberately do **not** inject a synthetic `emit_output` host tool and capture
the Draft from a function call.

## Why

`ToolDef.agent_key` is configurable: `"aikit"` runs the in-process loop, any other key spawns a runnable
agent CLI (codex, claude, …). A `HostToolProvider` (the only way to inject `emit_output`) works **only** for
the in-process `"aikit"` backend — spawned runnable agents cannot be handed an ad-hoc host tool. So
function-calling capture would be available for some tools and not others. Text + retry is the only capture
mechanism that is uniform across every backend, and the framework injects the `outputSchema` into the prompt
(`compose_prompt`) so the first attempt already targets the right shape.

## Consequences

- One capture path for all backends; no per-backend branching in the executor.
- Reliability rests on prompt-embedded schema + retry, not a hard function-call contract — acceptable given
  the schema is always injected. Do not re-add `emit_output` without first making tool injection work across
  runnable-agent backends too.
