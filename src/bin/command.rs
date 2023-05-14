use anyhow::{Context, Result};
use s3_event_bridge::{app, client};
use std::env::var;

/// Run a command with files pulled from S3, uploading the results to
/// S3 after it exits.
#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .without_time()
        .init();
    app::init()?;
    client::init().await?;

    let bucket =
        var(&app::current().settings.bucket_var).context(&app::current().settings.bucket_var)?;
    let prefix = var(&app::current().settings.key_prefix_var)
        .context(&app::current().settings.key_prefix_var)?;
    let batch = app::EventBatch { bucket, prefix };

    app::current()
        .handle(&batch, client::current())
        .await
        .with_context(|| format!("Failed to handle batch of records {:?}", &batch))?;

    Ok(())
}
