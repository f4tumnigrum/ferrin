//! Azure transcription uses the HTTP endpoint without OpenAI realtime routing.

use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::TranscriptionModel;
use ferrin_spec::error::ProviderError;
use ferrin_spec::transcription_model::TranscriptionOptions;
use ferrin_spec::transcription_model::TranscriptionResult;

/// An Azure OpenAI HTTP transcription model.
#[derive(Debug, Clone)]
pub struct AzureTranscriptionModel(pub(crate) ferrin_openai::OpenAiTranscriptionModel);

impl TranscriptionModel for AzureTranscriptionModel {
    fn provider(&self) -> &ProviderId {
        self.0.provider()
    }
    fn model_id(&self) -> &ModelId {
        self.0.model_id()
    }
    async fn do_generate(
        &self,
        options: TranscriptionOptions,
    ) -> Result<TranscriptionResult, ProviderError> {
        self.0.do_generate(options).await
    }
}
