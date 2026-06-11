use std::ffi::OsString;

pub fn runnable_agents() -> &'static [&'static str] {
    &["codex", "claude", "gemini", "opencode", "agent", "aikit"]
}

pub fn is_runnable(agent_key: &str) -> bool {
    runnable_agents().contains(&agent_key)
}

#[derive(Clone, Copy)]
enum SessionMode {
    Positional,
    Flag(&'static str),
}

struct AgentCliSpec {
    key: &'static str,
    binary: &'static str,
    model_flag: &'static str,
    yolo_flag: Option<&'static str>,
    session_mode: SessionMode,
}

static SPECS: &[AgentCliSpec] = &[
    AgentCliSpec {
        key: "codex",
        binary: "codex",
        model_flag: "-m",
        yolo_flag: Some("--yolo"),
        session_mode: SessionMode::Positional,
    },
    AgentCliSpec {
        key: "claude",
        binary: "claude",
        model_flag: "--model",
        yolo_flag: None,
        session_mode: SessionMode::Flag("--resume"),
    },
    AgentCliSpec {
        key: "gemini",
        binary: "gemini",
        model_flag: "--model",
        yolo_flag: None,
        session_mode: SessionMode::Flag("--resume"),
    },
    AgentCliSpec {
        key: "opencode",
        binary: "opencode",
        model_flag: "-m",
        yolo_flag: Some("--yolo"),
        session_mode: SessionMode::Flag("--session"),
    },
    AgentCliSpec {
        key: "agent",
        binary: "agent",
        model_flag: "--model",
        yolo_flag: Some("--force"),
        session_mode: SessionMode::Flag("--resume"),
    },
];

fn get_agent_spec(key: &str) -> Option<&'static AgentCliSpec> {
    SPECS.iter().find(|s| s.key == key)
}

impl AgentCliSpec {
    fn push_model(&self, argv: &mut Vec<OsString>, model: Option<&String>) {
        if let Some(m) = model {
            // An empty/whitespace model string means "unset" — passing the flag with
            // an empty value makes engines fail (e.g. codex: 400 "The '' model is not
            // supported"). Fall back to the engine's own default model instead.
            if !m.trim().is_empty() {
                argv.push(OsString::from(self.model_flag));
                argv.push(OsString::from(m.as_str()));
            }
        }
    }

    fn push_yolo(&self, argv: &mut Vec<OsString>, yolo: bool) {
        if let Some(flag) = self.yolo_flag {
            if yolo {
                argv.push(OsString::from(flag));
            }
        }
    }

    fn push_session_flag(&self, argv: &mut Vec<OsString>, session_id: Option<&str>) {
        if let SessionMode::Flag(flag) = self.session_mode {
            if let Some(id) = session_id {
                argv.push(OsString::from(flag));
                argv.push(OsString::from(id));
            }
        }
    }
}

