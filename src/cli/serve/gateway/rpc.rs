//! Bounded JSON-RPC stdio transport. Request callbacks never block the reader.
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        mpsc, Arc, Mutex,
    },
    time::Duration,
};
type Reply = mpsc::SyncSender<Result<Value, String>>;
pub type RequestHandler = Arc<dyn Fn(&str, Value) -> Result<Value, String> + Send + Sync>;
#[derive(Clone)]
pub struct Rpc(Arc<Inner>);
struct Inner {
    writer: mpsc::SyncSender<(Vec<u8>, mpsc::SyncSender<std::io::Result<()>>)>,
    child: Mutex<Child>,
    pending: Mutex<HashMap<String, Reply>>,
    next: AtomicU64,
    inbound: AtomicUsize,
}
impl Rpc {
    pub fn spawn(
        program: &str,
        args: &[String],
        cwd: &std::path::Path,
        notification: Arc<dyn Fn(Value) + Send + Sync>,
        request: RequestHandler,
    ) -> anyhow::Result<Self> {
        let mut cmd = Command::new(program);
        cmd.args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000);
        }
        let mut child = cmd.spawn()?;
        let mut stdin = child.stdin.take().unwrap();
        let (writer, writes) =
            mpsc::sync_channel::<(Vec<u8>, mpsc::SyncSender<std::io::Result<()>>)>(32);
        std::thread::spawn(move || {
            while let Ok((bytes, reply)) = writes.recv() {
                let result = stdin.write_all(&bytes).and_then(|_| stdin.flush());
                let failed = result.is_err();
                let _ = reply.try_send(result);
                if failed {
                    break;
                }
            }
        });
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let rpc = Self(Arc::new(Inner {
            writer,
            child: Mutex::new(child),
            pending: Mutex::new(HashMap::new()),
            next: AtomicU64::new(1),
            inbound: AtomicUsize::new(0),
        }));
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stderr);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match reader.by_ref().take(64 * 1024).read_until(b'\n', &mut buf) {
                    Ok(0) | Err(_) => break,
                    _ => {}
                }
            }
        });
        let weak = Arc::downgrade(&rpc.0);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let frame = match read_frame(&mut reader) {
                    Ok(Some(v)) => v,
                    Ok(None) | Err(_) => break,
                };
                let Some(inner) = weak.upgrade() else { break };
                let peer = Rpc(inner);
                if frame.get("method").is_some() {
                    if let Some(id) = frame.get("id").cloned() {
                        if peer.0.inbound.fetch_add(1, Ordering::SeqCst) >= 32 {
                            peer.0.inbound.fetch_sub(1, Ordering::SeqCst);
                            let _=peer.write(&json!({"jsonrpc":"2.0","id":id,"error":{"code":-32000,"message":"request capacity exceeded"}}));
                            continue;
                        }
                        let handler = request.clone();
                        std::thread::spawn(move || {
                            let method = frame["method"].as_str().unwrap_or("");
                            let result = handler(
                                method,
                                frame.get("params").cloned().unwrap_or(Value::Null),
                            );
                            let response = match result {
                                Ok(value) => json!({"jsonrpc":"2.0","id":id,"result":value}),
                                Err(message) => {
                                    json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":message}})
                                }
                            };
                            let _ = peer.write(&response);
                            peer.0.inbound.fetch_sub(1, Ordering::SeqCst);
                        });
                    } else {
                        notification(frame);
                    }
                } else if let Some(id) = frame.get("id") {
                    if let Some(reply) = peer.0.pending.lock().unwrap().remove(&id.to_string()) {
                        let result = if let Some(err) = frame.get("error") {
                            Err(err.to_string())
                        } else {
                            Ok(frame.get("result").cloned().unwrap_or(Value::Null))
                        };
                        let _ = reply.send(result);
                    }
                }
            }
            if let Some(inner) = weak.upgrade() {
                for (_, reply) in inner.pending.lock().unwrap().drain() {
                    let _ = reply.send(Err("agent transport closed".into()));
                }
            }
            notification(json!({"method":"aikit/transportClosed","params":{}}));
        });
        Ok(rpc)
    }
    fn write(&self, value: &Value) -> anyhow::Result<()> {
        self.write_timeout(value, Duration::from_secs(30))
    }
    fn write_timeout(&self, value: &Value, deadline: Duration) -> anyhow::Result<()> {
        let mut bytes = serde_json::to_vec(value)?;
        anyhow::ensure!(bytes.len() <= 1024 * 1024, "frame_too_large");
        bytes.push(b'\n');
        let (tx, rx) = mpsc::sync_channel(1);
        self.0
            .writer
            .try_send((bytes, tx))
            .map_err(|_| anyhow::anyhow!("native_write_unavailable"))?;
        match rx.recv_timeout(deadline) {
            Ok(result) => Ok(result?),
            Err(_) => {
                self.close();
                anyhow::bail!("native_write_timeout");
            }
        }
    }
    pub fn notify(&self, method: &str, params: Value) -> anyhow::Result<()> {
        self.write(&json!({"jsonrpc":"2.0","method":method,"params":params}))
    }
    pub fn request(&self, method: &str, params: Value, timeout: Duration) -> anyhow::Result<Value> {
        let id = self.0.next.fetch_add(1, Ordering::Relaxed);
        let key = id.to_string();
        let (tx, rx) = mpsc::sync_channel(1);
        {
            let mut pending = self.0.pending.lock().unwrap();
            anyhow::ensure!(pending.len() < 64, "outbound request capacity exceeded");
            pending.insert(key.clone(), tx);
        }
        let started = std::time::Instant::now();
        let result = self
            .write_timeout(
                &json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
                timeout.min(Duration::from_secs(30)),
            )
            .and_then(|_| {
                rx.recv_timeout(timeout.saturating_sub(started.elapsed()))
                    .map_err(|_| {
                        self.close();
                        anyhow::anyhow!("native_request_timeout")
                    })?
                    .map_err(anyhow::Error::msg)
            });
        self.0.pending.lock().unwrap().remove(&key);
        result
    }
    pub fn close(&self) {
        let mut child = self.0.child.lock().unwrap();
        kill_group(&mut child);
        let _ = child.kill();
        let _ = child.wait();
        for (_, reply) in self.0.pending.lock().unwrap().drain() {
            let _ = reply.send(Err("session closed".into()));
        }
    }
}
impl Drop for Inner {
    fn drop(&mut self) {
        if let Ok(child) = self.child.get_mut() {
            kill_group(child);
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
fn kill_group(child: &mut Child) {
    #[cfg(unix)]
    if child.try_wait().ok().flatten().is_none() {
        let _ = nix::sys::signal::killpg(
            nix::unistd::Pid::from_raw(child.id() as i32),
            nix::sys::signal::Signal::SIGKILL,
        );
    }
    #[cfg(not(unix))]
    let _ = child;
}
fn read_frame<R: BufRead>(reader: &mut R) -> anyhow::Result<Option<Value>> {
    let mut bytes = Vec::new();
    let n = reader.take(1024 * 1024 + 1).read_until(b'\n', &mut bytes)?;
    if n == 0 {
        return Ok(None);
    }
    anyhow::ensure!(
        n <= 1024 * 1024 && bytes.last() == Some(&b'\n'),
        "invalid or oversized RPC frame"
    );
    Ok(Some(serde_json::from_slice(&bytes)?))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_frames() {
        assert!(read_frame(&mut std::io::Cursor::new(vec![b'x'; 1024 * 1024 + 1])).is_err());
        assert!(read_frame(&mut std::io::Cursor::new(b"{\"id\":1}")).is_err());
        assert_eq!(
            read_frame(&mut std::io::Cursor::new(b"{\"id\":1}\n"))
                .unwrap()
                .unwrap()["id"],
            1
        );
    }
}

#[cfg(test)]
mod deadline_tests {
    use super::*;
    #[test]
    fn timeout_bounds_both_pipe_writes_and_silent_replies_and_reaps_child() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join(if cfg!(windows) {
            "deadline-peer.exe"
        } else {
            "deadline-peer"
        });
        assert!(Command::new("rustc")
            .args(["--edition=2021", "tests/fixtures/acp_peer.rs", "-o"])
            .arg(&exe)
            .status()
            .unwrap()
            .success());
        for blocked_write in [false, true] {
            let args = if blocked_write {
                vec!["no-read".into()]
            } else {
                vec![]
            };
            let rpc = Rpc::spawn(
                exe.to_str().unwrap(),
                &args,
                dir.path(),
                Arc::new(|_| {}),
                Arc::new(|_, _| Ok(json!({}))),
            )
            .unwrap();
            let start = std::time::Instant::now();
            let payload = if blocked_write {
                json!({"text":"x".repeat(512*1024)})
            } else {
                json!({})
            };
            assert!(rpc
                .request("silent", payload, Duration::from_millis(100))
                .is_err());
            assert!(start.elapsed() < Duration::from_secs(3));
            assert!(rpc.0.pending.lock().unwrap().is_empty());
            assert!(rpc.0.child.lock().unwrap().try_wait().unwrap().is_some());
        }
    }
}
