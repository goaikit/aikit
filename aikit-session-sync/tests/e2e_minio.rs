//! End-to-end sync against a real S3-compatible backend (MinIO in Docker).
//!
//! Drives the actual `SyncEngine` + `S3Sink` write path — no dry-run, no
//! in-memory sink — then reads the objects back with an independent
//! `object_store` client to prove the bytes, the scrubbing chokepoint, and
//! the `.meta.json` envelope all landed correctly. Exercises spec 012 §6/§7
//! against the wire, covering `s3.rs` which unit tests cannot reach.
//!
//! Requires a reachable Docker daemon (as does any testcontainers test). When
//! Docker is genuinely absent the test prints a notice and returns rather than
//! failing spuriously; it never passes vacuously on an assertion path.

use std::path::Path;
use std::sync::Arc;

use aikit_session_capture::{Adapter, AdapterError, ParseResult, ToolKind};
use aikit_session_sync::state::InMemorySyncStateStore;
use aikit_session_sync::{
    S3Sink, S3SinkConfig, SyncConfig, SyncEngine, SyncOutcome, SyncSink, SyncStateStore,
};
use async_trait::async_trait;
use futures::StreamExt;
use object_store::aws::AmazonS3Builder;
use object_store::{ClientOptions, ObjectStore, ObjectStoreExt};
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{GenericImage, ImageExt};

const BUCKET: &str = "sessions-test";

/// Minimal adapter over a temp dir so the e2e is deterministic and independent
/// of a real `~/.claude` layout. Mirrors the JSONL adapters' detection surface;
/// `parse_session_file` must never be called by sync (asserted by panicking).
struct TempDirAdapter {
    kind: ToolKind,
    root: std::path::PathBuf,
}

#[async_trait]
impl Adapter for TempDirAdapter {
    fn kind(&self) -> ToolKind {
        self.kind
    }
    fn watch_paths(&self) -> Vec<std::path::PathBuf> {
        vec![self.root.clone()]
    }
    fn is_session_file(&self, path: &Path) -> bool {
        path.starts_with(&self.root) && path.extension().is_some_and(|e| e == "jsonl")
    }
    async fn parse_session_file(
        &self,
        _path: &Path,
        _from_offset: u64,
    ) -> Result<ParseResult, AdapterError> {
        panic!("session sync must not call parse_session_file")
    }
}

