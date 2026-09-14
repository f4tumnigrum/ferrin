//! Object-safe skills service.

use super::BoxFuture;
use super::ServiceRef;
use super::model_ref::ref_conversions;
use crate::error::ProviderError;
use crate::shared::ProviderId;
use crate::skills::Skills;
use crate::skills::UploadSkillOptions;
use crate::skills::UploadSkillResult;

/// Object-safe counterpart of [`Skills`].
pub trait DynSkills: Send + Sync + 'static {
    /// See [`Skills::provider`].
    fn provider(&self) -> &ProviderId;
    /// See [`Skills::upload_skill`].
    fn upload_skill(
        &self,
        options: UploadSkillOptions,
    ) -> BoxFuture<'_, Result<UploadSkillResult, ProviderError>>;
}

impl<T: Skills> DynSkills for T {
    fn provider(&self) -> &ProviderId {
        Skills::provider(self)
    }

    fn upload_skill(
        &self,
        options: UploadSkillOptions,
    ) -> BoxFuture<'_, Result<UploadSkillResult, ProviderError>> {
        Box::pin(Skills::upload_skill(self, options))
    }
}

/// Shared reference to a skills service.
pub type SkillsRef = ServiceRef<dyn DynSkills>;

ref_conversions!(SkillsRef, Skills, DynSkills);
