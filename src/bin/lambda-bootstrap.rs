use anyhow::{anyhow, Context, Result};
use aws_lambda_events::event::s3::S3Event;
use aws_lambda_events::event::sqs::SqsEventObj;
use lambda_runtime::{run, service_fn, LambdaEvent};
use s3_event_bridge::{app, client};

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

/// Run an AWS Lambda function that listens to SQS events containing
/// batches of S3 events, that executes a handler command with files
/// pulled from S3 according to the input events, and that uploads the
/// command's results to S3 after it exits.
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
