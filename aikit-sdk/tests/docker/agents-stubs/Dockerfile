FROM debian:bookworm-slim

# Install stub scripts for all five supported agents.
# Each stub emits a few JSONL lines to stdout and exits 0.

RUN apt-get update && apt-get install -y --no-install-recommends \
    bash \
    && rm -rf /var/lib/apt/lists/*

COPY stubs/codex /usr/local/bin/codex
COPY stubs/claude /usr/local/bin/claude
COPY stubs/gemini /usr/local/bin/gemini
COPY stubs/opencode /usr/local/bin/opencode
COPY stubs/agent /usr/local/bin/agent

RUN chmod +x \
    /usr/local/bin/codex \
    /usr/local/bin/claude \
    /usr/local/bin/gemini \
    /usr/local/bin/opencode \
    /usr/local/bin/agent
