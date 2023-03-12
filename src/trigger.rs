//! Defines a _trigger_, the input for the handling of an event
//! through the handler command. The trigger is built from the S3
//! event.

use aws_lambda_events::event::s3::S3Event;
use tracing::instrument;

/// The input to the handler command.
#[derive(Debug)]
pub struct Trigger {}

impl Trigger {
    /// Builds a vector of triggers from the records inside an S3
    /// event.
    #[instrument]
    pub fn from(event: &S3Event) -> Vec<Self> {
        Vec::new()
    }
}
