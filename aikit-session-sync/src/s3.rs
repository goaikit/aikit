use object_store::aws::AmazonS3Builder;
use object_store::path::Path;
use object_store::{Certificate, ClientOptions, ObjectStoreExt};

use crate::sink::meta_key;
use crate::{SyncError, SyncObject, SyncSink};

#[derive(Debug, Clone)]
pub struct S3SinkConfig {
    pub bucket: String,
    pub endpoint: String,
    pub region: String,
    pub allow_http: bool,
    pub endpoint_ca_bundle: Option<std::path::PathBuf>,
    pub path_style: bool,
}

pub struct S3Sink {
    store: object_store::aws::AmazonS3,
}

/// Env vars whose presence signals a non-static AWS credential source is
/// configured (shared-config profile, ECS/EKS container creds, or
/// web-identity/IRSA). Any one of these means the default provider chain has a
/// real source to draw from, so the preflight lets construction proceed.
const CONFIGURED_PROVIDER_ENVS: &[&str] = &[
    "AWS_PROFILE",
    "AWS_DEFAULT_PROFILE",
    "AWS_CONTAINER_CREDENTIALS_RELATIVE_URI",
    "AWS_CONTAINER_CREDENTIALS_FULL_URI",
    "AWS_WEB_IDENTITY_TOKEN_FILE",
];

/// Opt-in escape hatch: set truthy to allow the EC2/ECS instance-metadata
/// (IMDS) credential provider when no explicit source is configured.
const ALLOW_INSTANCE_ENV: &str = "AIKIT_SYNC_ALLOW_INSTANCE_CREDENTIALS";

fn env_present(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| !v.is_empty())
}

fn parse_bool_flag(raw: &str) -> bool {
    matches!(raw.trim(), "1" | "true" | "TRUE" | "yes")
}

fn instance_credentials_opt_in() -> bool {
    std::env::var_os(ALLOW_INSTANCE_ENV)
        .map(|v| parse_bool_flag(&v.to_string_lossy()))
        .unwrap_or(false)
}

/// Fail fast when no AWS credential source is discoverable.
///
/// Without this, a missing-key misconfiguration falls through to the default
/// provider chain's *instance* provider, which hammers the IMDS endpoint
/// (`169.254.169.254`) — on a host with no metadata service (Hetzner, bare
/// metal, most laptops) that is a ~19s retry loop *per file* ending in an
/// opaque "error sending request", instead of one clear line up front.
fn credentials_preflight() -> Result<(), SyncError> {
    evaluate_credentials(
        env_present("AWS_ACCESS_KEY_ID"),
        env_present("AWS_SECRET_ACCESS_KEY"),
        CONFIGURED_PROVIDER_ENVS.iter().any(|n| env_present(n)),
        instance_credentials_opt_in(),
    )
}

/// Pure decision behind [`credentials_preflight`], split out so every branch is
/// testable without mutating process-global environment variables.
fn evaluate_credentials(
    have_key_id: bool,
    have_secret: bool,
    other_provider: bool,
    allow_instance: bool,
) -> Result<(), SyncError> {
    match (have_key_id, have_secret) {
        (true, true) => return Ok(()),
        (true, false) => {
            return Err(SyncError::Auth(
                "AWS_ACCESS_KEY_ID is set but AWS_SECRET_ACCESS_KEY is missing".to_string(),
            ))
        }
        (false, true) => {
            return Err(SyncError::Auth(
                "AWS_SECRET_ACCESS_KEY is set but AWS_ACCESS_KEY_ID is missing".to_string(),
            ))
        }
        (false, false) => {}
    }
    if other_provider || allow_instance {
        return Ok(());
    }
    Err(SyncError::Auth(
        "no AWS credentials found in the environment. Set AWS_ACCESS_KEY_ID and \
         AWS_SECRET_ACCESS_KEY (the S3 access key/secret for your bucket). To use an \
         instance/role provider instead (e.g. EC2/ECS IMDS), set \
         AIKIT_SYNC_ALLOW_INSTANCE_CREDENTIALS=1 to opt in."
            .to_string(),
    ))
}

impl S3Sink {
    pub fn new(config: S3SinkConfig) -> Result<Self, SyncError> {
        let mut client_options = ClientOptions::default().with_allow_http(config.allow_http);
        if let Some(path) = &config.endpoint_ca_bundle {
            let pem = std::fs::read(path)?;
            for cert in Certificate::from_pem_bundle(&pem)
                .map_err(|e| SyncError::Backend(format!("invalid CA bundle: {e}")))?
            {
                client_options = client_options.with_root_certificate(cert);
            }
        }
        // Surface a missing/partial credential setup as a clear auth error
        // before the builder falls through to the IMDS retry loop.
        credentials_preflight()?;
        let store = AmazonS3Builder::from_env()
            .with_bucket_name(config.bucket)
            .with_endpoint(config.endpoint)
            .with_region(config.region)
            .with_allow_http(config.allow_http)
            .with_virtual_hosted_style_request(!config.path_style)
            .with_client_options(client_options)
            .build()
            .map_err(|e| SyncError::Backend(e.to_string()))?;
        Ok(Self { store })
    }
}

