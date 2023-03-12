//! Defines configuration as read from the environment.

use serde::Deserialize;

/// Default `pull_parent_dirs` value.
fn default_pull_parent_dirs() -> i32 {
    1
}

/// Default `root_folder_var` value.
fn default_root_folder_var() -> String {
    String::from("ROOT_FOLDER")
}

/// The event bridge is configured to pull files from S3, execute a
/// command, and push resulting files to S3. The configuration must be
/// given as environment variables.
#[derive(Deserialize)]
pub struct Settings {
    /// Defines a filter to select only matching keys. The star (*)
    /// can be used as a wildcard matching any number of non-slash
    /// characters. E.g. to match any file in a folder, use
    /// `"folder/*"`. Omitting this will make it match any file.
    #[serde(default)]
    pub match_key: Option<String>,

    /// Defines the folder to pull from S3 given an event key. It
    /// counts the parent directories from the key, where `0` means
    /// the containing folder. If given a value greater than the
    /// amount of folders, the whole bucket will also be pulled. A
    /// negative value also is interpreted as pulling the whole
    /// bucket.
    #[serde(default = "default_pull_parent_dirs")]
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

    /// Defines a bucket to receive the outputs. If omitted, it will
    /// be the same bucket as the one in the triggering event.
    #[serde(default)]
    pub target_bucket: Option<String>,

    /// Defines the command that will be executed.
    pub handler_command: String,

    /// The environment variable populated with the temporary folder
    /// pulled from S3, to give the handler command.
    #[serde(default = "default_root_folder_var")]
    pub root_folder_var: String,
}
