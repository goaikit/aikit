//! The digest: the bounded, deterministic text a model sees for one
//! session. Built from the event store, never from a transcript (ADR 0022).

use aikit_session_capture::{
    ActionKind, ActionStatus, AreaTouch, SessionSummary, ToolEvent, ToolKind,
};

use crate::areas::{relative_target, touch_kind};

/// Character budgets. The defaults keep a digest near 12 000 characters, a
/// few thousand tokens, whatever the session's size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DigestOptions {
    /// Add the assistant's text blocks (a richer "why", more tokens).
    pub include_assistant: bool,
    pub max_prompts: usize,
    pub prompt_chars: usize,
    pub prompts_chars: usize,
    pub max_files: usize,
    pub max_commands: usize,
    pub command_chars: usize,
    pub final_message_chars: usize,
    pub max_assistant_notes: usize,
    pub assistant_chars: usize,
    pub max_total_chars: usize,
}

impl Default for DigestOptions {
    fn default() -> Self {
        Self {
            include_assistant: false,
            max_prompts: 12,
            prompt_chars: 600,
            prompts_chars: 3_000,
            max_files: 120,
            max_commands: 40,
            command_chars: 200,
            final_message_chars: 1_500,
            max_assistant_notes: 20,
            assistant_chars: 3_000,
            max_total_chars: 12_000,
        }
    }
}

/// Where the digest's prompts come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptsInput {
    /// Prompts the history reader returned (already scrubbed by the caller).
    History(Vec<String>),
    /// Use the prompt events in the store, if the adapter emitted any.
    Events,
}

/// One line of the files section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileLine {
    pub kind: ActionKind,
    pub path: String,
}

/// One line of the commands section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandLine {
    pub command: String,
    pub failed: bool,
}

/// The structured digest. [`Digest::render`] turns it into the text sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Digest {
    pub tool: ToolKind,
    pub session_id: String,
    pub started_at_ms: i64,
    pub ended_at_ms: i64,
    pub git_root: Option<String>,
    pub action_count: u64,
    pub prompts: Vec<String>,
    /// `"history"`, `"events"`, or `None` when there were no prompts.
    pub prompt_source: Option<&'static str>,
    pub files: Vec<FileLine>,
    pub files_omitted: usize,
    pub commands: Vec<CommandLine>,
    pub commands_omitted: usize,
    pub areas: Vec<AreaTouch>,
    pub final_message: Option<String>,
    pub assistant_notes: Vec<String>,
}

