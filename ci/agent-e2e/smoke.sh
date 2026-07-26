#!/usr/bin/env bash
#
# Real-agent smoke suite. Runs ACTUAL agent turns through aikit against the
# injected LLM gateway, then asserts the capture + sync pipeline end-to-end.
#
# Deliberately asserts PIPELINE/STRUCTURE, never the model's wording — a
# modest self-hosted model behind the gateway must still make every case pass.
#
# Cases (registered via run_case, in order):
#   1. case_claude_turn       — a real claude turn completes, captured + sync-detected.
#   2. case_claude_tool_use   — claude actually invokes a tool (file write), not just describes it.
#   3. case_streaming         — `--events` emits parseable AgentEvent JSON lines.
#   4. case_resume_claude     — a second turn with --resume appends to the SAME transcript.
#   5. case_codex             — same turn/sync shape via the Codex backend (skips unless configured).
#
# Every case function returns 0 (pass) or non-zero (fail); run_case aggregates
# into FAILURES and prints a PASS/FAIL/SKIP line per case, then a summary.
#
# Required env for the Claude cases (injected by the nightly workflow from
# repo secrets):
#   ANTHROPIC_BASE_URL   — gateway Anthropic-compatible base URL
#   ANTHROPIC_AUTH_TOKEN — gateway API key
#   AGENT_E2E_MODEL      — model the gateway key may serve (default claude-sonnet-4-6)
#
# Optional env for the Codex case (case skips cleanly if any is unset):
#   CODEX_BASE_URL — gateway OpenAI-compatible base URL (incl. any /v1 suffix)
#   CODEX_API_KEY  — gateway API key for that route
#   CODEX_MODEL    — model to run through Codex
set -uo pipefail

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

# ---------------------------------------------------------------------------
# Harness
# ---------------------------------------------------------------------------
FAILURES=0
CASE_NAMES=()
CASE_RESULTS=()

# run_case <name> <fn>
run_case() {
  local name="$1"
  local fn="$2"
  echo ""
  echo "== case: ${name} =="
  SKIPPED=0
  if "${fn}"; then
    if [ "${SKIPPED}" -eq 1 ]; then
      echo "[${name}] PASS (skipped)"
      CASE_RESULTS+=("SKIP")
    else
      echo "[${name}] PASS"
      CASE_RESULTS+=("PASS")
    fi
    CASE_NAMES+=("${name}")
  else
    echo "[${name}] FAIL" >&2
    FAILURES=$((FAILURES + 1))
    CASE_NAMES+=("${name}")
    CASE_RESULTS+=("FAIL")
  fi
}

# skip <reason> — call from inside a case function, then `return 0`. Marks
# the case as skipped (still counts as pass) for the summary/verdict.
SKIPPED=0
skip() {
  local reason="$1"
  echo "[SKIP] ${reason}"
  SKIPPED=1
}

# newest_claude_transcript — echoes the path of the most-recently-modified
# ~/.claude/projects/*/*.jsonl transcript, or nothing if none exist.
newest_claude_transcript() {
  find "$HOME/.claude/projects" -name '*.jsonl' -printf '%T@ %p\n' 2>/dev/null \
    | sort -rn \
    | head -n1 \
    | cut -d' ' -f2-
}

# ---------------------------------------------------------------------------
# Case 1: baseline claude turn — completes, captured, sync-detected.
# ---------------------------------------------------------------------------
case_claude_turn() {
  echo "running a real claude turn"
  if ! aikit agent run --agent claude --model "${MODEL}" --prompt "Reply with the single word: pong"; then
    echo "FAIL: claude turn did not exit 0" >&2
    return 1
  fi
  echo "  turn completed (exit 0)"

  shopt -s nullglob
  mapfile -t sessions < <(find "$HOME/.claude/projects" -name '*.jsonl' 2>/dev/null)
  if [ "${#sessions[@]}" -eq 0 ]; then
    echo "FAIL: no session transcript captured under ~/.claude/projects" >&2
    return 1
  fi
  echo "  captured ${#sessions[@]} session file(s)"

  local summary detected
  summary="$(aikit session sync --tool claude_code --owner ci --dry-run --format json)"
  echo "  summary: ${summary}"
  detected="$(echo "${summary}" | jq -r '.synced // 0' 2>/dev/null)"
  [[ "${detected}" =~ ^[0-9]+$ ]] || detected=0
  if [ "${detected}" -lt 1 ]; then
    echo "FAIL: session sync did not detect the captured transcript (synced=${detected})" >&2
    return 1
  fi
  echo "  synced=${detected}"
  return 0
}

