//! Agent runner builder and agent detection utilities.

use crate::runner::{
    get_agent_status, run_agent_events, runnable_agents, AgentEvent, AgentEventPayload,
    MessagePhase, MessageRole, RunError, RunOptions,
};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// AgentRunner
// ---------------------------------------------------------------------------

/// Builder for running an agent with a specific configuration.
pub struct AgentRunner {
    agent_key: String,
    model: Option<String>,
    working_dir: Option<PathBuf>,
}

impl AgentRunner {
    /// Create a new `AgentRunner` with the given agent key.
    pub fn new(agent_key: impl Into<String>) -> Self {
        Self {
            agent_key: agent_key.into(),
            model: None,
            working_dir: None,
        }
    }

    /// Alias for `new`.
    pub fn agent(key: impl Into<String>) -> Self {
        Self::new(key)
    }

    /// Set the model to use for this agent invocation.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Set the working directory for the agent child process.
    pub fn with_working_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.working_dir = Some(path.into());
        self
    }

    /// Decompose the runner into its configuration parts.
    ///
    /// Returns `(agent_key, model, working_dir)`. Used by `Pipeline::run` to
    /// reconstruct the runner on each retry attempt.
    pub fn into_parts(self) -> (String, Option<String>, Option<PathBuf>) {
        (self.agent_key, self.model, self.working_dir)
    }

    /// Run the agent with the given prompt, returning the concatenated final
    /// assistant text, or a `RunError` on failure.
    ///
    /// Collects all events, filters to `AgentEventPayload::StreamMessage(msg)`
    /// where `msg.role == MessageRole::Assistant` and `msg.phase == MessagePhase::Final`,
    /// sorts by `seq`, and concatenates `msg.text`.
    pub fn run_prompt(self, prompt: String) -> Result<String, RunError> {
        let mut options = RunOptions::default();
        if let Some(model) = self.model {
            options.model = Some(model);
        }
        if let Some(dir) = self.working_dir {
            options.current_dir = Some(dir);
        }

        let mut events: Vec<AgentEvent> = Vec::new();
        run_agent_events(&self.agent_key, &prompt, options, |ev| {
            events.push(ev);
        })?;

        // Filter: StreamMessage where role=Assistant and phase=Final
        let mut final_messages: Vec<(u64, String)> = events
            .into_iter()
            .filter_map(|ev| {
                if let AgentEventPayload::StreamMessage(msg) = ev.payload {
                    if msg.role == MessageRole::Assistant && msg.phase == MessagePhase::Final {
                        return Some((ev.seq, msg.text));
                    }
                }
                None
            })
            .collect();

        // Sort by seq
        final_messages.sort_by_key(|(seq, _)| *seq);

        // Concatenate
        let text: String = final_messages.into_iter().map(|(_, t)| t).collect();
        Ok(text)
    }
}

// ---------------------------------------------------------------------------
// AgentInfo
// ---------------------------------------------------------------------------

/// Information about a single runnable agent.
#[derive(Debug, Clone)]
pub struct AgentInfo {
    /// The runnable agent key (e.g. `"claude"`, `"agent"`).
    pub key: String,
    /// Human-readable name.
    pub name: String,
    /// Whether the agent binary is available and ready to use.
    pub installed: bool,
    /// Reason the agent is not available (if `installed` is `false`).
    pub reason: Option<String>,
}

// ---------------------------------------------------------------------------
// AgentDetector
// ---------------------------------------------------------------------------

/// Detects which agents are installed and available on the system.
pub struct AgentDetector;

impl AgentDetector {
    /// Probe all runnable agent keys and return their availability status.
    ///
    /// Key name mapping:
    /// - `"agent"` → catalog key `"cursor-agent"` → name `"Cursor"`
    /// - `"aikit"` → name `"aikit"` (fallback, not in catalog under that key)
    /// - all others → `crate::agent(key).name`
    pub fn detect() -> Vec<AgentInfo> {
        let status_map = get_agent_status();

        runnable_agents()
            .iter()
            .map(|&key| {
                let name = Self::resolve_name(key);
                let status = status_map
                    .get(key)
                    .cloned()
                    .unwrap_or_else(crate::runner::AgentStatus::available);

                AgentInfo {
                    key: key.to_string(),
                    name,
                    installed: status.available,
                    reason: status.reason.map(|r| format!("{:?}", r)),
                }
            })
            .collect()
    }