/// Cut `s` to at most `max` characters, marking the cut.
pub fn clip(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn is_prompt_event(e: &ToolEvent) -> bool {
    e.kind == ActionKind::Other
        && e.metadata.get("kind").and_then(|v| v.as_str()) == Some("user_prompt")
}

/// Build the digest for one session from its events (any order).
pub fn build_digest(
    session: &SessionSummary,
    events: &[ToolEvent],
    prompts: PromptsInput,
    areas: Vec<AreaTouch>,
    opts: &DigestOptions,
) -> Digest {
    let mut ordered: Vec<&ToolEvent> = events.iter().collect();
    ordered.sort_by_key(|e| (e.started_at_ms.unwrap_or(0), e.source_event_id.clone()));
    let git_root = session.git_root.as_deref();

    // Prompts: the history reader's when it had any, else prompt events.
    let (raw_prompts, prompt_source): (Vec<String>, Option<&'static str>) = match prompts {
        PromptsInput::History(p) if !p.is_empty() => (p, Some("history")),
        _ => {
            let from_events: Vec<String> = ordered
                .iter()
                .filter(|e| is_prompt_event(e))
                .filter_map(|e| e.input.clone().or_else(|| e.target.clone()))
                .collect();
            let src = (!from_events.is_empty()).then_some("events");
            (from_events, src)
        }
    };
    let mut prompts_out = Vec::new();
    let mut prompt_budget = opts.prompts_chars;
    for p in raw_prompts.iter().take(opts.max_prompts) {
        if prompt_budget == 0 {
            break;
        }
        let clipped = clip(p, opts.prompt_chars.min(prompt_budget));
        prompt_budget = prompt_budget.saturating_sub(clipped.chars().count());
        prompts_out.push(clipped);
    }

    // Files touched, in order, consecutive duplicates collapsed.
    let mut files: Vec<FileLine> = Vec::new();
    let mut files_total = 0usize;
    for e in &ordered {
        if touch_kind(e.kind).is_none() {
            continue;
        }
        let Some(t) = e.target.as_deref().filter(|t| !t.is_empty()) else {
            continue;
        };
        let line = FileLine {
            kind: e.kind,
            path: relative_target(t, git_root),
        };
        if files.last() == Some(&line) {
            continue;
        }
        files_total += 1;
        if files.len() < opts.max_files {
            files.push(line);
        }
    }
    let files_omitted = files_total.saturating_sub(files.len());

    // Commands run.
    let mut commands: Vec<CommandLine> = Vec::new();
    let mut commands_total = 0usize;
    for e in ordered.iter().filter(|e| e.kind == ActionKind::Bash) {
        let Some(cmd) = e
            .target
            .as_deref()
            .or(e.input.as_deref())
            .filter(|c| !c.trim().is_empty())
        else {
            continue;
        };
        commands_total += 1;
        if commands.len() < opts.max_commands {
            commands.push(CommandLine {
                command: clip(cmd, opts.command_chars),
                failed: e.status == ActionStatus::Failure,
            });
        }
    }
    let commands_omitted = commands_total.saturating_sub(commands.len());

    // Assistant text: the last block is the final message; the rest are
    // notes, included only on request.
    let assistant: Vec<&str> = ordered
        .iter()
        .filter(|e| e.kind == ActionKind::Think)
        .filter_map(|e| e.input.as_deref())
        .filter(|s| !s.trim().is_empty())
        .collect();
    let final_message = assistant.last().map(|s| clip(s, opts.final_message_chars));
    let mut assistant_notes = Vec::new();
    if opts.include_assistant && assistant.len() > 1 {
        let notes = &assistant[..assistant.len() - 1];
        let take = notes.len().min(opts.max_assistant_notes);
        let per = (opts.assistant_chars / take.max(1)).max(80);
        assistant_notes = notes[notes.len() - take..]
            .iter()
            .map(|s| clip(s, per))
            .collect();
    }

    Digest {
        tool: session.tool,
        session_id: session.session_id.clone(),
        started_at_ms: session.first_event_at_ms,
        ended_at_ms: session.last_event_at_ms,
        git_root: git_root.map(|p| p.display().to_string()),
        action_count: session.action_count,
        prompts: prompts_out,
        prompt_source,
        files,
        files_omitted,
        commands,
        commands_omitted,
        areas,
        final_message,
        assistant_notes,
    }
}

/// RFC 3339 to the second, UTC; `?` when the timestamp is missing.
pub fn fmt_ms(ms: i64) -> String {
    if ms <= 0 {
        return "?".into();
    }
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|t| t.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .unwrap_or_else(|| "?".into())
}

fn fmt_duration(ms: u64) -> String {
    let s = ms / 1000;
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m{:02}s", s / 60, s % 60)
    } else {
        format!("{}h{:02}m", s / 3600, (s % 3600) / 60)
    }
}

