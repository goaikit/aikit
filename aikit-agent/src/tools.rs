use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use crate::llm::types::{FunctionDefinition, ToolDefinition};
use crate::skills::DiscoveredSkill;

#[derive(Debug, Clone)]
pub struct ToolOutput {
    pub content: String,
    pub is_error: bool,
}

impl ToolOutput {
    pub fn ok(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: false,
        }
    }

    pub fn err(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: true,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("{0}")]
    Exec(String),
}

#[derive(Clone)]
pub struct ToolContext {
    pub workdir: PathBuf,
    pub allowed_roots: Vec<PathBuf>,
    pub timeout: Duration,
    pub max_output_bytes: usize,
}

impl ToolContext {
    pub fn new(workdir: PathBuf, allowed_roots: Vec<PathBuf>) -> Self {
        Self {
            workdir,
            allowed_roots,
            timeout: Duration::from_secs(30),
            max_output_bytes: 1024 * 1024,
        }
    }
}

pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn schema(&self) -> ToolDefinition;
    fn execute(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError>;
}

/// Check if `path` is within any of the allowed roots.
pub fn is_path_allowed(path: &Path, allowed_roots: &[PathBuf]) -> bool {
    let canonical = canonicalize_existing_prefix(path);
    allowed_roots.iter().any(|root| {
        root.canonicalize()
            .map(|r| canonical.starts_with(&r))
            .unwrap_or(false)
    })
}

fn canonicalize_existing_prefix(path: &Path) -> PathBuf {
    let mut missing = Vec::new();
    let mut cursor = path;

    while !cursor.exists() {
        if let Some(name) = cursor.file_name() {
            missing.push(name.to_os_string());
        }
        match cursor.parent() {
            Some(parent) if parent != cursor => cursor = parent,
            _ => break,
        }
    }

    let mut canonical = cursor
        .canonicalize()
        .unwrap_or_else(|_| cursor.to_path_buf());
    for component in missing.iter().rev() {
        canonical.push(component);
    }
    canonical
}

fn resolve_path(path_str: &str, workdir: &Path) -> PathBuf {
    let p = PathBuf::from(path_str);
    if p.is_absolute() {
        p
    } else {
        workdir.join(p)
    }
}

// ── read_file ────────────────────────────────────────────────────────────────

pub struct ReadFileTool;

impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn schema(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: "read_file".to_string(),
                description: Some("Read the contents of a file".to_string()),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Absolute or relative file path"
                        }
                    },
                    "required": ["path"]
                }),
            },
        }
    }

    fn execute(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| ToolError::Exec("missing 'path' parameter".to_string()))?;

        let path = resolve_path(path_str, &ctx.workdir);

        if !is_path_allowed(&path, &ctx.allowed_roots) {
            return Ok(ToolOutput::err(format!(
                "E_AIKIT_TOOL_EXEC_FAILED: path '{}' is outside allowed roots",
                path.display()
            )));
        }

        match std::fs::read_to_string(&path) {
            Ok(content) => {
                if content.len() > ctx.max_output_bytes {
                    Ok(ToolOutput::ok(content[..ctx.max_output_bytes].to_string()))
                } else {
                    Ok(ToolOutput::ok(content))
                }
            }
            Err(e) => Ok(ToolOutput::err(format!("failed to read file: {}", e))),
        }
    }
}

// ── write_file ───────────────────────────────────────────────────────────────

pub struct WriteFileTool;

