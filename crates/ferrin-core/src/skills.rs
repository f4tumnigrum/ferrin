//! Provider skill storage: [`upload_skill`].
//!
//! Design: `docs/01-architecture/11-other-modalities.md` §7.

use std::future::IntoFuture;

use ferrin_spec::BoxFuture;
use ferrin_spec::SkillsRef;
pub use ferrin_spec::skills::SkillFile;
pub use ferrin_spec::skills::SkillFileData;
use ferrin_spec::skills::UploadSkillOptions;
pub use ferrin_spec::skills::UploadSkillResult;
use tracing::Instrument;

use crate::error::Error;
use crate::modality::ModalityOptions;
use crate::modality::impl_modality_builder;
use crate::telemetry::ModelIdentity;
use crate::telemetry::spans;

/// Uploads a skill (a bundle of files) to the provider.
#[must_use]
pub fn upload_skill(skills: impl Into<SkillsRef>, files: Vec<SkillFile>) -> UploadSkill {
    UploadSkill {
        skills: skills.into(),
        files,
        display_title: None,
        base: ModalityOptions::default(),
    }
}

/// Builder returned by [`upload_skill`]; `.await` runs the upload.
#[derive(Debug)]
pub struct UploadSkill {
    skills: SkillsRef,
    files: Vec<SkillFile>,
    display_title: Option<String>,
    base: ModalityOptions,
}

impl UploadSkill {
    /// Sets the display title.
    #[must_use]
    pub fn display_title(mut self, display_title: impl Into<String>) -> Self {
        self.display_title = Some(display_title.into());
        self
    }
}

impl_modality_builder!(@no_retry UploadSkill);

impl IntoFuture for UploadSkill {
    type Output = Result<UploadSkillResult, Error>;
    type IntoFuture = BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let identity = ModelIdentity::new(self.skills.provider().clone(), "skills");
            let span = spans::modality_span("upload_skill", &identity);
            let skills = self.skills.clone();
            let base = self.base.clone();
            let files = self.files;
            let display_title = self.display_title;
            base.run(|base, token| {
                async move {
                    let result = skills
                        .upload_skill(UploadSkillOptions {
                            files,
                            display_title,
                            headers: base.request_headers(),
                            provider_options: base.provider_options.clone(),
                            cancellation: token,
                        })
                        .await
                        .map_err(Error::from)?;
                    spans::log_warnings(&result.warnings, &identity);
                    Ok(result)
                }
                .instrument(span)
            })
            .await
        })
    }
}