fn docker_available() -> bool {
    std::process::Command::new("docker")
        .arg("version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[tokio::test]
async fn e2e_sync_round_trips_content_scrubbed_and_envelope_via_minio() {
    if !docker_available() {
        eprintln!("SKIP e2e_minio: docker daemon not reachable");
        return;
    }

    // MinIO treats each top-level dir under /data as a bucket, so we create the
    // bucket by pre-making the dir, then launch the server in one command.
    let minio = GenericImage::new("minio/minio", "latest")
        .with_exposed_port(9000.tcp())
        .with_wait_for(WaitFor::message_on_stderr("API:"))
        .with_entrypoint("sh")
        .with_cmd([
            "-c".to_string(),
            format!("mkdir -p /data/{BUCKET} && minio server /data"),
        ])
        .with_env_var("MINIO_ROOT_USER", "minioadmin")
        .with_env_var("MINIO_ROOT_PASSWORD", "minioadmin")
        .start()
        .await
        .expect("start minio container");

    let port = minio
        .get_host_port_ipv4(9000.tcp())
        .await
        .expect("host port");
    let endpoint = format!("http://127.0.0.1:{port}");

    // S3Sink reads credentials from the environment (AmazonS3Builder::from_env).
    std::env::set_var("AWS_ACCESS_KEY_ID", "minioadmin");
    std::env::set_var("AWS_SECRET_ACCESS_KEY", "minioadmin");
    std::env::set_var("AWS_DEFAULT_REGION", "us-east-1");

    let sink = Arc::new(
        S3Sink::new(S3SinkConfig {
            bucket: BUCKET.to_string(),
            endpoint: endpoint.clone(),
            region: "us-east-1".to_string(),
            allow_http: true,
            endpoint_ca_bundle: None,
            path_style: true,
        })
        .expect("build S3Sink"),
    );

    // A session transcript containing a live-looking secret, to prove scrubbing
    // happens on the real write path (not just in unit tests).
    let tmp = tempfile::tempdir().unwrap();
    let session_id = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    let file = tmp.path().join(format!("{session_id}.jsonl"));
    tokio::fs::write(
        &file,
        "{\"role\":\"user\",\"text\":\"key=AKIAIOSFODNN7EXAMPLE\"}\n{\"role\":\"assistant\"}\n",
    )
    .await
    .unwrap();
    let adapter = TempDirAdapter {
        kind: ToolKind::ClaudeCode,
        root: tmp.path().to_path_buf(),
    };

    let state = Arc::new(InMemorySyncStateStore::default());
    let engine = SyncEngine::new(
        SyncConfig {
            owner: Some("alice".into()),
            host: "e2e-host".into(),
            key_prefix: "sessions/".into(),
            ..SyncConfig::default()
        },
        sink.clone() as Arc<dyn SyncSink>,
        state.clone() as Arc<dyn SyncStateStore>,
    )
    .expect("engine");

    // First sync: real PUTs to MinIO. Retry briefly to absorb server warm-up.
    let key = {
        let mut last = None;
        let mut out = None;
        for _ in 0..15 {
            match engine.sync_file(&adapter, &file).await {
                Ok(SyncOutcome::Synced { key, .. }) => {
                    out = Some(key);
                    break;
                }
                Ok(other) => panic!("expected Synced, got {other:?}"),
                Err(e) => {
                    last = Some(e);
                    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                }
            }
        }
        out.unwrap_or_else(|| panic!("sync never succeeded: {last:?}"))
    };

    // Independent read-back client.
    let store = AmazonS3Builder::new()
        .with_bucket_name(BUCKET)
        .with_endpoint(&endpoint)
        .with_region("us-east-1")
        .with_access_key_id("minioadmin")
        .with_secret_access_key("minioadmin")
        .with_allow_http(true)
        .with_client_options(ClientOptions::default().with_allow_http(true))
        .with_virtual_hosted_style_request(false)
        .build()
        .expect("read-back store");

    // Content object: secret scrubbed, key scheme correct.
    let content = store
        .get(&object_store::path::Path::from(key.as_str()))
        .await
        .expect("get content")
        .bytes()
        .await
        .unwrap();
    let content = String::from_utf8(content.to_vec()).unwrap();
    assert!(
        !content.contains("AKIAIOSFODNN7EXAMPLE"),
        "raw secret must not reach blob storage"
    );
    assert!(
        content.contains("[REDACTED:aws_access_key]"),
        "expected redaction marker in stored content, got: {content}"
    );
    assert!(key.starts_with("sessions/alice/claude_code/"));
    assert!(key.ends_with(".jsonl"));

    // Sidecar envelope: present, well-formed, hash matches the content key.
    let meta_key = key.replace(".jsonl", ".meta.json");
    let meta = store
        .get(&object_store::path::Path::from(meta_key.as_str()))
        .await
        .expect("get meta")
        .bytes()
        .await
        .unwrap();
    let env: aikit_session_sync::Envelope = serde_json::from_slice(&meta).unwrap();
    assert_eq!(env.owner, "alice");
    assert_eq!(env.tool, "claude_code");
    assert_eq!(env.session_id, session_id);
    assert_eq!(env.host, "e2e-host");
    assert_eq!(env.schema_version, 1);
    assert!(key.contains(&env.content_hash));
    assert_eq!(env.byte_len, content.len() as u64);

    // Idempotency on the real backend: unchanged file → skip, no re-PUT.
    assert_eq!(
        engine.sync_file(&adapter, &file).await.unwrap(),
        SyncOutcome::SkippedUnchanged
    );

    // Grown file → a second, distinct version object is retained.
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    tokio::fs::write(
        &file,
        "{\"role\":\"user\",\"text\":\"key=AKIAIOSFODNN7EXAMPLE\"}\n{\"role\":\"assistant\"}\n{\"role\":\"user\",\"text\":\"more\"}\n",
    )
    .await
    .unwrap();
    let key2 = match engine.sync_file(&adapter, &file).await.unwrap() {
        SyncOutcome::Synced { key, .. } => key,
        other => panic!("expected Synced on grown file, got {other:?}"),
    };
    assert_ne!(
        key, key2,
        "grown file must produce a new content-addressed key"
    );

    let listed = store
        .list(Some(&object_store::path::Path::from(format!(
            "sessions/alice/claude_code/{session_id}"
        ))))
        .collect::<Vec<_>>()
        .await;
    let jsonl_versions = listed
        .into_iter()
        .filter_map(Result::ok)
        .filter(|m| m.location.as_ref().ends_with(".jsonl"))
        .count();
    assert_eq!(
        jsonl_versions, 2,
        "both transcript versions must be retained"
    );
}
