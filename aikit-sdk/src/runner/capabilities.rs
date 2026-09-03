//! Declared, per-Backend capabilities.
//!
//! A [`BackendCapabilities`] answers "what can this Backend emit/do," letting
//! callers subscribe to (or require) richer behaviour only from Backends that
//! actually provide it, instead of assuming the lowest common denominator.
//!
//! Each [`Backend`](crate::runner::backend::Backend) declares its capabilities
//! via an exhaustive match, so adding a Backend forces a capabilities decision.
//!
//! Values are conservative: a field that is `false` today and flips to `true`
//! later (as a Backend's decode is upgraded) is a non-breaking change; the
//! reverse would break callers. See spec 006 and ADRs 0007-0009.

/// What a Backend is able to emit or do.
///
/// `#[non_exhaustive]` so later specs (e.g. spec 005) can add fields without a
/// breaking change. Construct via [`BackendCapabilities::NONE`] and the
/// builder-style `with_*` setters, or the `const fn` constructor used by the
/// per-Backend tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub struct BackendCapabilities {
    /// Has a Control channel (approvals / interrupts / turn lifecycle).
    pub bidirectional: bool,
    /// Emits typed `tool_use` / `tool_result` structures.
    pub structured_tools: bool,
    /// Emits thinking / reasoning frames.
    pub reasoning: bool,
    /// Emits structured file-edit frames.
    pub file_changes: bool,
    /// Can be cancelled mid-turn cleanly.
    pub interruptible: bool,
    /// Supports `--resume` / session-id resumption.
    pub resumable_sessions: bool,
    /// Routes MCP servers.
    pub mcp_routing: bool,
    /// Emits hook events.
    pub hooks: bool,
    /// Emits server-side / advisor tool calls.
    pub server_tools: bool,
    /// Spawns sub-agents.
    pub subagents: bool,
    /// Emits context-compression events.
    pub context_compression: bool,
    /// The Backend's on-disk session format is parseable by `aikit-session-capture`.
    /// `false` ⇒ no adapter is registered for this Backend; passive capture
    /// is unavailable, not merely empty. Spec 010.
    pub passive_capture: bool,
    /// Enforces `AgentPersona.tools` / `disallowed_tools` (D2 / ADR 0012 least-privilege tool
    /// policy) as a hard filter. `false` ⇒ a tool policy handed to this Backend is silently
    /// unenforceable — callers threading a persona/tool-policy through `RunOptions` must reject
    /// rather than accept-and-drop it. Currently only the in-process `aikit` Backend enforces it.
    pub supports_tool_policy: bool,
    /// The Backend persists browsable session transcripts that aikit can read
    /// via [`HistoryReader`](crate::history::HistoryReader). `false` = the
    /// Backend is opaque; history is unsupported, not merely empty. Spec 008.
    pub history_store: bool,
    /// History metadata mutations (rename/tag) are supported. Implies nothing
    /// without `history_store`. Distinct because a store may be read-only.
    /// Spec 008.
    pub history_mutations: bool,
    /// The decoder emits a status-bearing [`Decoded::Terminal`] frame for a
    /// completed run.
    ///
    /// `false` = this Backend's outcome is not observable from its stream, so
    /// a stream that ends without one says nothing. `true` = a run whose
    /// stream ends with no terminal frame did not complete, and a consumer may
    /// treat that as an error.
    ///
    /// Like every flag here this describes the decoder as it actually is: a
    /// flag that outruns its decoder is a bug, not a roadmap (ADR 0019).
    pub terminal_event: bool,
}

impl BackendCapabilities {
    /// All capabilities off — the conservative baseline.
    pub const NONE: BackendCapabilities = BackendCapabilities {
        bidirectional: false,
        structured_tools: false,
        reasoning: false,
        file_changes: false,
        interruptible: false,
        resumable_sessions: false,
        mcp_routing: false,
        hooks: false,
        server_tools: false,
        subagents: false,
        context_compression: false,
        passive_capture: false,
        supports_tool_policy: false,
        history_store: false,
        history_mutations: false,
        terminal_event: false,
    };

    pub const fn with_bidirectional(mut self) -> Self {
        self.bidirectional = true;
        self
    }
    pub const fn with_structured_tools(mut self) -> Self {
        self.structured_tools = true;
        self
    }
    pub const fn with_reasoning(mut self) -> Self {
        self.reasoning = true;
        self
    }
    pub const fn with_file_changes(mut self) -> Self {
        self.file_changes = true;
        self
    }
    pub const fn with_interruptible(mut self) -> Self {
        self.interruptible = true;
        self
    }
    pub const fn with_resumable_sessions(mut self) -> Self {
        self.resumable_sessions = true;
        self
    }
    pub const fn with_mcp_routing(mut self) -> Self {
        self.mcp_routing = true;
        self
    }
    pub const fn with_hooks(mut self) -> Self {
        self.hooks = true;
        self
    }
    pub const fn with_server_tools(mut self) -> Self {
        self.server_tools = true;
        self
    }
    pub const fn with_subagents(mut self) -> Self {
        self.subagents = true;
        self
    }
    pub const fn with_context_compression(mut self) -> Self {
        self.context_compression = true;
        self
    }
    /// Enable passive on-disk capture (spec 010). Flipped per-Backend only
    /// when the matching `aikit-session-capture` feature is on.
    pub const fn with_passive_capture(mut self) -> Self {
        self.passive_capture = true;
        self
    }
    /// Declare that this Backend enforces `AgentPersona.tools` / `disallowed_tools` (D2).
    pub const fn with_supports_tool_policy(mut self) -> Self {
        self.supports_tool_policy = true;
        self
    }
    /// Declare that this Backend persists browsable session transcripts
    /// readable via `HistoryReader` (spec 008 §4).
    pub const fn with_history_store(mut self) -> Self {
        self.history_store = true;
        self
    }
    /// Declare that this Backend's history store supports metadata mutations
    /// (rename/tag) via `HistoryMutator` (spec 008 §4).
    pub const fn with_history_mutations(mut self) -> Self {
        self.history_mutations = true;
        self
    }
    /// Declare that this Backend's decoder emits a status-bearing terminal
    /// frame. Only set it where a decoder actually does.
    pub const fn with_terminal_event(mut self) -> Self {
        self.terminal_event = true;
        self
    }
}

impl Default for BackendCapabilities {
    fn default() -> Self {
        Self::NONE
    }
}
