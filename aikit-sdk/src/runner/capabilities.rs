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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
}

impl Default for BackendCapabilities {
    fn default() -> Self {
        Self::NONE
    }
}
