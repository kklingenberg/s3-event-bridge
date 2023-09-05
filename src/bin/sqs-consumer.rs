use anyhow::{Context, Result};
use aws_sdk_sqs::{types::DeleteMessageBatchRequestEntry, Client};
use core::time::Duration;
use s3_event_bridge::{app, client, conf};
use std::env::var;
use tokio::time::sleep;
use tracing::{info, instrument, warn};

/// The minimum time to wait between ticks, in milliseconds.
const BASE_LAPSE_TIME: u64 = 300;

/// The base of the exponential backoff sequence.
const BACKOFF_BASE: u64 = 2;

/// The maximum amount of milliseconds to sleep between ticks. Set to
/// the equivalent of 20 minutes.
const MAX_SLEEP: u64 = 1200000;

/// Wrapper structure that executes successive SQS consumption cycles:
/// receive messages, parse their contents, assemble event batches,
/// invoke the handler, and finally delete messages.
struct SQSConsumer {
    queue_url: String,
    visibility_timeout: i32,
    max_number_of_messages: i32,
    client: Client,
    backoff: u32,
}

impl SQSConsumer {
    /// Record a success and wait a little while.
    async fn pass(&mut self) {
        self.backoff = 0;
        sleep(Duration::from_millis(BASE_LAPSE_TIME)).await;
    }

    /// Record a failure and wait a while.
    async fn fail(&mut self) {
        sleep(Duration::from_millis(std::cmp::min(
            BASE_LAPSE_TIME.saturating_mul(BACKOFF_BASE.saturating_pow(self.backoff)),
            MAX_SLEEP,
        )))
        .await;
        self.backoff = self.backoff.saturating_add(1);
    }

    /// Perform a single pass of the SQS consumption cycle.
    #[instrument(skip(self))]
    pub async fn tick(&mut self) -> () {
        let command_result = self
            .client
            .receive_message()
            .queue_url(&self.queue_url)
            .visibility_timeout(self.visibility_timeout)
            .max_number_of_messages(self.max_number_of_messages)
            .wait_time_seconds(30)
            .send()
            .await;
        if let Err(e) = command_result {
            warn!("Error while consuming messages from SQS queue: {:?}", e);
            return self.fail().await;
        }

        let result = command_result.unwrap();
        let messages = result.messages().unwrap_or_default();
        let mut handling_error = None;

        for batch in app::current().batch_events(
            messages
                .iter()
                .filter_map(|message| message.body())
                .filter_map(|body| {
                    serde_json::from_str(body)
                        .map_err(|e| {
                            warn!("Couldn't parse the body of SQS message: {:?}", e);
                            e
                        })
                        .ok()
                }),
        ) {
            let handle_result = app::current().handle(&batch, client::current()).await;
            if let Err(e) = handle_result {
                handling_error = Some(e);
            }
        }
        if let Some(e) = handling_error {
            warn!(
                "Error encountered while handling events; SQS messages won't be deleted: {:?}",
                e
            );
            return self.pass().await;
        }
        if messages.is_empty() {
            return self.pass().await;
        }
        info!("Deleting SQS messages");
        let delete_command_result = self
            .client
            .delete_message_batch()
            .queue_url(&self.queue_url)
            .set_entries(Some(
                messages
                    .iter()
                    .map(|message| {
                        DeleteMessageBatchRequestEntry::builder()
                            .set_id(message.message_id().map(String::from))
                            .set_receipt_handle(message.receipt_handle().map(String::from))
                            .build()
                    })
                    .collect(),
            ))
            .send()
            .await;
        if let Err(e) = delete_command_result {
            warn!("Couldn't delete SQS messages: {:?}", e);
            return self.fail().await;
        }
        let result = delete_command_result.unwrap();
        if !result.failed().unwrap_or_default().is_empty() {
            let failed = result.failed().unwrap_or_default().len();
            let total = messages.len();
            warn!(
                "Couldn't delete some SQS messages: {:?} out of {:?} weren't deleted",
                failed, total
            );
        }
        self.pass().await;
    }
}

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

    let queue_url = var("SQS_QUEUE_URL").context("SQS_QUEUE_URL is required")?;
    let visibility_timeout = var("SQS_VISIBILITY_TIMEOUT")
        .unwrap_or(String::from("30"))
        .parse::<i32>()
        .context("SQS_VISIBILITY_TIMEOUT must be a number")?;
    let max_number_of_messages = var("SQS_MAX_NUMBER_OF_MESSAGES")
        .unwrap_or(String::from("1"))
        .parse::<i32>()
        .context("SQS_MAX_NUMBER_OF_MESSAGES must be a number")?;
    let sqs_client = Client::new(conf::aws_service_config().await);

    let mut consumer = SQSConsumer {
        queue_url,
        visibility_timeout,
        max_number_of_messages,
        client: sqs_client,
        backoff: 0,
    };

    // Listen for abort signals
    let (stop_processing, mut should_stop) = tokio::sync::oneshot::channel();
    tokio::spawn(async {
        tokio::signal::ctrl_c().await.unwrap();
        info!("CTRL-C");
        stop_processing.send(()).unwrap();
    });

    // Continuosly receive messages and execute the corresponing
    // handler for each one
    loop {
        tokio::select! {
            _ = consumer.tick() => (),
            _ = &mut should_stop => break
        }
    }
    Ok(())
}
