//! Shared workspace helpers.

use anyhow::Context;
use anyhow::Result;
use cargo_metadata::Metadata;
use cargo_metadata::MetadataCommand;
use cargo_metadata::Package;

/// Workspace metadata without the dependency graph.
pub(crate) fn metadata_no_deps() -> Result<Metadata> {
    MetadataCommand::new()
        .no_deps()
        .exec()
        .context("cargo metadata failed")
}

/// Workspace metadata including the resolved dependency graph.
pub(crate) fn metadata_with_deps() -> Result<Metadata> {
    MetadataCommand::new()
        .exec()
        .context("cargo metadata failed")
}

/// Whether a workspace member is published (no `publish = false`).
pub(crate) fn is_publishable(package: &Package) -> bool {
    package
        .publish
        .as_ref()
        .is_none_or(|registries| !registries.is_empty())
}

/// A tokio runtime for the asynchronous commands.
pub(crate) fn runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("failed to start the tokio runtime")
}
