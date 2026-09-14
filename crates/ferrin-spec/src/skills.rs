//! Provider skill upload interface.

use std::future::Future;

use bytes::Bytes;
use serde::Deserialize;
use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::error::ProviderError;
use crate::shared::Headers;
use crate::shared::ProviderId;
use crate::shared::ProviderMetadata;
use crate::shared::ProviderOptions;
use crate::shared::ProviderReference;
use crate::shared::Warning;
use crate::shared::base64_bytes;

/// Upload skills (bundles of files) to the provider.
pub trait Skills: Send + Sync + 'static {
    /// Provider identifier.
    fn provider(&self) -> &ProviderId;

    /// Uploads a skill and returns its provider reference.
    fn upload_skill(
        &self,
        options: UploadSkillOptions,
    ) -> impl Future<Output = Result<UploadSkillResult, ProviderError>> + Send;
}

/// Content of a skill file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
#[non_exhaustive]
pub enum SkillFileData {
    /// Binary data.
    Data {
        /// The bytes.
        #[serde(with = "base64_bytes")]
        data: Bytes,
    },
    /// UTF-8 text.
    Text {
        /// The text.
        text: String,
    },
}

/// A file inside a skill bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillFile {
    /// Path inside the bundle.
    pub path: String,
    /// Content.
    pub data: SkillFileData,
}

/// Options for uploading a skill.
#[derive(Debug, Clone)]
pub struct UploadSkillOptions {
    /// Files of the bundle.
    pub files: Vec<SkillFile>,
    /// Display title.
    pub display_title: Option<String>,
    /// Additional request headers.
    pub headers: Headers,
    /// Provider-specific options keyed by provider name.
    pub provider_options: ProviderOptions,
    /// Cancellation token.
    pub cancellation: CancellationToken,
}

impl UploadSkillOptions {
    /// Creates options for `files`.
    #[must_use]
    pub fn new(files: Vec<SkillFile>) -> Self {
        Self {
            files,
            display_title: None,
            headers: Headers::new(),
            provider_options: ProviderOptions::new(),
            cancellation: CancellationToken::new(),
        }
    }
}

/// Result of a skill upload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UploadSkillResult {
    /// Reference to the stored skill.
    pub provider_reference: ProviderReference,
    /// Display title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_title: Option<String>,
    /// Skill name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Skill description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Latest version identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_version: Option<String>,
    /// Provider-specific metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
    /// Warnings.
    #[serde(default)]
    pub warnings: Vec<Warning>,
}
