//! Types shared by every model interface: identifiers, provider options and
//! metadata, warnings, headers, media types, file data and serde helpers.

mod audio_format;
pub(crate) mod base64_bytes;
mod file_data;
mod headers;
mod ids;
mod media_type;
mod provider_options;
mod provider_reference;
mod warning;

pub use audio_format::AudioFormat;
pub use file_data::FileData;
pub use headers::Headers;
pub use headers::InvalidHeader;
pub use ids::ApprovalId;
pub use ids::BatchId;
pub use ids::ModelId;
pub use ids::PartId;
pub use ids::ProviderId;
pub use ids::ToolCallId;
pub use ids::ToolName;
pub use media_type::MediaType;
pub use provider_options::ProviderMetadata;
pub use provider_options::ProviderOptions;
pub use provider_reference::ProviderReference;
pub use warning::Warning;
