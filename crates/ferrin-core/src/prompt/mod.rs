//! Prompt standardization and conversion to the provider prompt.
//!
//! - `standardize`: builder inputs (`system`, `prompt`, `messages`) become a
//!   `StandardizedPrompt`.
//! - `convert`: application messages become `ferrin_spec::Prompt`, with
//!   URL downloads and media type detection.
//! - `download`: the `DownloadFn` trait and the `DefaultDownloader`.
//! - `prepare_tools`: the tool set becomes provider tool definitions.
//! - `call_settings`: sampling settings and their validation.

mod call_settings;
pub(crate) mod convert;
pub mod download;
pub(crate) mod prepare_tools;
pub(crate) mod standardize;

pub use call_settings::CallSettings;
pub(crate) use convert::ConvertContext;
pub(crate) use convert::convert_to_prompt;
pub use download::DefaultDownloader;
pub use download::DownloadFn;
pub use download::DownloadRequest;
pub use download::DownloadedFile;
pub(crate) use prepare_tools::PrepareToolsInput;
pub(crate) use prepare_tools::prepare_tools;
pub use standardize::Instructions;
pub(crate) use standardize::standardize;