impl Digest {
    /// The text sent as the body of the user message. Deterministic for the
    /// same inputs; capped at `max_total_chars`.
    pub fn render(&self, opts: &DigestOptions) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "# Session {} ({})\n",
            self.session_id,
            self.tool.as_str()
        ));
        out.push_str(&format!(
            "Started: {}  Ended: {}  Duration: {}  Actions: {}\n",
            fmt_ms(self.started_at_ms),
            fmt_ms(self.ended_at_ms),
            fmt_duration((self.ended_at_ms - self.started_at_ms).max(0) as u64),
            self.action_count
        ));
        if let Some(root) = &self.git_root {
            out.push_str(&format!("Git root: {root}\n"));
        }

        out.push_str(&format!("\n## Prompts ({})\n", self.prompts.len()));
        if self.prompts.is_empty() {
            out.push_str("(none recorded)\n");
        }
        for (i, p) in self.prompts.iter().enumerate() {
            out.push_str(&format!("{}. {}\n", i + 1, p.replace('\n', " ")));
        }

        out.push_str(&format!(
            "\n## Files touched, in order ({})\n",
            self.files.len() + self.files_omitted
        ));
        if self.files.is_empty() {
            out.push_str("(none)\n");
        }
        for f in &self.files {
            out.push_str(&format!("{:<6} {}\n", f.kind.as_str(), f.path));
        }
        if self.files_omitted > 0 {
            out.push_str(&format!("… and {} more\n", self.files_omitted));
        }

        out.push_str(&format!(
            "\n## Commands ({})\n",
            self.commands.len() + self.commands_omitted
        ));
        if self.commands.is_empty() {
            out.push_str("(none)\n");
        }
        for c in &self.commands {
            let mark = if c.failed { "  (failed)" } else { "" };
            out.push_str(&format!("$ {}{mark}\n", c.command.replace('\n', " ")));
        }
        if self.commands_omitted > 0 {
            out.push_str(&format!("… and {} more\n", self.commands_omitted));
        }

        out.push_str("\n## Areas (reads / modifications / time)\n");
        if self.areas.is_empty() {
            out.push_str("(no files touched)\n");
        }
        for a in &self.areas {
            out.push_str(&format!(
                "{}  {} / {} / {}\n",
                a.area,
                a.reads,
                a.modifications,
                fmt_duration(a.time_ms)
            ));
        }

        out.push_str("\n## Final assistant message\n");
        match &self.final_message {
            Some(m) => out.push_str(&format!("{m}\n")),
            None => out.push_str("(none recorded)\n"),
        }

        if !self.assistant_notes.is_empty() {
            out.push_str(&format!(
                "\n## Assistant notes ({})\n",
                self.assistant_notes.len()
            ));
            for n in &self.assistant_notes {
                out.push_str(&format!("- {}\n", n.replace('\n', " ")));
            }
        }

        if out.chars().count() > opts.max_total_chars {
            let mut cut: String = out.chars().take(opts.max_total_chars).collect();
            cut.push_str("\n[digest truncated]\n");
            return cut;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn session() -> SessionSummary {
        SessionSummary {
            tool: ToolKind::Codex,
            session_id: "sess-1".into(),
            source_file: PathBuf::from("/tmp/rollout.jsonl"),
            first_event_at_ms: 1_700_000_000_000,
            last_event_at_ms: 1_700_000_090_000,
            action_count: 6,
            tool_kinds: vec![],
            git_root: Some(PathBuf::from("/repo")),
        }
    }

    fn ev(id: &str, kind: ActionKind, target: &str, input: Option<&str>, at: i64) -> ToolEvent {
        ToolEvent {
            source_event_id: id.into(),
            source_file: PathBuf::from("/tmp/rollout.jsonl"),
            session_id: "sess-1".into(),
            tool: ToolKind::Codex,
            kind,
            target: Some(target.into()),
            input: input.map(str::to_string),
            output: None,
            status: ActionStatus::Success,
            error_message: None,
            started_at_ms: Some(at),
            duration_ms: None,
            git_root: Some(PathBuf::from("/repo")),
            metadata: serde_json::Value::Null,
        }
    }

    fn prompt_ev(id: &str, text: &str, at: i64) -> ToolEvent {
        let mut e = ev(id, ActionKind::Other, text, Some(text), at);
        e.metadata = serde_json::json!({"kind": "user_prompt"});
        e
    }

    fn sample_events() -> Vec<ToolEvent> {
        let mut failed = ev("5", ActionKind::Bash, "cargo test", Some("cargo test"), 50);
        failed.status = ActionStatus::Failure;
        vec![
            prompt_ev("1", "fix the flaky test", 10),
            ev("2", ActionKind::Read, "/repo/src/lib.rs", None, 20),
            ev("2b", ActionKind::Read, "/repo/src/lib.rs", None, 21),
            ev("3", ActionKind::Edit, "/repo/tests/flaky_test.rs", None, 30),
            ev(
                "4",
                ActionKind::Think,
                "working",
                Some("Looking at the test."),
                40,
            ),
            failed,
            ev(
                "6",
                ActionKind::Think,
                "done",
                Some("Fixed by awaiting the join."),
                60,
            ),
        ]
    }

    #[test]
    fn digest_sections_come_from_events_in_order() {
        let opts = DigestOptions::default();
        let d = build_digest(
            &session(),
            &sample_events(),
            PromptsInput::Events,
            vec![],
            &opts,
        );
        assert_eq!(d.prompts, vec!["fix the flaky test"]);
        assert_eq!(d.prompt_source, Some("events"));
        // Consecutive duplicate reads collapse to one line.
        let files: Vec<String> = d
            .files
            .iter()
            .map(|f| format!("{} {}", f.kind.as_str(), f.path))
            .collect();
        assert_eq!(files, vec!["read src/lib.rs", "edit tests/flaky_test.rs"]);
        assert_eq!(d.commands.len(), 1);
        assert!(d.commands[0].failed);
        assert_eq!(
            d.final_message.as_deref(),
            Some("Fixed by awaiting the join.")
        );
        assert!(
            d.assistant_notes.is_empty(),
            "notes only with include_assistant"
        );

        let text = d.render(&opts);
        assert!(text.contains("# Session sess-1 (codex)"));
        assert!(text.contains("Started: 2023-11-14T22:13:20Z"));
        assert!(text.contains("1. fix the flaky test"));
        assert!(text.contains("edit   tests/flaky_test.rs"));
        assert!(text.contains("$ cargo test  (failed)"));
        assert!(text.contains("## Final assistant message\nFixed by awaiting the join."));
        assert!(!text.contains("Assistant notes"));

        // Deterministic: same input, same text.
        let again = build_digest(
            &session(),
            &sample_events(),
            PromptsInput::Events,
            vec![],
            &opts,
        );
        assert_eq!(again.render(&opts), text);
    }

    #[test]
    fn history_prompts_win_and_assistant_notes_are_opt_in() {
        let opts = DigestOptions {
            include_assistant: true,
            ..Default::default()
        };
        let d = build_digest(
            &session(),
            &sample_events(),
            PromptsInput::History(vec!["from history".into()]),
            vec![],
            &opts,
        );
        assert_eq!(d.prompts, vec!["from history"]);
        assert_eq!(d.prompt_source, Some("history"));
        assert_eq!(d.assistant_notes, vec!["Looking at the test."]);
        assert!(d
            .render(&opts)
            .contains("## Assistant notes (1)\n- Looking at the test."));

        // An empty history answer falls back to events.
        let d = build_digest(
            &session(),
            &sample_events(),
            PromptsInput::History(vec![]),
            vec![],
            &opts,
        );
        assert_eq!(d.prompt_source, Some("events"));
    }

    #[test]
    fn budgets_clip_and_count_omissions() {
        let opts = DigestOptions {
            max_prompts: 1,
            prompt_chars: 8,
            max_files: 1,
            max_commands: 1,
            command_chars: 5,
            max_total_chars: 200,
            ..Default::default()
        };
        let mut events = sample_events();
        events.push(prompt_ev("7", "second prompt that is long", 70));
        events.push(ev("8", ActionKind::Bash, "echo hello world", None, 80));
        let d = build_digest(&session(), &events, PromptsInput::Events, vec![], &opts);
        assert_eq!(d.prompts, vec!["fix the…"]);
        assert_eq!(d.files.len(), 1);
        assert_eq!(d.files_omitted, 1);
        assert_eq!(d.commands.len(), 1);
        assert_eq!(d.commands_omitted, 1);
        assert_eq!(d.commands[0].command, "carg…");
        let text = d.render(&opts);
        assert!(text.ends_with("[digest truncated]\n"));
        assert!(text.chars().count() <= 200 + "\n[digest truncated]\n".len());
    }

    #[test]
    fn clip_is_char_safe() {
        assert_eq!(clip("héllo wörld", 6), "héllo…");
        assert_eq!(clip("  short  ", 20), "short");
    }
}
