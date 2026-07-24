use aikit_session_capture::{SecretScrubber, SCRUBBER_PATTERN_VERSION};
use aikit_session_sync::{
    decode_session_id_from_key, object_key, Envelope, InMemorySink, SyncError, SyncObject, SyncSink,
};
use bytes::Bytes;

fn envelope() -> Envelope {
    Envelope {
        schema_version: 1,
        owner: "owner".into(),
        tool: "codex".into(),
        session_id: "session".into(),
        source_file: "/tmp/session.jsonl".into(),
        host: "host".into(),
        captured_at_ms: 1,
        content_hash: "hash".into(),
        byte_len: 4,
        scrubber_version: SCRUBBER_PATTERN_VERSION,
        sync_tool_version: "0.1.0".into(),
    }
}

#[test]
fn scrub_golden_vectors_have_exact_labels() {
    let scrubber = SecretScrubber::default();
    let cases = [
        ("aws=AKIAIOSFODNN7EXAMPLE", "aws=[REDACTED:aws_access_key]"),
        (
            "pat=ghp_0123456789012345678901234567890abcdefgh",
            "pat=[REDACTED:github_pat]",
        ),
        (
            "jwt=eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIx.eyJpc3MiOiJ4",
            "jwt=[REDACTED:jwt]",
        ),
        (
            "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEA...\n-----END RSA PRIVATE KEY-----",
            "[REDACTED:private_key]",
        ),
        (
            "postgres://user:secretpass@host:5432/db",
            "[REDACTED:connstring_password]host:5432/db",
        ),
    ];

    for (input, expected) in cases {
        assert_eq!(scrubber.scrub(input), expected);
    }
}

#[test]
fn percent_encoded_key_round_trip_includes_adversarial_ids() {
    for session_id in ["/", "a/b", "space id", "unicode-雪", "%already"] {
        let key = object_key(
            "sessions/",
            "owner",
            aikit_session_capture::ToolKind::ClaudeCode,
            session_id,
            "00ff",
        );
        assert_eq!(decode_session_id_from_key(&key).unwrap(), session_id);
        assert!(!key.contains("//"));
    }
}

#[tokio::test]
async fn in_memory_sink_is_idempotent_for_same_content_key() {
    let sink = InMemorySink::new();
    let object = SyncObject {
        key: "sessions/owner/codex/session/hash.jsonl".into(),
        content: Bytes::from_static(b"body"),
        envelope: envelope(),
    };
    sink.put(object.clone()).await.unwrap();
    sink.put(object).await.unwrap();
    assert_eq!(sink.object_count(), 1);
    assert_eq!(sink.meta_count(), 1);
    assert_eq!(sink.put_calls(), 2);
}

// Real MinIO round-trip lives in `tests/e2e_minio.rs` (testcontainers-backed).
// The backend-enforced cross-owner-prefix rejection still depends on the spec
// §13 owner-prefix IAM/bucket policy, which is an infra follow-up.

#[tokio::test]
async fn sync_error_is_non_exhaustive_shape() {
    let err = SyncError::Backend("backend down".into());
    assert_eq!(err.to_string(), "backend: backend down");
}
