//! Defines the read-only application state and hub for utility
//! functions.

use crate::conf::Settings;
use crate::trigger::Trigger;
use anyhow::{anyhow, Result};
use envy::from_env;
use once_cell::sync::OnceCell;
use regex::Regex;
use std::cmp::max;
use tracing::instrument;

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

    /// Handle an invocation trigger
    #[instrument(skip(self, client))]
    pub async fn handle(&self, trigger: &Trigger, client: &aws_sdk_s3::Client) -> Result<()> {
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
