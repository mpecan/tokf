use aws_sdk_s3::Client;
use aws_sdk_s3::config::{Credentials, Region};
use aws_sdk_s3::primitives::ByteStream;

use crate::config::Config;

use super::StorageClient;

/// Real Cloudflare R2 (S3-compatible) storage client.
pub struct R2StorageClient {
    client: Client,
    bucket: String,
}

impl std::fmt::Debug for R2StorageClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("R2StorageClient")
            .field("bucket", &self.bucket)
            .finish_non_exhaustive()
    }
}

impl R2StorageClient {
    /// Build an R2 storage client from application config.
    ///
    /// Requires `r2_bucket_name`, `r2_access_key_id`, `r2_secret_access_key`, and
    /// either `r2_endpoint` or `r2_account_id` to be set.
    ///
    /// # Errors
    ///
    /// Returns an error if required R2 config fields are missing.
    pub fn new(config: &Config) -> anyhow::Result<Self> {
        let bucket = config
            .r2_bucket_name
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("R2_BUCKET_NAME is required for R2 storage"))?
            .to_string();

        let endpoint_url = config
            .r2_endpoint_url()
            .ok_or_else(|| anyhow::anyhow!("R2_ENDPOINT or R2_ACCOUNT_ID is required"))?;

        let access_key_id = config
            .r2_access_key_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("R2_ACCESS_KEY_ID is required for R2 storage"))?;

        let secret_access_key = config
            .r2_secret_access_key
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("R2_SECRET_ACCESS_KEY is required for R2 storage"))?;

        let credentials =
            Credentials::new(access_key_id, secret_access_key, None, None, "tokf-server");

        let s3_config = aws_sdk_s3::Config::builder()
            .region(Region::new("auto"))
            .endpoint_url(&endpoint_url)
            .credentials_provider(credentials)
            .force_path_style(true)
            .behavior_version_latest()
            .build();

        let client = Client::from_conf(s3_config);

        Ok(Self { client, bucket })
    }
}

#[async_trait::async_trait]
impl StorageClient for R2StorageClient {
    async fn put(&self, key: &str, body: Vec<u8>) -> anyhow::Result<String> {
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(ByteStream::from(body))
            .send()
            .await?;
        Ok(key.to_string())
    }

    async fn get(&self, key: &str) -> anyhow::Result<Option<Vec<u8>>> {
        match self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
        {
            Ok(output) => {
                let bytes = output.body.collect().await?.to_vec();
                Ok(Some(bytes))
            }
            Err(err) => {
                if err
                    .as_service_error()
                    .is_some_and(aws_sdk_s3::operation::get_object::GetObjectError::is_no_such_key)
                {
                    return Ok(None);
                }
                Err(err.into())
            }
        }
    }

    async fn delete(&self, key: &str) -> anyhow::Result<()> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await?;
        Ok(())
    }

    async fn exists(&self, key: &str) -> anyhow::Result<bool> {
        match self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
        {
            Ok(_) => Ok(true),
            Err(err) => {
                if err
                    .as_service_error()
                    .is_some_and(aws_sdk_s3::operation::head_object::HeadObjectError::is_not_found)
                {
                    return Ok(false);
                }
                Err(err.into())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn full_config() -> Config {
        Config {
            port: 8080,
            database_url: None,
            migration_database_url: None,
            run_migrations: true,
            trust_proxy: false,
            r2_bucket_name: Some("test-bucket".to_string()),
            r2_access_key_id: Some("AKID".to_string()),
            r2_secret_access_key: Some("secret".to_string()),
            r2_endpoint: Some("https://r2.example.com".to_string()),
            r2_account_id: None,
            github_client_id: None,
            github_client_secret: None,
            public_url: "http://localhost:8080".to_string(),
            rate_limits: crate::config::RateLimitConfig::default(),
        }
    }

    #[test]
    fn new_succeeds_with_full_config() {
        let cfg = full_config();
        assert!(R2StorageClient::new(&cfg).is_ok());
    }

    #[test]
    fn new_fails_without_bucket() {
        let mut cfg = full_config();
        cfg.r2_bucket_name = None;
        let err = R2StorageClient::new(&cfg).unwrap_err();
        assert!(err.to_string().contains("R2_BUCKET_NAME"));
    }

    #[test]
    fn new_fails_without_endpoint_or_account_id() {
        let mut cfg = full_config();
        cfg.r2_endpoint = None;
        cfg.r2_account_id = None;
        let err = R2StorageClient::new(&cfg).unwrap_err();
        assert!(err.to_string().contains("R2_ENDPOINT or R2_ACCOUNT_ID"));
    }

    #[test]
    fn new_fails_without_access_key_id() {
        let mut cfg = full_config();
        cfg.r2_access_key_id = None;
        let err = R2StorageClient::new(&cfg).unwrap_err();
        assert!(err.to_string().contains("R2_ACCESS_KEY_ID"));
    }

    #[test]
    fn new_fails_without_secret_access_key() {
        let mut cfg = full_config();
        cfg.r2_secret_access_key = None;
        let err = R2StorageClient::new(&cfg).unwrap_err();
        assert!(err.to_string().contains("R2_SECRET_ACCESS_KEY"));
    }

    #[test]
    fn new_succeeds_with_account_id_instead_of_endpoint() {
        let mut cfg = full_config();
        cfg.r2_endpoint = None;
        cfg.r2_account_id = Some("myaccount".to_string());
        assert!(R2StorageClient::new(&cfg).is_ok());
    }
}
