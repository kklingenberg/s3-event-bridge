//! Defines the global S3 client.

use anyhow::{anyhow, Result};
use aws_config::from_env;
use aws_sdk_s3::Client;
use once_cell::sync::OnceCell;
use std::env;

/// Global S3 client instance.
static CURRENT: OnceCell<Client> = OnceCell::new();

/// Initialize the global S3 client.
pub async fn init() -> Result<()> {
    let endpoint_url_var = env::var("AWS_ENDPOINT_URL");
    let s3_config = if let Ok(endpoint_url) = endpoint_url_var {
        from_env()
            .endpoint_url(
                if endpoint_url.starts_with("http://") || endpoint_url.starts_with("https://") {
                    endpoint_url
                } else {
                    format!("https://{}", endpoint_url)
                },
            )
            .region("us-east-1") // should be OK since the endpoint was overridden
            .load()
    } else {
        from_env().load()
    }
    .await;
    let client = Client::new(&s3_config);
    CURRENT
        .set(client)
        .map_err(|_| anyhow!("client::CURRENT was already initialized"))
}

/// Get the current S3 client instance, or panic if it hasn't been initialized.
pub fn current() -> &'static Client {
    CURRENT.get().expect("client is not initialized")
}
