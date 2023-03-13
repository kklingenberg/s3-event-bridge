mod app;
mod client;
mod conf;
mod sign;

use anyhow::{anyhow, Result};
use aws_lambda_events::event::s3::S3Event;
use lambda_runtime::{run, service_fn, LambdaEvent};

/// Handle each S3 event record through the handler program
async fn function_handler(event: LambdaEvent<S3Event>) -> Result<()> {
    for record in event.payload.records {
        app::current().handle(&record, client::current()).await?;
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .without_time()
        .init();
    app::init()?;
    client::init().await?;

    run(service_fn(function_handler))
        .await
        .map_err(|e| anyhow!("{:?}", e))
}
