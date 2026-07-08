# Codex test fixtures

Vendored from `superbased-observer/testdata/codex/` at
https://github.com/marmutapp/superbased-observer (Apache-2.0).

## Files

| File | What it exercises |
|---|---|
| `rollout-session.jsonl` | Classic rollout: session_configured → user_message → 3 tool_call/tool_output pairs (file_read, shell with failure, web_search) → 2 token_count cumulative envelopes. |
| `rollout-response-item.jsonl` | Codex Desktop / newer-build shape: session_meta + turn_context + event_msg dispatch (task_started / user_message / agent_message / exec_command_end) + response_item function_call / custom_tool_call / web_search_call / function_call_output / message. Exercises the response_item payload branch and the message role taxonomy. |
| `with-secrets.jsonl` (synthetic) | One tool_call carrying AWS key, AWS secret, GitHub PAT, JWT, Anthropic key, OpenAI key, connstring — for the scrub invariant test. NOT from observer; hand-authored. |