    /// Resolve the human-readable name for a runnable key.
    fn resolve_name(key: &str) -> String {
        match key {
            "agent" => "Cursor".to_string(),
            "aikit" => "aikit".to_string(),
            other => {
                // Look up in catalog
                if let Some(config) = crate::agent(other) {
                    config.name.to_string()
                } else {
                    other.to_string()
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::{
        AgentEvent, AgentEventPayload, AgentEventStream, MessageKind, StreamMessage,
    };

    fn make_stream_message_event(
        seq: u64,
        role: MessageRole,
        phase: MessagePhase,
        text: &str,
    ) -> AgentEvent {
        AgentEvent {
            agent_key: "claude".to_string(),
            seq,
            stream: AgentEventStream::Stdout,
            payload: AgentEventPayload::StreamMessage(StreamMessage {
                text: text.to_string(),
                phase,
                role,
                kind: MessageKind::Message,
                source: AgentEventStream::Stdout,
                raw_line_seq: seq,
                turn_id: None,
            }),
        }
    }

    #[test]
    fn test_filter_and_sort_final_assistant_messages() {
        // Simulate collecting events from run_agent_events
        let events: Vec<AgentEvent> = vec![
            // Delta (should be ignored)
            make_stream_message_event(1, MessageRole::Assistant, MessagePhase::Delta, "partial"),
            // User final (should be ignored)
            make_stream_message_event(2, MessageRole::User, MessagePhase::Final, "user msg"),
            // Assistant Final (should be included)
            make_stream_message_event(3, MessageRole::Assistant, MessagePhase::Final, "Hello "),
            // Tool final (should be ignored)
            make_stream_message_event(4, MessageRole::Tool, MessagePhase::Final, "tool output"),
            // Another Assistant Final out of order (should be sorted after seq=3)
            make_stream_message_event(5, MessageRole::Assistant, MessagePhase::Final, "world"),
        ];

        // Test the filtering + sorting + concatenation logic directly
        let mut final_messages: Vec<(u64, String)> = events
            .into_iter()
            .filter_map(|ev| {
                if let AgentEventPayload::StreamMessage(msg) = ev.payload {
                    if msg.role == MessageRole::Assistant && msg.phase == MessagePhase::Final {
                        return Some((ev.seq, msg.text));
                    }
                }
                None
            })
            .collect();

        final_messages.sort_by_key(|(seq, _)| *seq);
        let text: String = final_messages.into_iter().map(|(_, t)| t).collect();
        assert_eq!(text, "Hello world");
    }

    #[test]
    fn test_filter_no_final_messages_returns_empty() {
        let events: Vec<AgentEvent> = vec![
            make_stream_message_event(1, MessageRole::Assistant, MessagePhase::Delta, "delta"),
            make_stream_message_event(2, MessageRole::User, MessagePhase::Final, "user"),
        ];

        let final_messages: Vec<(u64, String)> = events
            .into_iter()
            .filter_map(|ev| {
                if let AgentEventPayload::StreamMessage(msg) = ev.payload {
                    if msg.role == MessageRole::Assistant && msg.phase == MessagePhase::Final {
                        return Some((ev.seq, msg.text));
                    }
                }
                None
            })
            .collect();

        let text: String = final_messages.into_iter().map(|(_, t)| t).collect();
        assert_eq!(text, "");
    }

    #[test]
    fn test_agent_detector_returns_all_runnable_keys() {
        // AgentDetector::detect calls real system probes, but we can at minimum
        // verify the returned keys match runnable_agents()
        let infos = AgentDetector::detect();
        let returned_keys: std::collections::HashSet<String> =
            infos.iter().map(|i| i.key.clone()).collect();
        for &key in runnable_agents() {
            assert!(
                returned_keys.contains(key),
                "Missing key '{}' in AgentDetector::detect()",
                key
            );
        }
        assert_eq!(infos.len(), runnable_agents().len());
    }

    #[test]
    fn test_agent_detector_name_mapping_agent_key() {
        let infos = AgentDetector::detect();
        let agent_info = infos.iter().find(|i| i.key == "agent").unwrap();
        assert_eq!(agent_info.name, "Cursor");
    }

    #[test]
    fn test_agent_detector_name_mapping_aikit_key() {
        let infos = AgentDetector::detect();
        let aikit_info = infos.iter().find(|i| i.key == "aikit").unwrap();
        assert_eq!(aikit_info.name, "aikit");
    }

    #[test]
    fn test_run_error_not_runnable_maps_to_pipeline_error() {
        let run_err = RunError::AgentNotRunnable("fake".to_string());
        let pipeline_err = crate::pipeline::PipelineError::AgentInvocation { source: run_err };
        assert!(pipeline_err.to_string().contains("agent invocation failed"));
    }
}