#[async_trait::async_trait]
impl SyncSink for S3Sink {
    async fn put(&self, object: SyncObject) -> Result<(), SyncError> {
        let content_key = Path::from(object.key.as_str());
        self.store
            .put(&content_key, object.content.into())
            .await
            .map_err(|e| SyncError::Backend(e.to_string()))?;

        let meta = serde_json::to_vec(&object.envelope)
            .map_err(|e| SyncError::Backend(format!("serialize envelope: {e}")))?;
        let meta_key = Path::from(meta_key(&object.key).as_str());
        self.store
            .put(&meta_key, meta.into())
            .await
            .map_err(|e| SyncError::Backend(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_cfg() -> S3SinkConfig {
        S3SinkConfig {
            bucket: "b".into(),
            endpoint: "http://127.0.0.1:9".into(),
            region: "us-east-1".into(),
            allow_http: true,
            endpoint_ca_bundle: None,
            path_style: true,
        }
    }

    #[test]
    fn builds_with_valid_config() {
        // Client construction is lazy: no network happens until a request.
        std::env::set_var("AWS_ACCESS_KEY_ID", "x");
        std::env::set_var("AWS_SECRET_ACCESS_KEY", "y");
        assert!(S3Sink::new(base_cfg()).is_ok());
    }

    #[test]
    fn missing_ca_bundle_is_io_error() {
        let mut cfg = base_cfg();
        cfg.endpoint_ca_bundle = Some(std::path::PathBuf::from("/no/such/ca-bundle.pem"));
        assert!(matches!(S3Sink::new(cfg), Err(SyncError::Io(_))));
    }

    #[test]
    fn valid_ca_bundle_builds() {
        // Covers the custom-CA success path: a well-formed PEM bundle is parsed
        // and each cert added as a client root. The cert is a throwaway
        // self-signed fixture; from_pem_bundle validates structure, not trust.
        std::env::set_var("AWS_ACCESS_KEY_ID", "x");
        std::env::set_var("AWS_SECRET_ACCESS_KEY", "y");
        let tmp = tempfile::tempdir().unwrap();
        let bundle = tmp.path().join("ca.pem");
        std::fs::write(&bundle, include_str!("testdata/test-ca.pem")).unwrap();
        let mut cfg = base_cfg();
        cfg.endpoint_ca_bundle = Some(bundle);
        assert!(S3Sink::new(cfg).is_ok());
    }

    #[test]
    fn malformed_ca_bundle_is_backend_error() {
        let tmp = tempfile::tempdir().unwrap();
        let bundle = tmp.path().join("bad.pem");
        std::fs::write(
            &bundle,
            b"-----BEGIN CERTIFICATE-----\nnot base64!!\n-----END CERTIFICATE-----\n",
        )
        .unwrap();
        let mut cfg = base_cfg();
        cfg.endpoint_ca_bundle = Some(bundle);
        assert!(matches!(S3Sink::new(cfg), Err(SyncError::Backend(_))));
    }

    // --- credential preflight (pure decision; no env mutation) --------------

    #[test]
    fn creds_static_pair_ok() {
        assert!(evaluate_credentials(true, true, false, false).is_ok());
    }

    #[test]
    fn creds_key_id_without_secret_is_auth() {
        let e = evaluate_credentials(true, false, false, false).unwrap_err();
        assert_eq!(e.kind(), "auth");
        assert!(e.to_string().contains("AWS_SECRET_ACCESS_KEY is missing"));
    }

    #[test]
    fn creds_secret_without_key_id_is_auth() {
        let e = evaluate_credentials(false, true, false, false).unwrap_err();
        assert_eq!(e.kind(), "auth");
        assert!(e.to_string().contains("AWS_ACCESS_KEY_ID is missing"));
    }

    #[test]
    fn creds_other_provider_ok() {
        // A profile / container / web-identity source is configured.
        assert!(evaluate_credentials(false, false, true, false).is_ok());
    }

    #[test]
    fn creds_instance_opt_in_ok() {
        // No explicit source, but the user opted into IMDS.
        assert!(evaluate_credentials(false, false, false, true).is_ok());
    }

    #[test]
    fn creds_none_is_auth_with_actionable_message() {
        let e = evaluate_credentials(false, false, false, false).unwrap_err();
        assert_eq!(e.kind(), "auth");
        let msg = e.to_string();
        assert!(msg.contains("no AWS credentials"));
        assert!(msg.contains("AWS_ACCESS_KEY_ID"));
        assert!(msg.contains("AIKIT_SYNC_ALLOW_INSTANCE_CREDENTIALS"));
    }

    #[test]
    fn parse_bool_flag_accepts_truthy_only() {
        for truthy in ["1", "true", "TRUE", "yes", "  yes  "] {
            assert!(parse_bool_flag(truthy), "{truthy:?} should be truthy");
        }
        for falsy in ["0", "false", "no", "", "  "] {
            assert!(!parse_bool_flag(falsy), "{falsy:?} should be falsy");
        }
    }
}