pub(super) fn build_argv(
    agent_key: &str,
    model: Option<&String>,
    yolo: bool,
    stream: bool,
    events_mode: bool,
    session_id: Option<&str>,
) -> Vec<OsString> {
    let spec = get_agent_spec(agent_key).expect("unknown agent key for build_argv");

    match spec.key {
        "codex" => {
            let mut argv = match session_id {
                Some(id) => vec![
                    OsString::from(spec.binary),
                    OsString::from("resume"),
                    OsString::from(id),
                ],
                None => vec![OsString::from(spec.binary), OsString::from("exec")],
            };
            spec.push_model(&mut argv, model);
            spec.push_yolo(&mut argv, yolo);
            argv.extend_from_slice(&[
                OsString::from("--json"),
                OsString::from("--"),
                OsString::from("-"),
            ]);
            argv
        }
        "claude" => {
            let mut argv = vec![
                OsString::from(spec.binary),
                OsString::from("-p"),
                OsString::from("-"),
                OsString::from("--dangerously-skip-permissions"),
            ];
            spec.push_model(&mut argv, model);
            let fmt = if events_mode {
                if stream {
                    "stream-json"
                } else {
                    "json"
                }
            } else if stream {
                "stream-json"
            } else {
                "text"
            };
            argv.extend_from_slice(&[OsString::from("--output-format"), OsString::from(fmt)]);
            if events_mode && stream {
                argv.push(OsString::from("--verbose"));
            }
            spec.push_session_flag(&mut argv, session_id);
            argv
        }
        "gemini" => {
            let mut argv = vec![
                OsString::from(spec.binary),
                OsString::from("--prompt"),
                OsString::from("-"),
            ];
            if events_mode {
                argv.extend_from_slice(&[
                    OsString::from("--output-format"),
                    OsString::from("stream-json"),
                    OsString::from("--yolo"),
                ]);
            }
            spec.push_model(&mut argv, model);
            spec.push_session_flag(&mut argv, session_id);
            argv
        }
        "opencode" => {
            let mut argv = vec![OsString::from(spec.binary)];
            spec.push_model(&mut argv, model);
            argv.push(OsString::from("run"));
            // `--yolo` (auto-approve tool use) must be passed in BOTH plain and
            // events mode. Without it opencode auto-rejects its own write/edit tool
            // calls, so an agent asked to modify files silently produces nothing.
            spec.push_yolo(&mut argv, yolo);
            if events_mode {
                argv.extend_from_slice(&[OsString::from("--format"), OsString::from("json")]);
            }
            spec.push_session_flag(&mut argv, session_id);
            argv
        }
        "agent" => {
            let mut argv = vec![OsString::from(spec.binary), OsString::from("--print")];
            if events_mode {
                let fmt = if stream { "stream-json" } else { "json" };
                argv.extend_from_slice(&[OsString::from("--output-format"), OsString::from(fmt)]);
            } else if stream {
                argv.extend_from_slice(&[
                    OsString::from("--output-format"),
                    OsString::from("json"),
                ]);
            }
            spec.push_model(&mut argv, model);
            spec.push_yolo(&mut argv, yolo);
            spec.push_session_flag(&mut argv, session_id);
            argv
        }
        _ => unreachable!(),
    }
}

