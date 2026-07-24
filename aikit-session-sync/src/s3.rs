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
}
