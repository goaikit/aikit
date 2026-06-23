//! Shared argv-building helpers.
//!
//! Each Backend owns its own `argv(...)` assembly in `backends/<name>.rs`, but
//! the flag-pushing mechanics (model / yolo / session flags) are identical and
//! shared here via [`ArgvSpec`]. Behaviour is identical to the former
//! `runner/argv.rs`.

use std::ffi::OsString;

#[derive(Clone, Copy)]
pub(crate) enum SessionMode {
    /// Session id is appended as a positional argument (handled per-Backend).
    Positional,
    /// Session id is passed via the given flag.
    Flag(&'static str),
}

/// Per-Backend CLI flag conventions shared by the argv builders.
pub(crate) struct ArgvSpec {
    /// The spawn binary name (resolved via `command_resolve`).
    pub binary: &'static str,
    pub model_flag: &'static str,
    pub yolo_flag: Option<&'static str>,
    pub session_mode: SessionMode,
}

impl ArgvSpec {
    pub(crate) fn push_model(&self, argv: &mut Vec<OsString>, model: Option<&String>) {
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

    pub(crate) fn push_yolo(&self, argv: &mut Vec<OsString>, yolo: bool) {
        if let Some(flag) = self.yolo_flag {
            if yolo {
                argv.push(OsString::from(flag));
            }
        }
    }

    pub(crate) fn push_session_flag(&self, argv: &mut Vec<OsString>, session_id: Option<&str>) {
        if let SessionMode::Flag(flag) = self.session_mode {
            if let Some(id) = session_id {
                argv.push(OsString::from(flag));
                argv.push(OsString::from(id));
            }
        }
    }
}

/// Inputs to a Backend's argv builder.
#[derive(Clone, Copy)]
pub(crate) struct ArgvCtx<'a> {
    pub model: Option<&'a String>,
    pub yolo: bool,
    pub stream: bool,
    pub events_mode: bool,
    pub session_id: Option<&'a str>,
}
