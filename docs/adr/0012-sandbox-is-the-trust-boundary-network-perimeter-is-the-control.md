# The sandbox is the trust boundary; the network perimeter is the only in-app control

## Status

accepted

## Context

`aikit serve` exposes the in-process `aikit` agent, whose default toolset includes
`run_bash` (an unrestricted `sh -c`), `write_file`, and `git`. A security audit
(`specs/ISSUES-2026-07-07.md`, SEC-2) framed this as unauthenticated prompt-driven RCE:
a caller who reaches the API can steer the agent into arbitrary shell execution on the
serving host. Two fixes were proposed — gate `run_bash`/`yolo` behind a `--allow-shell`
flag, and/or make the per-agent tool policy (which already exists as
`AgentPersona.tools` / `disallowed_tools`, hard-filtered at `loop_runner.rs:355-360`)
default-deny for dangerous tools.

We rejected both as the *primary* control. An autonomous coding agent whose entire
purpose is to read, write, and run code is not made safe by removing its ability to run
code; it is made safe by running it somewhere disposable. aikit's intended deployment is
inside a sandboxed container (driven by agentrt, the optimization loop, or a chat BFF),
where taking control of the execution environment is expected agent behaviour, not an
exploit.

## Decision

**Protecting the host from the agent is the container's responsibility, not aikit's.**
The in-process agent keeps its full default toolset (`run_bash` included); tool
availability is a *capability* concern expressed per-agent via `AgentPersona.tools` /
`disallowed_tools`, not a host-safety control, and is not gated by any serve flag. There
is no `--allow-shell`.

Because we deliberately do **not** constrain the agent internally, the **network
perimeter is the sole remaining in-application control**, and it must fail closed:
`aikit serve` MUST refuse to start when bound to a non-loopback address without an
`--api-key` (a hard error, not the current warning at `serve/mod.rs:738`). The loopback
default stays open so existing local consumers (agentrt, optimization loop, chat BFF) are
unaffected.

## Consequences

- SEC-2's "RCE" reframes from a code defect to a **deployment contract**: aikit must be
  run in a sandbox. This is documented in the `serve` help text and `webdocs`, and is a
  precondition for any exposure.
- The perimeter carries more weight, not less. The sandbox walls off the *host* but not
  the two things the container still holds — **LLM credentials** (an exposed agent can
  burn or exfiltrate `ANTHROPIC_API_KEY` / the OpenAI-compat key) and **network egress**
  (the agent can reach whatever the container can). Fail-closed auth is the only thing
  standing in front of those, so it is mandatory, not optional.
- `yolo` (client-settable permission bypass) and client-supplied `mcp_servers`
  (SEC-3) are **not** security bugs under this stance — they are in-sandbox capability
  choices. SEC-3's *concurrency cap* (no `max_sessions` on live sessions) remains a real
  resource-exhaustion defect and is tracked independently of this decision.
- `AgentPersona.tools` / `disallowed_tools` already exists and is wired; the remaining
  work is to **plumb a tool policy through serve** (`SendMessageRequest` cannot currently
  carry one) so callers *may* restrict capability when they want to — an opt-in
  least-privilege lever, not a default.
- Operators who cannot sandbox must not expose aikit; there is no in-app substitute for
  the container boundary, by design.
