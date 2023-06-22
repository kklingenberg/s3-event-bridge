//! Defines configuration as read from the environment.

use once_cell::sync::OnceCell;
use serde::Deserialize;
use std::env;

/// Default `root_folder_var` value.
fn default_root_folder_var() -> String {
    String::from("ROOT_FOLDER")
}

/// Default `bucket_var` value.
fn default_bucket_var() -> String {
    String::from("BUCKET")
}

/// Default `key_prefix_var` value.
fn default_key_prefix_var() -> String {
    String::from("KEY_PREFIX")
}

/// The event bridge is configured to pull files from S3, execute a
/// command, and push resulting files to S3. The configuration must be
/// given as environment variables.
#[derive(Deserialize)]
pub struct Settings {
    /// Defines a filter to select only matching keys. Use regexes to
    /// match object keys. Omitting this will make it match any file.
    #[serde(default)]
    pub match_key: Option<String>,

    /// Defines the folder to pull from S3 given an event key. It
    /// counts the parent directories from the key, where `0` means
    /// the containing folder. If given a value greater than the
    /// amount of folders, the whole bucket will also be pulled. A
    /// negative value also is interpreted as pulling the whole
    /// bucket.
    #[serde(default)]
    pub pull_parent_dirs: i32,

    /// Defines filters to pull only matching keys, after having
    /// selected a folder to pull. This limits the files pulled to
    /// those matching any of the expressions. If omitted or empty, it
    /// will pull all the selected files from S3. If used, the user
    /// should add the value given to `match_key` unless they
    /// explicitly want to use a wider expression, or exclude the
    /// triggering key from the pull.
    #[serde(default)]
    pub pull_match_keys: Vec<String>,

    /// Defines a jq expression to run against the set of objects to
    /// be pulled which, if defined and returning `false`, will skip
    /// execution.
    #[serde(default)]
    pub execution_filter_expr: Option<String>,

    /// Defines a file containing a jq expression to run against the
    /// set of objects to be pulled which, if defined and returning
    /// `false`, will skip execution.
    #[serde(default)]
    pub execution_filter_file: Option<String>,

    /// Defines a bucket to receive the outputs. If omitted, it will
    /// be the same bucket as the one in the triggering event.
    #[serde(default)]
    pub target_bucket: Option<String>,

    /// The environment variable populated with the temporary folder
    /// pulled from S3, to be passed to the handler command.
    #[serde(default = "default_root_folder_var")]
    pub root_folder_var: String,

    /// The environment variable populated with the bucket name from
    /// which files are pulled, to be passed to the handler command.
    #[serde(default = "default_bucket_var")]
    pub bucket_var: String,

    /// The environment variable populated with the object key prefix
    /// used to pull files from S3, to be passed to the handler
    /// command.
    #[serde(default = "default_key_prefix_var")]
    pub key_prefix_var: String,
}

/// Global AWS configuration instance.
static CURRENT_AWS_CONFIG: OnceCell<aws_config::SdkConfig> = OnceCell::new();

/// Returns the singleton AWS configuration to use with service
/// clients.
pub async fn aws_service_config() -> &'static aws_config::SdkConfig {
    if let Some(config) = CURRENT_AWS_CONFIG.get() {
        config
    } else {
        let endpoint_url_var = env::var("AWS_ENDPOINT_URL");
        let config = if let Ok(endpoint_url) = endpoint_url_var {
            aws_config::from_env()
                .endpoint_url(
                    if endpoint_url.starts_with("http://") || endpoint_url.starts_with("https://") {
                        endpoint_url
                    } else {
                        format!("https://{}", endpoint_url)
                    },
                )
                .region("us-east-1") // should be OK since the endpoint was overridden
                .load()
        } else {
            aws_config::from_env().load()
        }
        .await;
        CURRENT_AWS_CONFIG
            .set(config)
            .expect("Concurrent configuration of AWS services");
        CURRENT_AWS_CONFIG.get().unwrap()
    }
}