pub(super) fn should_write_stdin(_agent_key: &str) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_runnable_true_for_supported_false_for_others() {
        assert!(is_runnable("codex"));
        assert!(is_runnable("claude"));
        assert!(is_runnable("gemini"));
        assert!(is_runnable("opencode"));
        assert!(is_runnable("agent"));
        assert!(!is_runnable("copilot"));
        assert!(!is_runnable("cursor-agent"));
        assert!(!is_runnable("unknown"));
    }

    #[test]
    fn test_is_runnable_case_sensitive() {
        assert!(is_runnable("codex"));
        assert!(!is_runnable("Codex"));
        assert!(!is_runnable("CODEX"));
    }

    #[test]
    fn test_runnable_agents_includes_codex_claude_gemini_opencode_agent() {
        let agents = runnable_agents();
        assert!(agents.contains(&"codex"));
        assert!(agents.contains(&"claude"));
        assert!(agents.contains(&"gemini"));
        assert!(agents.contains(&"opencode"));
        assert!(agents.contains(&"agent"));
        assert!(agents.contains(&"aikit"));
        assert_eq!(agents.len(), 6);
    }

    // --- codex ---

    #[test]
    fn test_codex_plain_contains_exec_and_model() {
        let argv = build_argv(
            "codex",
            Some(&"gpt-4".to_string()),
            true,
            false,
            false,
            None,
        );
        assert!(argv.contains(&OsString::from("codex")));
        assert!(argv.contains(&OsString::from("exec")));
        assert!(argv.contains(&OsString::from("-m")));
        assert!(argv.contains(&OsString::from("gpt-4")));
        assert!(argv.contains(&OsString::from("--yolo")));
        assert!(argv.contains(&OsString::from("--json")));
    }

    #[test]
    fn test_codex_plain_no_model() {
        let argv = build_argv("codex", None, false, false, false, None);
        assert!(!argv.contains(&OsString::from("-m")));
        assert!(!argv.contains(&OsString::from("--yolo")));
        assert!(argv.contains(&OsString::from("--json")));
    }

    #[test]
    fn test_empty_model_is_not_passed() {
        // An empty model string must NOT produce `-m ""` (engines reject it).
        let empty = "".to_string();
        let argv = build_argv("codex", Some(&empty), true, false, false, None);
        assert!(
            !argv.contains(&OsString::from("-m")),
            "empty model must be skipped; got {:?}",
            argv
        );
        let ws = "   ".to_string();
        let argv2 = build_argv("opencode", Some(&ws), true, false, true, None);
        assert!(
            !argv2.contains(&OsString::from("-m")),
            "whitespace model must be skipped; got {:?}",
            argv2
        );
    }

    #[test]
    fn test_opencode_events_mode_passes_yolo() {
        // Regression: opencode must get --yolo in events mode too, else its write
        // tool is auto-rejected and file-modifying agents silently produce nothing.
        let argv = build_argv("opencode", None, true, false, true, None);
        assert!(
            argv.contains(&OsString::from("--yolo")),
            "opencode events mode must include --yolo; got {:?}",
            argv
        );
        assert!(argv.contains(&OsString::from("--format")));
    }

    #[test]
    fn test_codex_plain_with_session_id() {
        let argv = build_argv("codex", None, false, false, false, Some("test-session-id"));
        assert!(argv.contains(&OsString::from("codex")));
        assert!(argv.contains(&OsString::from("resume")));
        assert!(argv.contains(&OsString::from("test-session-id")));
        assert!(!argv.contains(&OsString::from("exec")));
        let resume_pos = argv.iter().position(|a| a == "resume").unwrap();
        let id_pos = argv.iter().position(|a| a == "test-session-id").unwrap();
        assert_eq!(id_pos, resume_pos + 1);
    }

    #[test]
    fn test_codex_events_has_json_flag() {
        let argv = build_argv("codex", Some(&"gpt-4".to_string()), true, false, true, None);
        assert!(argv.contains(&OsString::from("--json")));
        assert!(argv.contains(&OsString::from("--yolo")));
        assert!(argv.contains(&OsString::from("-m")));
    }

    #[test]
    fn test_codex_events_with_session_id() {
        let argv = build_argv("codex", None, false, false, true, Some("test-id-123"));
        let argv_str: Vec<&str> = argv.iter().map(|s| s.to_str().unwrap()).collect();
        assert_eq!(argv_str[0], "codex");
        assert_eq!(argv_str[1], "resume");
        assert_eq!(argv_str[2], "test-id-123");
        assert!(argv.contains(&OsString::from("--json")));
        assert!(!argv.contains(&OsString::from("exec")));
    }

    // --- claude ---

    #[test]
    fn test_claude_plain_contains_prompt_and_model() {
        let argv = build_argv(
            "claude",
            Some(&"claude-3-opus".to_string()),
            false,
            true,
            false,
            None,
        );
        assert!(argv.contains(&OsString::from("claude")));
        assert!(argv.contains(&OsString::from("-p")));
        assert!(argv.contains(&OsString::from("-")));
        assert!(!argv.contains(&OsString::from("test prompt")));
        assert!(argv.contains(&OsString::from("--model")));
        assert!(argv.contains(&OsString::from("claude-3-opus")));
        assert!(argv.contains(&OsString::from("--output-format")));
        assert!(argv.contains(&OsString::from("stream-json")));
    }

    #[test]
    fn test_claude_plain_text_format() {
        let argv = build_argv("claude", None, false, false, false, None);
        assert!(argv.contains(&OsString::from("text")));
        assert!(!argv.contains(&OsString::from("stream-json")));
    }

    #[test]
    fn test_claude_plain_with_session_id() {
        let argv = build_argv("claude", None, false, false, false, Some("test-session-id"));
        assert!(argv.contains(&OsString::from("--resume")));
        assert!(argv.contains(&OsString::from("test-session-id")));
    }

    #[test]
    fn test_claude_events_json_format() {
        let argv = build_argv("claude", None, false, false, true, None);
        assert!(argv.contains(&OsString::from("--output-format")));
        assert!(argv.contains(&OsString::from("json")));
    }

    #[test]
    fn test_claude_events_stream_json_format() {
        let argv = build_argv("claude", None, false, true, true, None);
        assert!(argv.contains(&OsString::from("stream-json")));
        assert!(
            argv.contains(&OsString::from("--verbose")),
            "stream-json must include --verbose; got argv: {:?}",
            argv
        );
    }

    #[test]
    fn test_claude_events_json_format_no_verbose() {
        let argv = build_argv("claude", None, false, false, true, None);
        assert!(!argv.contains(&OsString::from("--verbose")));
    }

    #[test]
    fn test_claude_events_with_session_id() {
        let argv = build_argv("claude", None, false, false, true, Some("test-id-456"));
        assert!(argv.contains(&OsString::from("--resume")));
        assert!(argv.contains(&OsString::from("test-id-456")));
        let resume_pos = argv.iter().position(|a| a == "--resume").unwrap();
        assert_eq!(argv[resume_pos + 1], OsString::from("test-id-456"));
    }

    // --- gemini ---

    #[test]
    fn test_gemini_plain_contains_prompt_and_model() {
        let argv = build_argv(
            "gemini",
            Some(&"gemini-pro".to_string()),
            false,
            false,
            false,
            None,
        );
        assert!(argv.contains(&OsString::from("gemini")));
        assert!(argv.contains(&OsString::from("--prompt")));
        assert!(argv.contains(&OsString::from("-")));
        assert!(!argv.contains(&OsString::from("test prompt")));
        assert!(argv.contains(&OsString::from("--model")));
        assert!(argv.contains(&OsString::from("gemini-pro")));
    }

    #[test]
    fn test_gemini_plain_with_session_id() {
        let argv = build_argv("gemini", None, false, false, false, Some("test-session-id"));
        assert!(argv.contains(&OsString::from("--resume")));
        assert!(argv.contains(&OsString::from("test-session-id")));
    }

    #[test]
    fn test_gemini_events_stream_json_headless() {
        let argv = build_argv("gemini", None, false, false, true, None);
        assert!(argv.contains(&OsString::from("--output-format")));
        assert!(argv.contains(&OsString::from("stream-json")));
        assert!(argv.contains(&OsString::from("--yolo")));
        assert!(!argv.contains(&OsString::from("--json")));
    }

    #[test]
    fn test_gemini_events_with_session_id() {
        let argv = build_argv("gemini", None, false, false, true, Some("test-id-789"));
        assert!(argv.contains(&OsString::from("--resume")));
        assert!(argv.contains(&OsString::from("test-id-789")));
        let resume_pos = argv.iter().position(|a| a == "--resume").unwrap();
        assert_eq!(argv[resume_pos + 1], OsString::from("test-id-789"));
    }

    // --- opencode ---

    #[test]
    fn test_opencode_plain_contains_prompt_and_model() {
        let argv = build_argv(
            "opencode",
            Some(&"zai-coding-plan/glm-4.7".to_string()),
            true,
            false,
            false,
            None,
        );
        assert!(argv.contains(&OsString::from("opencode")));
        assert!(argv.contains(&OsString::from("run")));
        assert!(!argv.contains(&OsString::from("test prompt")));
        assert!(argv.contains(&OsString::from("-m")));
        assert!(argv.contains(&OsString::from("zai-coding-plan/glm-4.7")));
        assert!(argv.contains(&OsString::from("--yolo")));
    }

    #[test]
    fn test_opencode_plain_no_options() {
        let argv = build_argv("opencode", None, false, false, false, None);
        assert!(!argv.contains(&OsString::from("--yolo")));
    }

    #[test]
    fn test_opencode_plain_with_session_id() {
        let argv = build_argv(
            "opencode",
            None,
            false,
            false,
            false,
            Some("test-session-id"),
        );
        assert!(argv.contains(&OsString::from("--session")));
        assert!(argv.contains(&OsString::from("test-session-id")));
    }

    #[test]
    fn test_opencode_events_uses_run_subcommand() {
        let argv = build_argv("opencode", None, false, false, true, None);
        assert!(argv.contains(&OsString::from("opencode")));
        assert!(argv.contains(&OsString::from("run")));
        assert!(!argv.contains(&OsString::from("test prompt")));
        assert!(argv.contains(&OsString::from("--format")));
        assert!(argv.contains(&OsString::from("json")));
        assert!(!argv.contains(&OsString::from("--json")));
        assert!(!argv.contains(&OsString::from("--prompt")));
    }

    #[test]
    fn test_opencode_events_with_model() {
        let model = "zai-coding-plan/glm-4.7".to_string();
        let argv = build_argv("opencode", Some(&model), false, false, true, None);
        assert!(argv.contains(&OsString::from("-m")));
        assert!(argv.contains(&OsString::from("zai-coding-plan/glm-4.7")));
        let m_pos = argv.iter().position(|a| a == "-m").unwrap();
        let run_pos = argv.iter().position(|a| a == "run").unwrap();
        assert!(m_pos < run_pos);
    }

    #[test]
    fn test_opencode_events_with_session_id() {
        let argv = build_argv("opencode", None, false, false, true, Some("test-id-abc"));
        assert!(argv.contains(&OsString::from("--session")));
        assert!(argv.contains(&OsString::from("test-id-abc")));
        let session_pos = argv.iter().position(|a| a == "--session").unwrap();
        assert_eq!(argv[session_pos + 1], OsString::from("test-id-abc"));
    }

    // --- agent (cursor) ---

    #[test]
    fn test_agent_plain_contains_all_options() {
        let argv = build_argv(
            "agent",
            Some(&"custom-model".to_string()),
            true,
            true,
            false,
            None,
        );
        assert!(argv.contains(&OsString::from("agent")));
        assert!(argv.contains(&OsString::from("--print")));
        assert!(argv.contains(&OsString::from("--output-format")));
        assert!(argv.contains(&OsString::from("json")));
        assert!(argv.contains(&OsString::from("--model")));
        assert!(argv.contains(&OsString::from("custom-model")));
        assert!(argv.contains(&OsString::from("--force")));
        assert!(!argv.contains(&OsString::from("test prompt")));
        assert!(!argv.contains(&OsString::from("--prompt")));
        assert!(!argv.contains(&OsString::from("--yolo")));
    }

    #[test]
    fn test_agent_plain_with_session_id() {
        let argv = build_argv("agent", None, false, false, false, Some("test-session-id"));
        assert!(argv.contains(&OsString::from("--resume")));
        assert!(argv.contains(&OsString::from("test-session-id")));
    }

    #[test]
    fn test_agent_events_has_json_flag() {
        let argv = build_argv("agent", None, false, false, true, None);
        assert!(argv.contains(&OsString::from("--print")));
        assert!(argv.contains(&OsString::from("--output-format")));
        assert!(argv.contains(&OsString::from("json")));
        assert!(!argv.contains(&OsString::from("--json")));
    }

    #[test]
    fn test_agent_events_stream_json() {
        let argv = build_argv("agent", None, false, true, true, None);
        assert!(argv.contains(&OsString::from("stream-json")));
        assert!(!argv.contains(&OsString::from("test")));
    }

    #[test]
    fn test_agent_events_with_session_id() {
        let argv = build_argv("agent", None, false, false, true, Some("test-id-def"));
        assert!(argv.contains(&OsString::from("--resume")));
        assert!(argv.contains(&OsString::from("test-id-def")));
        let resume_pos = argv.iter().position(|a| a == "--resume").unwrap();
        assert_eq!(argv[resume_pos + 1], OsString::from("test-id-def"));
    }

    #[test]
    fn test_should_write_stdin() {
        assert!(should_write_stdin("agent"));
        assert!(should_write_stdin("opencode"));
        assert!(should_write_stdin("codex"));
        assert!(should_write_stdin("claude"));
        assert!(should_write_stdin("gemini"));
    }
}
