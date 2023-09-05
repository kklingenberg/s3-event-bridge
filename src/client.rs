//! Defines the global S3 client.

use crate::conf::aws_service_config;
use anyhow::{anyhow, Context, Result};
use aws_sdk_s3::{primitives::ByteStream, types::Object, Client};
use once_cell::sync::OnceCell;
use std::path::Path;
use tokio::{
    fs::{create_dir_all, File},
    io::copy,
};

/// Lists all keys found in a bucket under a given prefix. Returns a
/// page of keys and a token that can be used for a subsequent fetch.
pub async fn list_keys(
    client: &Client,
    bucket: &str,
    prefix: &str,
    next: &Option<String>,
) -> Result<(Vec<Object>, Option<String>)> {
    let mut operation = client.list_objects_v2().bucket(bucket).prefix(prefix);
    if let Some(continuation_token) = next {
        operation = operation.continuation_token(continuation_token)
    }
    let response = operation.send().await.with_context(|| {
        format!(
            "Failed to list keys under {:?} in bucket {:?} \
             using {} continuation token",
            prefix,
            bucket,
            if next.is_some() { "a" } else { "no" }
        )
    })?;
    Ok((
        response.contents().unwrap_or_default().to_vec(),
        response.next_continuation_token().map(String::from),
    ))
}

/// Downloads a single object from storage into the specified path.
pub async fn download(client: &Client, bucket: &str, key: &str, path: &Path) -> Result<()> {
    // Ensure the directory structure exists
    if let Some(parent) = path.parent() {
        create_dir_all(parent).await.with_context(|| {
            format!(
                "Failed to prepare local directory {:?} for object {:?}",
                parent, key
            )
        })?;
    }
    let mut body = client
        .get_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .with_context(|| {
            format!(
                "Failed to download object {:?} from bucket {:?}",
                key, bucket
            )
        })?
        .body
        .into_async_read();
    let mut file = File::create(path).await.with_context(|| {
        format!(
            "Failed to create local file {:?} to hold remote object {:?} from bucket {:?}",
            path, key, bucket
        )
    })?;
    copy(&mut body, &mut file).await.with_context(|| {
        format!(
            "Failed to save the contents of remote object {:?} from bucket {:?} \
             into local file {:?}",
            key, bucket, path
        )
    })?;
    Ok(())
}

/// Uploads a single object to storage.
pub async fn upload(client: &Client, bucket: &str, path: &Path, key: &str) -> Result<()> {
    let body = ByteStream::from_path(path).await.with_context(|| {
        format!(
            "Failed to load contents of local file {:?} for upload",
            path
        )
    })?;
    client
        .put_object()
        .bucket(bucket)
        .key(key)
        .body(body)
        .send()
        .await
        .with_context(|| {
            format!(
                "Failed to upload local file {:?} to remote object {:?} in bucket {:?}",
                path, key, bucket
            )
        })?;
    Ok(())
}

/// Global S3 client instance.
static CURRENT: OnceCell<Client> = OnceCell::new();

/// Initialize the global S3 client.
pub async fn init() -> Result<()> {
    let s3_config = aws_service_config().await;
    let client = Client::new(s3_config);
    CURRENT
        .set(client)
        .map_err(|_| anyhow!("client::CURRENT was already initialized"))
}

/// Get the current S3 client instance, or panic if it hasn't been initialized.
pub fn current() -> &'static Client {
    CURRENT.get().expect("client is not initialized")
}
