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
