use super::executor::{ChatEvent, ExecError, ToolChat, ToolExecutor};
use super::tool::ToolDef;

pub struct MockExecutor {
    pub response: serde_json::Value,
    pub error: Option<ExecError>,
}

impl MockExecutor {
    pub fn ok(response: serde_json::Value) -> Self {
        Self {
            response,
            error: None,
        }
    }

    pub fn err(error: ExecError) -> Self {
        Self {
            response: serde_json::Value::Null,
            error: Some(error),
        }
    }
}

impl ToolExecutor for MockExecutor {
    fn execute(
        &self,
        _tool: &ToolDef,
        _input: &serde_json::Value,
    ) -> Result<serde_json::Value, ExecError> {
        match &self.error {
            Some(ExecError::AgentFailed(m)) => Err(ExecError::AgentFailed(m.clone())),
            Some(ExecError::OutputInvalid(m)) => Err(ExecError::OutputInvalid(m.clone())),
            Some(ExecError::Internal(m)) => Err(ExecError::Internal(m.clone())),
            None => Ok(self.response.clone()),
        }
    }
}

pub struct MockChat {
    pub events: Vec<ChatEvent>,
    pub draft: serde_json::Value,
    pub error: Option<ExecError>,
}

impl MockChat {
    pub fn new(events: Vec<ChatEvent>, draft: serde_json::Value) -> Self {
        Self {
            events,
            draft,
            error: None,
        }
    }

    pub fn with_error(error: ExecError) -> Self {
        Self {
            events: vec![],
            draft: serde_json::Value::Null,
            error: Some(error),
        }
    }
}

impl ToolChat for MockChat {
    fn run_turn(
        &self,
        _tool: &ToolDef,
        _session_id: Option<&str>,
        _msg: &str,
        sink: &mut dyn FnMut(ChatEvent),
    ) -> Result<(), ExecError> {
        if let Some(ref e) = self.error {
            return Err(match e {
                ExecError::AgentFailed(m) => ExecError::AgentFailed(m.clone()),
                ExecError::OutputInvalid(m) => ExecError::OutputInvalid(m.clone()),
                ExecError::Internal(m) => ExecError::Internal(m.clone()),
            });
        }
        for event in &self.events {
            sink(event.clone());
        }
        Ok(())
    }

    fn finalize(&self, _tool: &ToolDef, _session_id: &str) -> Result<serde_json::Value, ExecError> {
        if let Some(ref e) = self.error {
            return Err(match e {
                ExecError::AgentFailed(m) => ExecError::AgentFailed(m.clone()),
                ExecError::OutputInvalid(m) => ExecError::OutputInvalid(m.clone()),
                ExecError::Internal(m) => ExecError::Internal(m.clone()),
            });
        }
        Ok(self.draft.clone())
    }
}
