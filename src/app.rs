//! Defines the read-only application state and hub for utility
//! functions.

use crate::client::{download, list_keys, upload};
use crate::conf::Settings;
use crate::sign::{compute_signatures, find_signature_differences};
use anyhow::{anyhow, Result};
use aws_lambda_events::s3::S3EventRecord;
use envy::from_env;
use once_cell::sync::OnceCell;
use regex::Regex;
use std::cmp::max;
use tempfile::TempDir;
use tokio::process::Command;
use tracing::{info, instrument};

/// An App is an initialized application state, derived from
/// settings. This is only useful to pre-compute stuff that will be
/// used constantly.
pub struct App {
    /// The original settings.
    pub settings: Settings,

    /// The regex that matches S3 event keys.
    pub match_key_re: Regex,

    /// The regexes that match files to be pulled.
    pub pull_match_key_res: Vec<Regex>,

    /// The program that needs to be executed as the handler.
    pub handler_command_program: String,

    /// The arguments passed to the executed handler program.
    pub handler_command_args: Vec<String>,
}

impl App {
    /// Initialize an App instance given a settings struct. Consumes
    /// the settings struct.
    pub fn new(settings: Settings) -> Result<Self> {
        // Parse regexes
        let match_key_re = if let Some(match_key) = &settings.match_key {
            Regex::new(&format!(
                "^{}$",
                match_key.split('*').collect::<Vec<&str>>().join("[^/]*?")
            ))
        } else {
            Regex::new("")
        }?;
        let mut pull_match_key_res = Vec::with_capacity(max(settings.pull_match_keys.len(), 1));
        for pull_match_key in &settings.pull_match_keys {
            pull_match_key_res.push(Regex::new(&format!(
                "^{}$",
                pull_match_key
                    .split('*')
                    .collect::<Vec<&str>>()
                    .join("[^/]*?")
            ))?);
        }
        if pull_match_key_res.is_empty() {
            pull_match_key_res.push(Regex::new("")?)
        }
        // Parse handler command
        let mut handler_command_args = shell_words::split(&settings.handler_command)?;
        let handler_command_program = handler_command_args
            .pop()
            .ok_or(anyhow::Error::msg("empty handler command"))?;
        // Done
        Ok(App {
            settings,
            match_key_re,
            pull_match_key_res,
            handler_command_program,
            handler_command_args,
        })
    }

    /// Handle an S3 event record.
    #[instrument(skip(self, client))]
    pub async fn handle(&self, record: &S3EventRecord, client: &aws_sdk_s3::Client) -> Result<()> {
        let base_dir = TempDir::new()?;
        let base_path = base_dir.into_path();
        info!(
            path = ?base_path,
            "Created temporary directory to hold input and output files"
        );

        let key = record
            .s3
            .object
            .key
            .as_ref()
            .ok_or_else(|| anyhow!("S3 event record is missing an object key"))?;
        if !self.match_key_re.is_match(key) {
            info!(
                key,
                pattern = self.settings.match_key,
                "S3 event record has object key that doesn't match configured pattern; ignoring"
            );
            return Ok(());
        }
        let bucket = record
            .s3
            .bucket
            .name
            .as_ref()
            .ok_or_else(|| anyhow!("S3 event record is missing a bucket name"))?;
        let target_bucket = self
            .settings
            .target_bucket
            .clone()
            .unwrap_or_else(|| bucket.clone());
        let prefix = if self.settings.pull_parent_dirs < 0 {
            String::from("")
        } else {
            let mut prefix_parts = key
                .split('/')
                .rev()
                .skip((self.settings.pull_parent_dirs + 1).try_into()?)
                .collect::<Vec<&str>>()
                .into_iter()
                .rev()
                .collect::<Vec<&str>>()
                .join("/");
            if !prefix_parts.is_empty() {
                prefix_parts.push('/');
            }
            prefix_parts
        };

        // First: pull all relevant files from S3, and compute a
        // signature for each file pulled
        info!("Downloading input files");
        let mut next = None;
        loop {
            let (page, next_token) = list_keys(client, bucket, &prefix, &next).await?;
            for page_key in page {
                if self
                    .pull_match_key_res
                    .iter()
                    .any(|re| re.is_match(&page_key))
                {
                    let filename = page_key.strip_prefix(&prefix).unwrap_or(&page_key);
                    let local_path = base_path.join(filename);
                    download(client, bucket, &page_key, &local_path).await?;
                }
            }
            if next_token.is_none() {
                break;
            } else {
                next = next_token;
            }
        }
        let signatures = compute_signatures(&base_path)?;

        // Second: invoke the handler program
        info!(
            command = self.settings.handler_command,
            "Invoking handler command"
        );
        let status = Command::new(&self.handler_command_program)
            .args(&self.handler_command_args)
            .env(&self.settings.root_folder_var, &base_path)
            .status()
            .await?;
        if !status.success() {
            info!(status = ?status, "Handler command was not successful");
            return Ok(());
        }

        // Third: upload the changed files
        let differences = find_signature_differences(&base_path, &signatures)?;
        info!(
            total = differences.len(),
            "Uploading files with found differences"
        );
        for path in differences {
            let storage_key = path
                .strip_prefix(&base_path)?
                .to_str()
                .ok_or_else(|| anyhow!("couldn't translate file path to object key: {:?}", path))?;
            info!(key = ?storage_key, "Uploading file");
            upload(client, &target_bucket, &path, storage_key).await?;
        }

        // Done
        Ok(())
    }
}

/// Global App instance.
static CURRENT: OnceCell<App> = OnceCell::new();

/// Initialize the global App instance.
pub fn init() -> Result<()> {
    let settings = from_env()?;
    let app = App::new(settings)?;
    CURRENT
        .set(app)
        .map_err(|_| anyhow!("app::CURRENT was already initialized"))
}

/// Get the current App instance, or panic if it hasn't been
/// initialized.
pub fn current() -> &'static App {
    CURRENT.get().expect("app is not initialized")
}
