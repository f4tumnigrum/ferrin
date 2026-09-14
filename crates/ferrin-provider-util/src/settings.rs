//! Credential and setting loading.
//!
//! Values passed as parameters win; environment variables are the fallback.
//! This module is the only place in the workspace that reads the process
//! environment.

use ferrin_spec::error::LoadApiKeyError;
use ferrin_spec::error::LoadSettingError;
use secrecy::SecretString;

/// Reads an environment variable, treating non-UTF-8 values as absent.
#[must_use]
#[allow(
    clippy::disallowed_methods,
    reason = "the single audited environment lookup; everything else goes through this module"
)]
pub fn env_var(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// How to find an API key.
#[derive(Debug)]
pub struct ApiKeyConfig<'a> {
    /// Key passed by the application, if any.
    pub api_key: Option<SecretString>,
    /// Environment variable consulted when `api_key` is `None`.
    pub environment_variable: &'a str,
    /// Parameter name shown in error messages (for example `api_key`).
    pub parameter_name: &'a str,
    /// Provider description shown in error messages (for example `OpenAI`).
    pub description: &'a str,
}

/// Loads an API key.
///
/// # Errors
///
/// Returns [`LoadApiKeyError`] when neither the parameter nor the
/// environment variable provides a value.
pub fn load_api_key(config: ApiKeyConfig<'_>) -> Result<SecretString, LoadApiKeyError> {
    if let Some(key) = config.api_key {
        return Ok(key);
    }
    match env_var(config.environment_variable) {
        Some(value) => Ok(SecretString::from(value)),
        None => Err(LoadApiKeyError::new(format!(
            "{} API key is missing. Pass it using the '{}' parameter or the {} environment variable.",
            config.description, config.parameter_name, config.environment_variable
        ))),
    }
}

/// How to find a required setting.
#[derive(Debug)]
pub struct SettingConfig<'a> {
    /// Value passed by the application, if any.
    pub value: Option<String>,
    /// Environment variable consulted when `value` is `None`.
    pub environment_variable: &'a str,
    /// Setting name shown in error messages.
    pub setting_name: &'a str,
    /// Description shown in error messages.
    pub description: &'a str,
}

/// Loads a required setting.
///
/// # Errors
///
/// Returns [`LoadSettingError`] when neither the parameter nor the
/// environment variable provides a value.
pub fn load_setting(config: SettingConfig<'_>) -> Result<String, LoadSettingError> {
    if let Some(value) = config.value {
        return Ok(value);
    }
    env_var(config.environment_variable).ok_or_else(|| {
        LoadSettingError::new(format!(
            "{} setting is missing. Pass it using the '{}' parameter or the {} environment variable.",
            config.description, config.setting_name, config.environment_variable
        ))
    })
}

/// Loads an optional setting: the parameter, else the environment variable,
/// else `None`.
#[must_use]
pub fn load_optional_setting(value: Option<String>, environment_variable: &str) -> Option<String> {
    value.or_else(|| env_var(environment_variable))
}
