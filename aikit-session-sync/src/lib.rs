//! `aikit-session-sync`: raw scrubbed transcript sync for JSONL session files.

pub mod engine;
pub mod key;
pub mod s3;
pub mod sink;
pub mod state;

pub use engine::{
    credential_owner_from_env, resolve_owner, OutputFormat, SyncConfig, SyncEngine, SyncOutcome,
    SyncRunSummary, WatchRetryPolicy,
};
pub use key::{
    decode_session_id_from_key, object_key, percent_decode_segment, percent_encode_segment,
};
pub use s3::{S3Sink, S3SinkConfig};
pub use sink::{Envelope, InMemorySink, SyncError, SyncObject, SyncSink};
pub use state::{FileFingerprint, JsonSyncStateStore, SyncStateEntry, SyncStateStore};