impl Tool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn schema(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: "write_file".to_string(),
                description: Some(
                    "Write content to a file, creating parent directories if needed".to_string(),
                ),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "content": { "type": "string" }
                    },
                    "required": ["path", "content"]
                }),
            },
        }
    }

    fn execute(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| ToolError::Exec("missing 'path' parameter".to_string()))?;
        let content = input["content"]
            .as_str()
            .ok_or_else(|| ToolError::Exec("missing 'content' parameter".to_string()))?;

        let path = resolve_path(path_str, &ctx.workdir);

        if !is_path_allowed(&path, &ctx.allowed_roots) {
            return Ok(ToolOutput::err(format!(
                "E_AIKIT_TOOL_EXEC_FAILED: path '{}' is outside allowed roots",
                path.display()
            )));
        }

        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return Ok(ToolOutput::err(format!(
                    "failed to create parent dirs: {}",
                    e
                )));
            }
        }

        match std::fs::write(&path, content) {
            Ok(_) => Ok(ToolOutput::ok(format!(
                "wrote {} bytes to {}",
                content.len(),
                path.display()
            ))),
            Err(e) => Ok(ToolOutput::err(format!("failed to write file: {}", e))),
        }
    }
}

// ── run_bash ─────────────────────────────────────────────────────────────────

pub struct RunBashTool;

impl Tool for RunBashTool {
    fn name(&self) -> &str {
        "run_bash"
    }

    fn schema(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: "run_bash".to_string(),
                description: Some("Execute a shell command".to_string()),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": { "type": "string" },
                        "timeout_secs": { "type": "integer" }
                    },
                    "required": ["command"]
                }),
            },
        }
    }

    fn execute(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let command = input["command"]
            .as_str()
            .ok_or_else(|| ToolError::Exec("missing 'command' parameter".to_string()))?;

        let timeout_secs = input["timeout_secs"]
            .as_u64()
            .unwrap_or(ctx.timeout.as_secs());
        let timeout = Duration::from_secs(timeout_secs);

        let output = run_command_with_timeout(command, &ctx.workdir, timeout, ctx.max_output_bytes);
        Ok(output)
    }
}

fn run_command_with_timeout(
    command: &str,
    workdir: &Path,
    timeout: Duration,
    max_bytes: usize,
) -> ToolOutput {
    use std::process::{Command, Stdio};
    use std::thread;

    let workdir = workdir.to_path_buf();
    let command = command.to_string();

    let (tx, rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let result = Command::new("sh")
            .arg("-c")
            .arg(&command)
            .current_dir(&workdir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output();
        let _ = tx.send(result);
    });

    match rx.recv_timeout(timeout) {
        Ok(Ok(output)) => {
            let mut combined = output.stdout;
            combined.extend_from_slice(&output.stderr);
            if combined.len() > max_bytes {
                combined.truncate(max_bytes);
            }
            let text = String::from_utf8_lossy(&combined).to_string();
            if output.status.success() {
                ToolOutput::ok(text)
            } else {
                ToolOutput::err(format!(
                    "exit code {}: {}",
                    output.status.code().unwrap_or(-1),
                    text
                ))
            }
        }
        Ok(Err(e)) => ToolOutput::err(format!("failed to spawn command: {}", e)),
        Err(_) => ToolOutput::err(format!(
            "E_AIKIT_TOOL_EXEC_FAILED: command timed out after {}s",
            timeout.as_secs()
        )),
    }
}

// ── git ──────────────────────────────────────────────────────────────────────

pub struct GitTool;

impl Tool for GitTool {
    fn name(&self) -> &str {
        "git"
    }

    fn schema(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: "git".to_string(),
                description: Some("Execute git commands".to_string()),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "args": {
                            "type": "string",
                            "description": "Git subcommand and arguments"
                        }
                    },
                    "required": ["args"]
                }),
            },
        }
    }

    fn execute(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let args = input["args"]
            .as_str()
            .ok_or_else(|| ToolError::Exec("missing 'args' parameter".to_string()))?;

        let command = format!("git {}", args);
        Ok(run_command_with_timeout(
            &command,
            &ctx.workdir,
            ctx.timeout,
            ctx.max_output_bytes,
        ))
    }
}

// ── read_skill ───────────────────────────────────────────────────────────────

pub struct ReadSkillTool {
    pub skills: Vec<DiscoveredSkill>,
}

impl Tool for ReadSkillTool {
    fn name(&self) -> &str {
        "read_skill"
    }

