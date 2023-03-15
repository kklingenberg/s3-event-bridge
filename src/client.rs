//! Defines the global S3 client.

use anyhow::{anyhow, Result};
use aws_config::from_env;
use aws_sdk_s3::types::ByteStream;
use aws_sdk_s3::Client;
use once_cell::sync::OnceCell;
use std::env;
use std::path::Path;
use tokio::fs::File;
use tokio::io::copy;

/// Lists all keys found in a bucket under a given prefix. Returns a
/// page of keys and a token that can be used for a subsequent fetch.
pub async fn list_keys(
    client: &Client,
    bucket: &str,
    prefix: &str,
    next: &Option<String>,
) -> Result<(Vec<String>, Option<String>)> {
    let mut operation = client.list_objects_v2().bucket(bucket).prefix(prefix);
    if let Some(continuation_token) = next {
        operation = operation.continuation_token(continuation_token)
    }
    let response = operation.send().await?;
    Ok((
        response
            .contents()
            .unwrap_or_default()
            .iter()
            .filter_map(|o| o.key().map(String::from))
            .collect(),
        response.next_continuation_token().map(String::from),
    ))
}

/// Downloads a single object from storage into the specified path.
pub async fn download(client: &Client, bucket: &str, key: &str, path: &Path) -> Result<()> {
    let mut body = client
        .get_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await?
        .body
        .into_async_read();
    let mut file = File::create(path).await?;
    copy(&mut body, &mut file).await?;
    Ok(())
}

/// Uploads a single object to storage.
pub async fn upload(client: &Client, bucket: &str, path: &Path, key: &str) -> Result<()> {
    let body = ByteStream::from_path(path).await?;
    client
        .put_object()
        .bucket(bucket)
        .key(key)
        .body(body)
        .send()
        .await?;
    Ok(())
}

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