# ---------------------------------------------------------------------------
# Case 2: tool-use — claude must actually CALL its file-writing tool, not just
# describe doing so. Asserted via the captured transcript's tool_use blocks,
# never the model's prose.
# ---------------------------------------------------------------------------
case_claude_tool_use() {
  local workdir
  workdir="$(mktemp -d)"
  echo "workdir: ${workdir}"

  if ! aikit agent run --agent claude --model "${MODEL}" \
      --cd "${workdir}" --skip-git-repo-check \
      --prompt "You MUST actually call your file-writing tool right now to create a file named e2e-proof.txt in the current directory containing exactly the text: ready
Do not merely describe or explain the action — invoke the tool and perform the write."; then
    echo "FAIL: claude tool-use turn did not exit 0" >&2
    return 1
  fi

  local transcript
  transcript="$(newest_claude_transcript)"
  if [ -z "${transcript}" ]; then
    echo "FAIL: no session transcript captured under ~/.claude/projects" >&2
    return 1
  fi
  echo "  transcript: ${transcript}"

  local tool_use_count
  tool_use_count="$(jq -s '[.[] | select(.type=="assistant") | .message.content[]? | select(.type=="tool_use")] | length' "${transcript}" 2>/dev/null || echo 0)"
  if [ -z "${tool_use_count}" ] || [ "${tool_use_count}" -lt 1 ]; then
    echo "FAIL: no tool_use block found in captured transcript (authoritative assertion)" >&2
    return 1
  fi
  echo "  transcript contains ${tool_use_count} tool_use block(s) [authoritative]"

  # Secondary signal only — log it, don't gate on it (some tools may sandbox
  # writes or the model may pick a different filename despite instructions).
  if [ -f "${workdir}/e2e-proof.txt" ]; then
    echo "  secondary signal: ${workdir}/e2e-proof.txt exists ($(cat "${workdir}/e2e-proof.txt" 2>/dev/null))"
  else
    echo "  secondary signal: ${workdir}/e2e-proof.txt NOT found (not gating on this)"
  fi

  return 0
}

# ---------------------------------------------------------------------------
# Case 3: streaming — `--events` prints one JSON AgentEvent per line.
# ---------------------------------------------------------------------------
case_streaming() {
  local outfile
  outfile="$(mktemp)"

  if ! aikit agent run --agent claude --model "${MODEL}" --events \
      --prompt "Reply with the single word: pong" >"${outfile}" 2>/dev/null; then
    echo "FAIL: streaming claude turn did not exit 0" >&2
    rm -f "${outfile}"
    return 1
  fi

  local json_lines
  json_lines="$(jq -c '.' "${outfile}" 2>/dev/null | wc -l)"
  echo "  ${json_lines} line(s) parsed as JSON"
  if [ "${json_lines}" -lt 2 ]; then
    echo "FAIL: expected at least 2 parseable JSON event lines, got ${json_lines}" >&2
    rm -f "${outfile}"
    return 1
  fi

  local marked
  marked="$(jq -s '[.[] | select(.payload.stream_message != null or .payload.result != null)] | length' "${outfile}" 2>/dev/null || echo 0)"
  if [ -z "${marked}" ] || [ "${marked}" -lt 1 ]; then
    echo "FAIL: no event line had .payload.stream_message or .payload.result" >&2
    rm -f "${outfile}"
    return 1
  fi
  echo "  ${marked} line(s) carried stream_message/result payload"

  rm -f "${outfile}"
  return 0
}

