use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionFile {
    pub session_id: String,
    pub agent: String,
    pub created_at: String,
    pub updated_at: String,
    pub cwd: String,
    pub turns: Vec<SessionTurn>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTurn {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<SessionToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_results: Option<Vec<SessionToolResult>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionToolCall {
    pub id: String,
    pub name: String,
    pub input: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionToolResult {
    pub tool_call_id: String,
    pub name: String,
    pub output: String,
}

#[derive(Debug)]
pub enum SessionStoreError {
    NotFound(String),
    Io(std::io::Error),
    Parse { id: String, reason: String },
}

pub struct SessionStore {
    pub sessions_dir: PathBuf,
}

impl Default for SessionStore {
    fn default() -> Self {
        Self::open()
    }
}

impl SessionStore {
    /// Resolves AIKIT_SESSIONS_DIR env var, then ~/.aikit/sessions/. Creates dir if absent.
    pub fn open() -> Self {
        let sessions_dir = if let Ok(dir) = std::env::var("AIKIT_SESSIONS_DIR") {
            PathBuf::from(dir)
        } else {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".aikit")
                .join("sessions")
        };
        let _ = std::fs::create_dir_all(&sessions_dir);
        Self { sessions_dir }
    }

    pub fn load(&self, id: &str) -> Result<SessionFile, SessionStoreError> {
        let path = self.sessions_dir.join(format!("{}.json", id));
        let content = std::fs::read_to_string(&path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                SessionStoreError::NotFound(id.to_string())
            } else {
                SessionStoreError::Io(e)
            }
        })?;
        serde_json::from_str::<SessionFile>(&content).map_err(|e| SessionStoreError::Parse {
            id: id.to_string(),
            reason: e.to_string(),
        })
    }

    pub fn save(&self, file: &SessionFile) -> Result<(), SessionStoreError> {
        let path = self.sessions_dir.join(format!("{}.json", file.session_id));
        let content = serde_json::to_string_pretty(file)
            .map_err(|e| SessionStoreError::Io(std::io::Error::other(e.to_string())))?;
        std::fs::write(&path, content).map_err(SessionStoreError::Io)
    }

    pub fn update_index(&self, cwd: &str, session_id: &str) -> Result<(), SessionStoreError> {
        let index_path = self.sessions_dir.join("index.json");
        let mut map: HashMap<String, String> = if index_path.exists() {
            let content = std::fs::read_to_string(&index_path).map_err(SessionStoreError::Io)?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            HashMap::new()
        };

        map.insert(cwd.to_string(), session_id.to_string());

        let content = serde_json::to_string_pretty(&map)
            .map_err(|e| SessionStoreError::Io(std::io::Error::other(e.to_string())))?;

        let tmp_path = self.sessions_dir.join(format!("index.tmp.{}", session_id));
        std::fs::write(&tmp_path, content).map_err(SessionStoreError::Io)?;
        std::fs::rename(&tmp_path, &index_path).map_err(SessionStoreError::Io)?;

        Ok(())
    }

    pub fn last_for_cwd(&self, cwd: &str) -> Result<Option<String>, SessionStoreError> {
        let index_path = self.sessions_dir.join("index.json");
        if !index_path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&index_path).map_err(SessionStoreError::Io)?;
        let map: HashMap<String, String> = serde_json::from_str(&content).unwrap_or_default();
        Ok(map.get(cwd).cloned())
    }
}

/// Returns current UTC time as an RFC 3339 string.
pub(crate) fn now_rfc3339() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    unix_secs_to_rfc3339(secs)
}

fn unix_secs_to_rfc3339(secs: u64) -> String {
    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    let mut days = secs / 86400;

    let mut year = 1970u64;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    let month_days: [u64; 12] = if is_leap(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month = 1u64;
    for &d in &month_days {
        if days < d {
            break;
        }
        days -= d;
        month += 1;
    }
    let day = days + 1;

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, h, m, s
    )
}

fn is_leap(year: u64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_store(tmp: &TempDir) -> SessionStore {
        let dir = tmp.path().join("sessions");
        std::fs::create_dir_all(&dir).unwrap();
        SessionStore { sessions_dir: dir }
    }

    fn make_session(id: &str) -> SessionFile {
        SessionFile {
            session_id: id.to_string(),
            agent: "aikit".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            cwd: "/tmp/test".to_string(),
            turns: vec![SessionTurn {
                role: "user".to_string(),
                content: "hello".to_string(),
                tool_calls: None,
                tool_results: None,
            }],
        }
    }

    #[test]
    fn test_load_missing_returns_not_found() {
        let tmp = TempDir::new().unwrap();
        let store = make_store(&tmp);
        let err = store.load("nonexistent").unwrap_err();
        assert!(matches!(err, SessionStoreError::NotFound(_)));
    }

    #[test]
    fn test_save_and_load_round_trip() {
        let tmp = TempDir::new().unwrap();
        let store = make_store(&tmp);
        let session = make_session("test-id-001");
        store.save(&session).unwrap();
        let loaded = store.load("test-id-001").unwrap();
        assert_eq!(loaded.session_id, "test-id-001");
        assert_eq!(loaded.turns.len(), 1);
        assert_eq!(loaded.turns[0].content, "hello");
    }

    #[test]
    fn test_update_index_creates_and_updates() {
        let tmp = TempDir::new().unwrap();
        let store = make_store(&tmp);
        store
            .update_index("/home/user/project", "session-1")
            .unwrap();
        let last = store.last_for_cwd("/home/user/project").unwrap();
        assert_eq!(last, Some("session-1".to_string()));

        store
            .update_index("/home/user/project", "session-2")
            .unwrap();
        let last = store.last_for_cwd("/home/user/project").unwrap();
        assert_eq!(last, Some("session-2".to_string()));
    }

    #[test]
    fn test_last_for_cwd_returns_none_when_absent() {
        let tmp = TempDir::new().unwrap();
        let store = make_store(&tmp);
        let last = store.last_for_cwd("/nonexistent").unwrap();
        assert!(last.is_none());
    }

    #[test]
    fn test_aikit_sessions_dir_env_override() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("custom-sessions");
        std::env::set_var("AIKIT_SESSIONS_DIR", dir.to_str().unwrap());
        let store = SessionStore::open();
        assert!(dir.exists());
        assert_eq!(store.sessions_dir, dir);
        std::env::remove_var("AIKIT_SESSIONS_DIR");
    }

    #[test]
    fn test_load_corrupt_returns_parse_error() {
        let tmp = TempDir::new().unwrap();
        let store = make_store(&tmp);
        let path = store.sessions_dir.join("bad-id.json");
        std::fs::write(&path, "not valid json").unwrap();
        let err = store.load("bad-id").unwrap_err();
        assert!(matches!(err, SessionStoreError::Parse { .. }));
    }

    #[test]
    fn test_unix_secs_to_rfc3339_epoch() {
        assert_eq!(unix_secs_to_rfc3339(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn test_unix_secs_to_rfc3339_known_date() {
        // 2024-01-01T00:00:00Z = 1704067200
        assert_eq!(unix_secs_to_rfc3339(1704067200), "2024-01-01T00:00:00Z");
    }
}
