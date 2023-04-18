//! Defines the read-only application state and hub for utility
//! functions.

use crate::client::{download, list_keys, upload};
use crate::conf::Settings;
use crate::sign::{compute_signatures, empty_signatures, find_signature_differences};
use anyhow::{anyhow, Context, Result};
use aws_lambda_events::s3::S3EventRecord;
use aws_sdk_s3::types::{Object, Owner};
use aws_smithy_types_convert::date_time::DateTimeExt;
use chrono::{DateTime, Utc};
use envy::from_env;
use once_cell::sync::OnceCell;
use regex::Regex;
use serde::Serialize;
use std::cmp::max;
use std::collections::BTreeSet;
use std::collections::VecDeque;
use std::fs;
use std::path::Path;
use tempfile::TempDir;
use tokio::process::Command;
use tokio::task::JoinSet;
use tracing::{info, instrument};

/// A batch of S3 events that share a key prefix and represent objects
/// that belong to the same bucket.
#[derive(Debug)]
pub struct EventBatch {
    pub bucket: String,
    pub prefix: String,
}

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

    /// The execution filter expression to use on pulled objects.
    pub execution_filter_expr: Option<String>,

    /// The program that needs to be executed as the handler.
    pub handler_command_program: String,

    /// The arguments passed to the executed handler program.
    pub handler_command_args: VecDeque<String>,
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
        }
        .with_context(|| {
            format!(
                "Failed to build a key matching regex from {:?}",
                &settings.match_key
            )
        })?;
        let mut pull_match_key_res = Vec::with_capacity(max(settings.pull_match_keys.len(), 1));
        for pull_match_key in &settings.pull_match_keys {
            pull_match_key_res.push(
                Regex::new(&format!(
                    "^{}$",
                    pull_match_key
                        .split('*')
                        .collect::<Vec<&str>>()
                        .join("[^/]*?")
                ))
                .with_context(|| {
                    format!(
                        "Failed to build a pull key matching regex from {:?}",
                        &pull_match_key
                    )
                })?,
            );
        }
        if pull_match_key_res.is_empty() {
            pull_match_key_res.push(Regex::new("")?)
        }
        // Compile execution filter, to catch syntax errors early
        let execution_filter_expr = match (
            &settings.execution_filter_expr.clone().unwrap_or_default(),
            &settings.execution_filter_file.clone().unwrap_or_default(),
        ) {
            (expr, filepath) if filepath.is_empty() => {
                jq_rs::compile(expr).map_err(|e| {
                    anyhow!("Failed to compile execution filter expression: {:?}", e)
                })?;
                Ok(Some(expr.clone()))
            }
            (expr, filepath) if expr.is_empty() => {
                let file_expr = fs::read_to_string(filepath).with_context(|| {
                    format!("Failed to read execution filter file: {:?}", filepath)
                })?;
                jq_rs::compile(&file_expr).map_err(|e| {
                    anyhow!(
                        "Failed to compile execution filter expression within file: {:?}",
                        e
                    )
                })?;
                Ok(Some(file_expr))
            }
            (expr, filepath) if expr.is_empty() && filepath.is_empty() => Ok(None),
            _ => Err(anyhow!(
                "Can't use both an execution filter expression and a file at the same time",
            )),
        }?;
        // Parse handler command
        let mut handler_command_args = VecDeque::from(
            shell_words::split(&settings.handler_command).with_context(|| {
                format!(
                    "Failed to shell-split handler command {:?}",
                    &settings.handler_command
                )
            })?,
        );
        let handler_command_program = handler_command_args
            .pop_front()
            .ok_or(anyhow!("empty handler command"))?;
        // Done
        Ok(App {
            settings,
            match_key_re,
            pull_match_key_res,
            execution_filter_expr,
            handler_command_program,
            handler_command_args,
        })
    }

    /// Group events according to common bucket and key prefixes.
    pub fn batch_events<I>(&self, records: I) -> Vec<EventBatch>
    where
        I: Iterator<Item = S3EventRecord>,
    {
        let mut batches = BTreeSet::new();
        for record in records {
            let processed = (|| {
                let key = record
                    .s3
                    .object
                    .key
                    .as_ref()
                    .ok_or_else(|| anyhow!("S3 event record is missing an object key"))?;
                if !self.match_key_re.is_match(key) {
                    return Err(anyhow!(
                        "S3 event record has object key {:?} \
                         that doesn't match configured pattern {:?}; ignoring",
                        key,
                        self.settings.match_key
                    ));
                }
                let bucket = record
                    .s3
                    .bucket
                    .name
                    .clone()
                    .ok_or_else(|| anyhow!("S3 event record is missing a bucket name"))?;
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
                Ok((bucket, prefix))
            })();
            if let Ok((bucket, prefix)) = processed {
                batches.insert((bucket, prefix));
            } else {
                info!("Skipped event record {:?}", processed);
            }
        }

        batches
            .into_iter()
            .map(|(bucket, prefix)| EventBatch { bucket, prefix })
            .collect()
    }

    /// Handle a batch of S3 event records.
    #[instrument(skip(self, client))]
    pub async fn handle(
        &self,
        batch: &EventBatch,
        client: &'static aws_sdk_s3::Client,
    ) -> Result<()> {
        let base_dir = TempDir::new().context("Failed to create temporary directory")?;
        let base_path = base_dir.into_path();
        info!(
            path = ?base_path,
            "Created temporary directory to hold input and output files"
        );
        let target_bucket = self
            .settings
            .target_bucket
            .clone()
            .unwrap_or_else(|| batch.bucket.clone());

        // First: list all relevant objects from S3
        info!("Listing input objects");
        let mut next = None;
        let mut pending_objects = Vec::new();
        loop {
            let (page, next_token) = list_keys(client, &batch.bucket, &batch.prefix, &next)
                .await
                .with_context(|| {
                    format!(
                        "Failed to list keys under {:?} in bucket {:?}",
                        &batch.prefix, &batch.bucket
                    )
                })?;
            for obj in page {
                if self.pull_match_key_res.iter().any(|re| {
                    if let Some(k) = obj.key() {
                        re.is_match(k)
                    } else {
                        false
                    }
                }) {
                    pending_objects.push(obj);
                }
            }
            if next_token.is_none() {
                break;
            } else {
                next = next_token;
            }
        }

        // Second: run the filter expression on all to-be-pulled objects
        if let Some(execution_filter_expr) = &self.execution_filter_expr {
            info!("Evaluating execution filter");
            let execution_filter_result = serialize_objects(&pending_objects)
                .context("Failed to serialize objects for execution filter")
                .and_then(|input| {
                    jq_rs::run(execution_filter_expr, &input)
                        .map_err(|e| anyhow!("Execution filter failed during evaluation: {:?}", e))
                })?;
            if execution_filter_result == "false\n" {
                info!(
                    "Execution filter returned 'false'; stopping before download of {:?} files",
                    pending_objects.len()
                );
                return Ok(());
            }
        }

        // Third: pull all relevant files
        info!("Downloading input objects");
        let mut joinset: JoinSet<Result<String>> = JoinSet::new();
        for obj in pending_objects {
            let bucket = batch.bucket.clone();
            let obj_key = obj.key().unwrap_or_default().to_string();
            let filename = obj_key.strip_prefix(&batch.prefix).unwrap_or(&obj_key);
            let local_path = base_path.join(filename);
            joinset.spawn(async move {
                download(client, &bucket, &obj_key, &local_path)
                    .await
                    .with_context(|| {
                        format!(
                            "Failed to download object {:?} from bucket {:?}",
                            &obj_key, &bucket
                        )
                    })?;
                Ok(obj_key)
            });
        }
        while let Some(downloaded_obj_key) = joinset.join_next().await {
            info!("Downloaded {:?}", downloaded_obj_key??);
        }

        // Fourth: compute a signature for each file pulled
        let signatures = if target_bucket == batch.bucket {
            compute_signatures(&base_path)
                .with_context(|| format!("Failed to compute signatures in {:?}", &base_path))
        } else {
            empty_signatures()
        }?;

        // Fifth: invoke the handler program
        info!(
            command = self.settings.handler_command,
            "Invoking handler command"
        );
        let status = Command::new(&self.handler_command_program)
            .args(&self.handler_command_args)
            .env(&self.settings.root_folder_var, &base_path)
            .env(&self.settings.bucket_var, &batch.bucket)
            .env(&self.settings.key_prefix_var, &batch.prefix)
            .status()
            .await
            .with_context(|| {
                format!(
                    "Failed to execute program {:?} with args {:?}",
                    &self.handler_command_program, &self.handler_command_args
                )
            })?;
        if !status.success() {
            info!(status = ?status, "Handler command was not successful");
            return Ok(());
        }

        // Sixth: upload the changed files
        let differences =
            find_signature_differences(&base_path, &signatures).with_context(|| {
                format!(
                    "Failed to compute signature differences in {:?}",
                    &base_path
                )
            })?;
        info!(
            total = differences.len(),
            "Uploading files with found differences"
        );
        let mut joinset: JoinSet<Result<String>> = JoinSet::new();
        for path in differences {
            let bucket = target_bucket.clone();
            let storage_key_path = Path::new(&batch.prefix).join(
                path.strip_prefix(&base_path).with_context(|| {
                    format!(
                        "Failed to convert local file path \
                         to bucket path for {:?} (using base path {:?})",
                        path, &base_path
                    )
                })?,
            );
            let storage_key = storage_key_path.to_string_lossy().to_string();
            joinset.spawn(async move {
                info!(key = ?storage_key, "Uploading file");
                upload(client, &bucket, &path, &storage_key)
                    .await
                    .with_context(|| format!("Failed to upload file to {:?}", &storage_key))?;
                Ok(storage_key)
            });
        }
        while let Some(uploaded_obj_key) = joinset.join_next().await {
            info!("Uploaded {:?}", uploaded_obj_key??);
        }

        // Done
        Ok(())
    }
}