    fn schema(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: "read_skill".to_string(),
                description: Some("Read the full content of a skill by name".to_string()),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "skill_name": { "type": "string" }
                    },
                    "required": ["skill_name"]
                }),
            },
        }
    }

    fn execute(&self, input: Value, _ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let skill_name = input["skill_name"]
            .as_str()
            .ok_or_else(|| ToolError::Exec("missing 'skill_name' parameter".to_string()))?;

        let skill = self.skills.iter().find(|s| s.metadata.name == skill_name);
        match skill {
            Some(s) => match s.load_content() {
                Ok(content) => Ok(ToolOutput::ok(content)),
                Err(e) => Ok(ToolOutput::err(format!("failed to load skill: {}", e))),
            },
            None => Ok(ToolOutput::err(format!("skill '{}' not found", skill_name))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn make_ctx(tmp: &TempDir) -> ToolContext {
        ToolContext::new(tmp.path().to_path_buf(), vec![tmp.path().to_path_buf()])
    }

    #[test]
    fn test_read_file_rejects_outside_allowed_roots() {
        let tmp = TempDir::new().unwrap();
        let ctx = make_ctx(&tmp);
        let tool = ReadFileTool;

        let input = serde_json::json!({"path": "/etc/passwd"});
        let result = tool.execute(input, &ctx).unwrap();
        assert!(result.is_error, "should be an error");
        assert!(result.content.contains("outside allowed roots"));
    }

    #[test]
    fn test_read_file_reads_within_roots() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("test.txt");
        fs::write(&file, "hello file").unwrap();

        let ctx = make_ctx(&tmp);
        let tool = ReadFileTool;
        let input = serde_json::json!({"path": file.to_str().unwrap()});
        let result = tool.execute(input, &ctx).unwrap();
        assert!(!result.is_error);
        assert_eq!(result.content, "hello file");
    }

    #[test]
    fn test_write_file_creates_parent_dirs() {
        let tmp = TempDir::new().unwrap();
        let ctx = make_ctx(&tmp);
        let tool = WriteFileTool;

        let path = tmp.path().join("subdir").join("nested").join("file.txt");
        let input = serde_json::json!({
            "path": path.to_str().unwrap(),
            "content": "nested content"
        });
        let result = tool.execute(input, &ctx).unwrap();
        assert!(!result.is_error, "should succeed: {}", result.content);
        assert_eq!(fs::read_to_string(&path).unwrap(), "nested content");
    }

    #[test]
    fn test_write_file_rejects_outside_allowed_roots() {
        let tmp = TempDir::new().unwrap();
        let ctx = make_ctx(&tmp);
        let tool = WriteFileTool;

        let input = serde_json::json!({
            "path": "/tmp/outside-allowed-roots-test-file",
            "content": "evil"
        });
        let result = tool.execute(input, &ctx).unwrap();
        assert!(result.is_error);
        assert!(result.content.contains("outside allowed roots"));
    }

    #[test]
    fn test_run_bash_timeout() {
        let tmp = TempDir::new().unwrap();
        let mut ctx = make_ctx(&tmp);
        ctx.timeout = Duration::from_millis(100); // very short timeout
        let tool = RunBashTool;

        let input = serde_json::json!({"command": "sleep 10", "timeout_secs": 0});
        // Override with a 0-second timeout via explicit param - but let's use default
        let input2 = serde_json::json!({"command": "sleep 5"});
        let result = tool.execute(input2, &ctx).unwrap();
        assert!(result.is_error, "should time out");
        assert!(
            result.content.contains("timed out"),
            "should mention timeout: {}",
            result.content
        );
        drop(input); // suppress unused warning
    }

    #[test]
    fn test_run_bash_success() {
        let tmp = TempDir::new().unwrap();
        let ctx = make_ctx(&tmp);
        let tool = RunBashTool;

        let input = serde_json::json!({"command": "echo hello"});
        let result = tool.execute(input, &ctx).unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("hello"));
    }
}
