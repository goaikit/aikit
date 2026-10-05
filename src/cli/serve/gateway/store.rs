use aikit_sdk::runner::session::*;
use rusqlite::{params, Connection, OptionalExtension};
use std::{path::Path, sync::Mutex};

pub struct Store {
    connection: Mutex<Connection>,
    pub host_id: String,
    _lock: Option<std::fs::File>,
}
const MAX_EVENTS: u64 = 10_000;
const MAX_BYTES: u64 = 16 * 1024 * 1024;
impl Store {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let lock = if path == Path::new(":memory:") {
            None
        } else {
            let file = std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(path.with_extension("lock"))?;
            fs2::FileExt::try_lock_exclusive(&file)
                .map_err(|_| anyhow::anyhow!("another host process owns this session store"))?;
            Some(file)
        };
        let c = Connection::open(path)?;
        c.busy_timeout(std::time::Duration::from_secs(5))?;
        c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA max_page_count=131072; PRAGMA journal_size_limit=4194304;
          CREATE TABLE IF NOT EXISTS host(id TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS sessions(id TEXT PRIMARY KEY, body TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS commands(scope TEXT NOT NULL, id TEXT NOT NULL, body TEXT NOT NULL, receipt TEXT NOT NULL, PRIMARY KEY(scope,id));
          CREATE TABLE IF NOT EXISTS events(session TEXT NOT NULL, seq INTEGER NOT NULL, body TEXT NOT NULL, bytes INTEGER NOT NULL, PRIMARY KEY(session,seq));
          CREATE TABLE IF NOT EXISTS requests(session TEXT NOT NULL, id TEXT NOT NULL, body TEXT NOT NULL, PRIMARY KEY(session,id));
          CREATE TABLE IF NOT EXISTS event_budget(session TEXT PRIMARY KEY, bytes INTEGER NOT NULL DEFAULT 0);
          CREATE TRIGGER IF NOT EXISTS event_added AFTER INSERT ON events BEGIN INSERT INTO event_budget VALUES(NEW.session,NEW.bytes) ON CONFLICT(session) DO UPDATE SET bytes=bytes+NEW.bytes; END;
          CREATE TRIGGER IF NOT EXISTS event_removed AFTER DELETE ON events BEGIN UPDATE event_budget SET bytes=bytes-OLD.bytes WHERE session=OLD.session; END;")?;
        let host_id = c
            .query_row("SELECT id FROM host LIMIT 1", [], |r| r.get(0))
            .optional()?
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        c.execute(
            "INSERT INTO host(id) SELECT ?1 WHERE NOT EXISTS(SELECT 1 FROM host)",
            [&host_id],
        )?;
        let store = Self {
            connection: Mutex::new(c),
            host_id,
            _lock: lock,
        };
        for mut info in store.list()? {
            if matches!(
                info.status,
                SessionStatus::Opening
                    | SessionStatus::Running
                    | SessionStatus::Idle
                    | SessionStatus::Closing
            ) {
                info.status = SessionStatus::Interrupted;
                info.active_turn_id = None;
                store.save(&info)?;
                store.append(
                    &info.session_id,
                    SessionEventKind::State(SessionStatus::Interrupted),
                )?;
            }
        }
        {
            let c = store.connection.lock().unwrap();
            let rows = c
                .prepare("SELECT scope,id,receipt FROM commands")?
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            for (scope, id, body) in rows {
                let mut receipt: CommandReceipt = serde_json::from_str(&body)?;
                if receipt.status == CommandStatus::Accepted {
                    receipt.status = CommandStatus::OutcomeUnknown;
                    receipt.failure = Some(CommandFailure {
                        code: "outcome_unknown".into(),
                        retry: "inspect_receipt".into(),
                    });
                    receipt.error=Some("Host restarted before dispatch outcome was recorded; do not automatically retry".into());
                    c.execute(
                        "UPDATE commands SET receipt=?3 WHERE scope=?1 AND id=?2",
                        params![scope, id, serde_json::to_string(&receipt)?],
                    )?;
                }
            }
            c.execute("DELETE FROM requests", [])?;
        }
        Ok(store)
    }
    pub fn save(&self, info: &SessionInfo) -> anyhow::Result<()> {
        self.connection.lock().unwrap().execute("INSERT INTO sessions(id,body) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET body=excluded.body",params![info.session_id,serde_json::to_string(info)?])?;
        Ok(())
    }
    pub fn create(
        &self,
        info: &SessionInfo,
        body: &str,
        receipt: &CommandReceipt,
    ) -> anyhow::Result<()> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction()?;
        tx.execute(
            "INSERT INTO sessions(id,body) VALUES(?1,?2)",
            params![info.session_id, serde_json::to_string(info)?],
        )?;
        tx.execute(
            "INSERT INTO commands(scope,id,body,receipt) VALUES('create',?1,?2,?3)",
            params![receipt.command_id, body, serde_json::to_string(receipt)?],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn get(&self, id: &str) -> anyhow::Result<SessionInfo> {
        let body: String = self.connection.lock().unwrap().query_row(
            "SELECT body FROM sessions WHERE id=?1",
            [id],
            |r| r.get(0),
        )?;
        Ok(serde_json::from_str(&body)?)
    }
    pub fn update(&self, id: &str, change: impl FnOnce(&mut SessionInfo)) -> anyhow::Result<()> {
        let c = self.connection.lock().unwrap();
        let body: String =
            c.query_row("SELECT body FROM sessions WHERE id=?1", [id], |r| r.get(0))?;
        let mut info: SessionInfo = serde_json::from_str(&body)?;
        change(&mut info);
        c.execute(
            "UPDATE sessions SET body=?2 WHERE id=?1",
            params![id, serde_json::to_string(&info)?],
        )?;
        Ok(())
    }
    pub fn begin_turn(&self, id: &str) -> anyhow::Result<()> {
        let c = self.connection.lock().unwrap();
        let body: String =
            c.query_row("SELECT body FROM sessions WHERE id=?1", [id], |r| r.get(0))?;
        let mut info: SessionInfo = serde_json::from_str(&body)?;
        anyhow::ensure!(info.status == SessionStatus::Idle, "turn_not_idle");
        info.status = SessionStatus::Running;
        info.active_turn_id = Some(uuid::Uuid::new_v4().to_string());
        c.execute(
            "UPDATE sessions SET body=?2 WHERE id=?1",
            params![id, serde_json::to_string(&info)?],
        )?;
        Ok(())
    }
    pub fn lookup(&self, scope: &str, id: &str) -> anyhow::Result<CommandReceipt> {
        let c = self.connection.lock().unwrap();
        let raw: String = c.query_row(
            "SELECT receipt FROM commands WHERE scope=?1 AND id=?2",
            params![scope, id],
            |r| r.get(0),
        )?;
        Ok(serde_json::from_str(&raw)?)
    }
    pub fn list(&self) -> anyhow::Result<Vec<SessionInfo>> {
        let c = self.connection.lock().unwrap();
        let mut stmt = c.prepare("SELECT body FROM sessions ORDER BY rowid DESC")?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows.iter().map(|s| Ok(serde_json::from_str(s)?)).collect()
    }
    pub fn receipt(
        &self,
        scope: &str,
        id: &str,
        body: &str,
    ) -> anyhow::Result<Option<CommandReceipt>> {
        let found: Option<(String, String)> = self
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT body,receipt FROM commands WHERE scope=?1 AND id=?2",
                params![scope, id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        match found {
            Some((old, receipt)) => {
                anyhow::ensure!(old == body, "idempotency_conflict");
                Ok(Some(serde_json::from_str(&receipt)?))
            }
            None => Ok(None),
        }
    }
    /// Caller serializes acceptance within a session (or the host creation gate).
    pub fn accept(&self, scope: &str, body: &str, receipt: &CommandReceipt) -> anyhow::Result<()> {
        self.connection.lock().unwrap().execute(
            "INSERT INTO commands(scope,id,body,receipt) VALUES(?1,?2,?3,?4)",
            params![
                scope,
                receipt.command_id,
                body,
                serde_json::to_string(receipt)?
            ],
        )?;
        Ok(())
    }
    pub fn finish(&self, scope: &str, receipt: &CommandReceipt) -> anyhow::Result<()> {
        self.connection.lock().unwrap().execute(
            "UPDATE commands SET receipt=?3 WHERE scope=?1 AND id=?2",
            params![scope, receipt.command_id, serde_json::to_string(receipt)?],
        )?;
        Ok(())
    }
    pub fn append(&self, id: &str, mut event: SessionEventKind) -> anyhow::Result<SessionEvent> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction()?;
        let raw: String =
            tx.query_row("SELECT body FROM sessions WHERE id=?1", [id], |r| r.get(0))?;
        let mut info: SessionInfo = serde_json::from_str(&raw)?;
        let turn_id = info.active_turn_id.clone();
        if let SessionEventKind::State(next) = &event {
            let ended = matches!(
                info.status,
                SessionStatus::Closed | SessionStatus::Failed | SessionStatus::Interrupted
            );
            let reopening = !matches!(
                next,
                SessionStatus::Closed | SessionStatus::Failed | SessionStatus::Interrupted
            );
            let hides_failure = *next == SessionStatus::Closed
                && matches!(
                    info.status,
                    SessionStatus::Failed | SessionStatus::Interrupted
                );
            if (ended && reopening) || hides_failure {
                event = SessionEventKind::State(info.status.clone());
            }
        }
        match &event {
            SessionEventKind::State(status) => {
                info.status = status.clone();
                if *status != SessionStatus::Running {
                    info.active_turn_id = None;
                }
            }
            SessionEventKind::Agent(e) => match &e.payload {
                aikit_sdk::AgentEventPayload::SessionStarted { session_id } => {
                    info.native_session_id = Some(session_id.clone())
                }
                aikit_sdk::AgentEventPayload::Terminal { .. }
                    if matches!(info.status, SessionStatus::Opening | SessionStatus::Running) =>
                {
                    info.status = SessionStatus::Idle;
                    info.active_turn_id = None;
                }
                _ => {}
            },
            _ => {}
        }
        info.last_sequence += 1;
        let envelope = SessionEvent {
            version: PROTOCOL_VERSION,
            host_id: self.host_id.clone(),
            session_id: id.into(),
            sequence: info.last_sequence,
            turn_id,
            timestamp_ms: now_ms(),
            event,
        };
        let encoded = serde_json::to_string(&envelope)?;
        anyhow::ensure!(encoded.len() <= 1024 * 1024, "event_too_large");
        tx.execute(
            "INSERT INTO events VALUES(?1,?2,?3,?4)",
            params![id, envelope.sequence as i64, encoded, encoded.len() as i64],
        )?;
        tx.execute(
            "UPDATE sessions SET body=?2 WHERE id=?1",
            params![id, serde_json::to_string(&info)?],
        )?;
        tx.execute(
            "DELETE FROM events WHERE session=?1 AND seq<=?2",
            params![id, envelope.sequence.saturating_sub(MAX_EVENTS) as i64],
        )?;
        let mut bytes: i64 = tx.query_row(
            "SELECT bytes FROM event_budget WHERE session=?1",
            [id],
            |r| r.get(0),
        )?;
        while bytes > MAX_BYTES as i64 {
            let (seq, size): (i64, i64) = tx.query_row(
                "SELECT seq,bytes FROM events WHERE session=?1 ORDER BY seq LIMIT 1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            tx.execute(
                "DELETE FROM events WHERE session=?1 AND seq=?2",
                params![id, seq],
            )?;
            bytes -= size;
        }
        tx.commit()?;
        Ok(envelope)
    }
    pub fn replay(&self, id: &str, after: u64) -> anyhow::Result<Vec<SessionEvent>> {
        let c = self.connection.lock().unwrap();
        let first: Option<i64> =
            c.query_row("SELECT MIN(seq) FROM events WHERE session=?1", [id], |r| {
                r.get(0)
            })?;
        anyhow::ensure!(
            first.map_or(true, |n| after.saturating_add(1) >= n as u64),
            "cursor_expired"
        );
        let mut stmt = c.prepare(
            "SELECT body FROM events WHERE session=?1 AND seq>?2 ORDER BY seq LIMIT 128",
        )?;
        let rows = stmt.query_map(params![id, after as i64], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        let mut bytes = 0;
        for row in rows {
            let body = row?;
            bytes += body.len();
            if bytes > 1024 * 1024 && !out.is_empty() {
                break;
            }
            out.push(serde_json::from_str(&body)?);
        }
        Ok(out)
    }
    pub fn request(&self, id: &str, request: &PendingRequest) -> anyhow::Result<()> {
        self.connection.lock().unwrap().execute(
            "INSERT INTO requests VALUES(?1,?2,?3)",
            params![id, request.request_id, serde_json::to_string(request)?],
        )?;
        Ok(())
    }
    pub fn resolve(&self, id: &str, request: &str) -> anyhow::Result<()> {
        self.connection.lock().unwrap().execute(
            "DELETE FROM requests WHERE session=?1 AND id=?2",
            params![id, request],
        )?;
        Ok(())
    }
    pub fn pending(&self, id: &str) -> anyhow::Result<Vec<PendingRequest>> {
        let c = self.connection.lock().unwrap();
        let mut stmt = c.prepare("SELECT body FROM requests WHERE session=?1")?;
        let rows = stmt
            .query_map([id], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows.iter().map(|s| Ok(serde_json::from_str(s)?)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn info() -> SessionInfo {
        SessionInfo {
            host_id: "h".into(),
            session_id: "s".into(),
            backend: SessionBackend::Pi,
            cwd: ".".into(),
            status: SessionStatus::Running,
            capabilities: SessionCapabilities::default(),
            native_session_id: None,
            active_turn_id: Some("t".into()),
            last_sequence: 0,
        }
    }
    #[test]
    fn restart_retains_events_and_marks_uncertain_commands() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sessions.db");
        {
            let s = Store::open(&path).unwrap();
            s.save(&info()).unwrap();
            s.append("s", SessionEventKind::State(SessionStatus::Running))
                .unwrap();
            s.accept(
                "s",
                "body",
                &CommandReceipt {
                    command_id: "c".into(),
                    session_id: "s".into(),
                    status: CommandStatus::Accepted,
                    result: None,
                    error: None,
                    failure: None,
                },
            )
            .unwrap();
        }
        let s = Store::open(&path).unwrap();
        assert_eq!(s.get("s").unwrap().status, SessionStatus::Interrupted);
        assert_eq!(s.replay("s", 0).unwrap().len(), 2);
        assert_eq!(
            s.receipt("s", "c", "body").unwrap().unwrap().status,
            CommandStatus::OutcomeUnknown
        );
        assert!(s.receipt("s", "c", "different").is_err());
    }

    #[test]
    fn late_native_cleanup_cannot_erase_failure_or_reopen_session() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        store.save(&info()).unwrap();
        store
            .append("s", SessionEventKind::State(SessionStatus::Failed))
            .unwrap();
        store
            .append("s", SessionEventKind::State(SessionStatus::Closed))
            .unwrap();
        let last = store
            .append("s", SessionEventKind::State(SessionStatus::Idle))
            .unwrap();
        assert_eq!(store.get("s").unwrap().status, SessionStatus::Failed);
        assert!(matches!(
            last.event,
            SessionEventKind::State(SessionStatus::Failed)
        ));
    }
    #[test]
    fn event_bytes_bound_replay_and_expire_cursor() {
        let s = Store::open(Path::new(":memory:")).unwrap();
        s.save(&info()).unwrap();
        for _ in 0..20 {
            s.append(
                "s",
                SessionEventKind::Native {
                    protocol: "fixture".into(),
                    value: serde_json::json!("x".repeat(900_000)),
                },
            )
            .unwrap();
        }
        assert!(s.replay("s", 0).is_err());
        assert_eq!(s.replay("s", 19).unwrap().len(), 1);
    }
    #[test]
    fn only_one_host_can_own_store() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sessions.db");
        let first = Store::open(&path).unwrap();
        assert!(Store::open(&path).is_err());
        drop(first);
        assert!(Store::open(&path).is_ok());
    }
    #[test]
    fn concurrent_updates_preserve_event_sequences() {
        let store = std::sync::Arc::new(Store::open(Path::new(":memory:")).unwrap());
        store.save(&info()).unwrap();
        let workers: Vec<_> = (0..4)
            .map(|_| {
                let store = store.clone();
                std::thread::spawn(move || {
                    for _ in 0..50 {
                        store
                            .append(
                                "s",
                                SessionEventKind::Native {
                                    protocol: "test".into(),
                                    value: serde_json::json!({}),
                                },
                            )
                            .unwrap();
                        store
                            .update("s", |i| i.capabilities.send_turn = true)
                            .unwrap();
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(store.get("s").unwrap().last_sequence, 200);
        assert_eq!(
            store.replay("s", 128).unwrap().last().unwrap().sequence,
            200
        );
    }
}
