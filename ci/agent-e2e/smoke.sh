#!/usr/bin/env bash
#
# Real-agent smoke suite. Runs an ACTUAL Claude Code turn through aikit against
# the injected LLM gateway, then asserts the capture + sync pipeline end-to-end.
#
# Deliberately asserts the PIPELINE, not the model's wording — a modest
# self-hosted model behind the gateway must still make this pass. What we prove:
#   1. `aikit agent run --agent claude` completes a real turn (exit 0).
#   2. A session transcript was captured under ~/.claude/projects.
#   3. `aikit session sync --dry-run` detects and would sync that transcript.
#
# Required env (injected by the nightly workflow from repo secrets):
#   ANTHROPIC_BASE_URL   — gateway Anthropic-compatible base URL
#   ANTHROPIC_AUTH_TOKEN — gateway API key
set -euo pipefail

: "${ANTHROPIC_BASE_URL:?ANTHROPIC_BASE_URL (gateway URL) is required}"
: "${ANTHROPIC_AUTH_TOKEN:?ANTHROPIC_AUTH_TOKEN (gateway key) is required}"
export ANTHROPIC_BASE_URL ANTHROPIC_AUTH_TOKEN

# Pin the model the gateway key is allowed to serve. Claude Code otherwise
# defaults to the newest model, which the gateway may reject (403 "key not
# allowed to access model"). Override via AGENT_E2E_MODEL as the gateway config
# changes.
MODEL="${AGENT_E2E_MODEL:-claude-sonnet-4-6}"

echo "== aikit version =="
aikit --version || true
echo "== gateway: ${ANTHROPIC_BASE_URL}  model: ${MODEL} =="

# 1. A real agent turn via the gateway. We don't assert the reply text (the
#    backing model may be small); a clean exit means a turn round-tripped.
echo "== [1/3] running a real claude turn =="
aikit agent run --agent claude --model "${MODEL}" --prompt "Reply with the single word: pong"
echo "   turn completed (exit 0)"

# 2. The turn must have produced a captured session transcript.
echo "== [2/3] checking captured session files =="
shopt -s nullglob
mapfile -t sessions < <(find "$HOME/.claude/projects" -name '*.jsonl' 2>/dev/null)
if [ "${#sessions[@]}" -eq 0 ]; then
  echo "FAIL: no session transcript captured under ~/.claude/projects" >&2
  exit 1
fi
echo "   captured ${#sessions[@]} session file(s)"

# 3. Sync must detect the transcript. Dry-run keeps it network-free (no bucket):
#    it detects + scrubs + hashes and reports what it WOULD upload.
echo "== [3/3] session sync --dry-run detects the transcript =="
summary="$(aikit session sync --tool claude_code --owner ci --dry-run --format json)"
echo "   summary: ${summary}"
detected="$(echo "${summary}" | jq -r '.synced // 0')"
if [ "${detected}" -lt 1 ]; then
  echo "FAIL: session sync did not detect the captured transcript (synced=${detected})" >&2
  exit 1
fi

echo "SMOKE PASS: real agent turn captured and sync-detected (synced=${detected})"