# ---------------------------------------------------------------------------
# Case 4: resume — `--resume-last` doesn't work for external agents (only the
# built-in backend updates the session index), so we glob the newest
# transcript for its session id (the filename stem) and pass it via --resume.
# Resume must append to the SAME transcript, not start a new session.
# ---------------------------------------------------------------------------
case_resume_claude() {
  local workdir
  workdir="$(mktemp -d)"
  echo "workdir: ${workdir}"

  if ! aikit agent run --agent claude --model "${MODEL}" \
      --cd "${workdir}" --skip-git-repo-check \
      --prompt "Remember the number 42."; then
    echo "FAIL: first (seed) turn did not exit 0" >&2
    return 1
  fi

  local transcript sid
  transcript="$(newest_claude_transcript)"
  if [ -z "${transcript}" ]; then
    echo "FAIL: no session transcript captured after the seed turn" >&2
    return 1
  fi
  sid="$(basename "${transcript}" .jsonl)"
  echo "  seed session id: ${sid}"

  if ! aikit agent run --agent claude --model "${MODEL}" \
      --cd "${workdir}" --skip-git-repo-check --resume "${sid}" \
      --prompt "What number did I ask you to remember?"; then
    echo "FAIL: resumed turn did not exit 0" >&2
    return 1
  fi

  local transcript2 sid2
  transcript2="$(newest_claude_transcript)"
  sid2="$(basename "${transcript2}" .jsonl)"
  if [ "${sid2}" != "${sid}" ]; then
    echo "FAIL: resume did not append to the same transcript (seed=${sid} newest=${sid2})" >&2
    return 1
  fi
  echo "  resume appended to the same session (${sid2})"

  return 0
}

# ---------------------------------------------------------------------------
# Case 5: codex — same turn/sync shape via the Codex backend. Skips cleanly
# unless the gateway route + credentials are configured.
# ---------------------------------------------------------------------------
case_codex() {
  if [ -z "${CODEX_BASE_URL:-}" ] || [ -z "${CODEX_API_KEY:-}" ] || [ -z "${CODEX_MODEL:-}" ]; then
    skip "CODEX_BASE_URL/CODEX_API_KEY/CODEX_MODEL not all set — codex case skipped"
    return 0
  fi

  export CODEX_API_KEY
  local codex_home="${CODEX_HOME:-$HOME/.codex}"
  mkdir -p "${codex_home}"
  cat >"${codex_home}/config.toml" <<EOF
model = "${CODEX_MODEL}"
model_provider = "e2e"

[model_providers.e2e]
name = "e2e"
base_url = "${CODEX_BASE_URL}"
env_key = "CODEX_API_KEY"
wire_api = "chat"
EOF
  echo "  wrote ${codex_home}/config.toml (provider e2e -> ${CODEX_BASE_URL})"

  if ! aikit agent run --agent codex --yolo --model "${CODEX_MODEL}" \
      --skip-git-repo-check --prompt "Reply with the single word: pong"; then
    echo "FAIL: codex turn did not exit 0" >&2
    return 1
  fi
  echo "  turn completed (exit 0)"

  mapfile -t codex_sessions < <(find "${codex_home}/sessions" -name '*.jsonl' 2>/dev/null)
  if [ "${#codex_sessions[@]}" -eq 0 ]; then
    echo "FAIL: no codex session file under ${codex_home}/sessions" >&2
    return 1
  fi
  echo "  captured ${#codex_sessions[@]} codex session file(s)"

  local summary detected
  summary="$(aikit session sync --tool codex --owner ci --dry-run --format json)"
  echo "  summary: ${summary}"
  detected="$(echo "${summary}" | jq -r '.synced // 0' 2>/dev/null)"
  [[ "${detected}" =~ ^[0-9]+$ ]] || detected=0
  if [ "${detected}" -lt 1 ]; then
    echo "FAIL: session sync did not detect the captured codex transcript (synced=${detected})" >&2
    return 1
  fi
  echo "  synced=${detected}"

  return 0
}

# ---------------------------------------------------------------------------
# Register + run cases
# ---------------------------------------------------------------------------
run_case "claude_turn"     case_claude_turn
run_case "claude_tool_use" case_claude_tool_use
run_case "streaming"       case_streaming
run_case "resume_claude"   case_resume_claude
run_case "codex"           case_codex

echo ""
echo "== summary =="
pass_count=0
fail_count=0
skip_count=0
for i in "${!CASE_NAMES[@]}"; do
  echo "  ${CASE_RESULTS[$i]}  ${CASE_NAMES[$i]}"
  case "${CASE_RESULTS[$i]}" in
    PASS) pass_count=$((pass_count + 1)) ;;
    FAIL) fail_count=$((fail_count + 1)) ;;
    SKIP) skip_count=$((skip_count + 1)) ;;
  esac
done
echo "  ${pass_count} passed, ${fail_count} failed, ${skip_count} skipped"

if [ "${FAILURES}" -gt 0 ]; then
  echo "SMOKE FAIL (${FAILURES} case(s))" >&2
  exit 1
fi

echo "SMOKE PASS"
exit 0