/// Global App instance.
static CURRENT: OnceCell<App> = OnceCell::new();

/// Initialize the global App instance.
pub fn init() -> Result<()> {
    let settings = from_env().context("Failed to initialize settings from the environment")?;
    let app = App::new(settings).context("Failed to initialize app instance from settings")?;
    CURRENT
        .set(app)
        .map_err(|_| anyhow!("app::CURRENT was already initialized"))
}

/// Get the current App instance, or panic if it hasn't been
/// initialized.
pub fn current() -> &'static App {
    CURRENT.get().expect("app is not initialized")
}

/// Define a serde serializable version of AWS SDK object owner.
#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct SerializableOwner<'fields> {
    #[serde(skip_serializing_if = "Option::is_none")]
    display_name: Option<&'fields str>,

    #[serde(skip_serializing_if = "Option::is_none")]
    i_d: Option<&'fields str>,
}

impl<'fields> SerializableOwner<'fields> {
    /// Instantiate a serializable object owner from an AWS SDK object owner.
    pub fn from_owner(owner: &'fields Owner) -> Self {
        Self {
            display_name: owner.display_name(),
            i_d: owner.id(),
        }
    }
}

/// Define a serde serializable version of AWS SDK object.
#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct SerializableObject<'fields> {
    #[serde(skip_serializing_if = "Option::is_none")]
    checksum_algorithm: Option<Vec<&'fields str>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    e_tag: Option<&'fields str>,

    #[serde(skip_serializing_if = "Option::is_none")]
    key: Option<&'fields str>,

    #[serde(skip_serializing_if = "Option::is_none")]
    last_modified: Option<DateTime<Utc>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    owner: Option<SerializableOwner<'fields>>,

    size: i64,

    #[serde(skip_serializing_if = "Option::is_none")]
    storage_class: Option<&'fields str>,
}

impl<'fields> SerializableObject<'fields> {
    /// Instantiate a serializable object from an AWS SDK object.
    pub fn from_object(object: &'fields Object) -> Self {
        Self {
            checksum_algorithm: object
                .checksum_algorithm()
                .map(|algorithm| algorithm.iter().map(|a| a.as_str()).collect()),
            e_tag: object.e_tag(),
            key: object.key(),
            last_modified: object.last_modified().and_then(|d| d.to_chrono_utc().ok()),
            owner: object.owner().map(SerializableOwner::from_owner),
            size: object.size(),
            storage_class: object.storage_class().map(|s| s.as_str()),
        }
    }
}

/// Serializes a vector of S3 objects as an input to the execution
/// filter. Reference:
/// https://docs.aws.amazon.com/AmazonS3/latest/API/API_Object.html
fn serialize_objects(objects: &[Object]) -> Result<String> {
    let converted = objects
        .iter()
        .map(SerializableObject::from_object)
        .collect::<Vec<SerializableObject>>();
    serde_json::to_string(&converted).context("Failed serialization of S3 objects")
}
