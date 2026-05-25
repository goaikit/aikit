//! Agent runner builder and agent detection utilities.

use crate::pipeline::PipelineError;
use crate::runner::{
    get_agent_status, run_agent_events, runnable_agents, AgentEvent, AgentEventPayload,
    AgentStatus, MessagePhase, MessageRole, RunOptions,
};
use std::path::PathBuf;

#[cfg(test)]
type MockQueue = std::sync::Arc<
    std::sync::Mutex<std::collections::VecDeque<Result<String, crate::pipeline::PipelineError>>>,
>;
#[cfg(test)]
type CapturedPrompts = std::sync::Arc<std::sync::Mutex<Vec<String>>>;

// ---------------------------------------------------------------------------
// AgentRunner
// ---------------------------------------------------------------------------

/// Builder for running an agent with a specific configuration.
pub struct AgentRunner {
    agent_key: String,
    model: Option<String>,
    working_dir: Option<PathBuf>,
    #[cfg(test)]
    mock_responses: Option<MockQueue>,
    #[cfg(test)]
    captured_prompts: Option<CapturedPrompts>,
}

impl AgentRunner {
    /// Create a new `AgentRunner` with no agent key set.
    pub fn new() -> Self {
        Self {
            agent_key: String::new(),
            model: None,
            working_dir: None,
            #[cfg(test)]
            mock_responses: None,
            #[cfg(test)]
            captured_prompts: None,
        }
    }

    /// Create a mock `AgentRunner` for testing.
    ///
    /// Returns the runner and a shared prompt capture buffer.
    /// Each call to `run()` pops the next response from `responses`.
    #[cfg(test)]
    pub(crate) fn with_mock(
        responses: Vec<Result<String, crate::pipeline::PipelineError>>,
    ) -> (Self, CapturedPrompts) {
        use std::collections::VecDeque;
        use std::sync::{Arc, Mutex};
        let captured: CapturedPrompts = Arc::new(Mutex::new(Vec::new()));
        let runner = Self {
            agent_key: String::new(),
            model: None,
            working_dir: None,
            mock_responses: Some(Arc::new(Mutex::new(VecDeque::from(responses)))),
            captured_prompts: Some(captured.clone()),
        };
        (runner, captured)
    }

    /// Set the agent key.
    pub fn agent(mut self, key: &str) -> Self {
        self.agent_key = key.to_string();
        self
    }

    /// Set the model to use for this agent invocation.
    pub fn model(mut self, model: &str) -> Self {
        self.model = Some(model.to_string());
        self
    }

    /// Set the working directory for the agent child process.
    pub fn working_dir(mut self, path: &str) -> Self {
        self.working_dir = Some(PathBuf::from(path));
        self
    }

    /// Invoke the agent with `prompt`; assemble assistant text from the event stream.
    ///
    /// Blocking. Returns `PipelineError::AgentInvocation` on any RunError.
    pub fn run(&self, prompt: &str) -> Result<String, PipelineError> {
        // In tests: use mock response queue if populated.
        #[cfg(test)]
        if let Some(ref responses) = self.mock_responses {
            let mut queue = responses.lock().unwrap();
            if !queue.is_empty() {
                if let Some(ref captured) = self.captured_prompts {
                    captured.lock().unwrap().push(prompt.to_string());
                }
                return queue.pop_front().unwrap();
            }
        }

        let mut options = RunOptions::default();
        if let Some(ref model) = self.model {
            options.model = Some(model.clone());
        }
        if let Some(ref dir) = self.working_dir {
            options.current_dir = Some(dir.clone());
        }

        let mut events: Vec<AgentEvent> = Vec::new();
        run_agent_events(&self.agent_key, prompt, options, |ev| {
            events.push(ev);
        })
        .map_err(|source| PipelineError::AgentInvocation { source })?;

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

impl Default for AgentRunner {
    fn default() -> Self {
        Self::new()
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
                let status = status_map.get(key).cloned().unwrap_or(AgentStatus {
                    available: false,
                    reason: None,
                });

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
            "agent" => {
                // "agent" maps to catalog key "cursor-agent"
                crate::agent("cursor-agent")
                    .map(|c| c.name.to_string())
                    .unwrap_or_else(|| "Cursor".to_string())
            }
            "aikit" => "aikit".to_string(),
            other => {
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

    /// AC8: Verify that AgentRunner::run() maps RunError to PipelineError::AgentInvocation.
    ///
    /// An unknown agent key causes run_agent_events to return RunError::AgentNotRunnable,
    /// which must be mapped to PipelineError::AgentInvocation by the .map_err() call.
    #[test]
    fn test_agent_runner_run_maps_run_error_to_agent_invocation() {
        let runner = AgentRunner::new().agent("_not_a_real_agent_key_for_testing_xyz_");
        let result = runner.run("test prompt");
        assert!(
            matches!(result, Err(PipelineError::AgentInvocation { .. })),
            "expected AgentInvocation, got {:?}",
            result
        );
    }

    /// Integration test: verify AgentDetector::detect() works on a live system.
    /// Requires at least one agent binary to be installed.
    #[test]
    #[ignore]
    fn integration_test_agent_detector_detect_on_live_system() {
        let infos = AgentDetector::detect();
        assert!(!infos.is_empty(), "expected at least one agent entry");
        let has_installed = infos.iter().any(|i| i.installed);
        assert!(has_installed, "expected at least one installed agent");
    }
}
