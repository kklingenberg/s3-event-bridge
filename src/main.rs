mod app;
mod client;
mod conf;
mod sign;

use anyhow::{anyhow, Context, Result};
use aws_lambda_events::event::s3::S3Event;
use aws_lambda_events::event::sqs::SqsEventObj;
use lambda_runtime::{run, service_fn, LambdaEvent};

/// Handle each S3 event record through the handler program
async fn function_handler(event: LambdaEvent<SqsEventObj<S3Event>>) -> Result<()> {
    for batch in app::current().batch_events(
        event
            .payload
            .records
            .into_iter()
            .flat_map(|record| record.body.records),
    ) {
        app::current()
            .handle(&batch, client::current())
            .await
            .with_context(|| format!("Failed to handle batch of records {:?}", &batch))?;
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
