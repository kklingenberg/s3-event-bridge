mod app;
mod client;
mod conf;
mod trigger;

use anyhow::{anyhow, Result};
use aws_lambda_events::event::s3::S3Event;
use lambda_runtime::{run, service_fn, LambdaEvent};

/// Batch the records within the S3 Event into groups of compatible
/// keys, and handle each group as a unique invocation of the handler
/// command.
async fn function_handler(event: LambdaEvent<S3Event>) -> Result<()> {
    let triggers = trigger::Trigger::from(&event.payload);
    for trigger in triggers {
        app::current().handle(&trigger, &client::current()).await?;
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
