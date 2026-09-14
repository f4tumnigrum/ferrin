//! Batch result types and the conversion of provider items.

use ferrin_spec::BoxStream;
use ferrin_spec::FinishReason;
use ferrin_spec::ProviderMetadata;
use ferrin_spec::RequestMetadata;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::Usage;
use ferrin_spec::Warning;
use ferrin_spec::batch::BatchError;
use ferrin_spec::batch::BatchItem;
use ferrin_spec::batch::BatchItemResult;
use ferrin_spec::image_model::ImageUsage;
use ferrin_tool::ToolSet;

use crate::error::Error;
use crate::generate_text::RefineToolInputs;
use crate::generate_text::StepContent;
use crate::generate_text::parse_tool_call::ParseContext;
use crate::generate_text::run::convert_content;
use crate::image::GeneratedImage;
use crate::image::convert_images;

/// Successful text item of a batch: the shape of one `generate_text` step.
#[derive(Debug, Clone, PartialEq)]
pub struct TextBatchResult {
    /// Content parts (tool calls parsed against the tools given to
    /// [`GetBatchResults::tools`](super::GetBatchResults::tools)).
    pub content: Vec<StepContent>,
    /// Finish reason.
    pub finish_reason: FinishReason,
    /// Token usage.
    pub usage: Usage,
    /// Adapter warnings.
    pub warnings: Vec<Warning>,
    /// Request metadata.
    pub request: RequestMetadata,
    /// Response metadata.
    pub response: ResponseMetadata,
    /// Provider metadata.
    pub provider_metadata: Option<ProviderMetadata>,
}

impl TextBatchResult {
    /// Concatenated text parts.
    #[must_use]
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(|part| match part {
                StepContent::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }
}

/// Successful image item of a batch.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageBatchResult {
    /// Generated images.
    pub images: Vec<GeneratedImage>,
    /// Adapter warnings.
    pub warnings: Vec<Warning>,
    /// Response metadata.
    pub response: ResponseMetadata,
    /// Provider metadata.
    pub provider_metadata: Option<ProviderMetadata>,
    /// Usage.
    pub usage: Option<ImageUsage>,
}

/// One item of [`get_batch_results`](super::get_batch_results).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum BatchResultItem {
    /// A text request.
    Text(Box<BatchItem<TextBatchResult>>),
    /// An image request.
    Image(Box<BatchItem<ImageBatchResult>>),
}

impl BatchResultItem {
    /// The request id.
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Text(item) => item.id(),
            Self::Image(item) => item.id(),
        }
    }
}

/// The stream returned by [`get_batch_results`](super::get_batch_results).
pub type BatchResults = BoxStream<'static, Result<BatchResultItem, Error>>;

pub(super) fn unsupported_status<R>(id: String) -> BatchItem<R> {
    BatchItem::Failed {
        id,
        error: BatchError {
            message: "unsupported batch item status".to_owned(),
            error_type: None,
            code: None,
            status_code: None,
        },
        provider_metadata: None,
    }
}

pub(super) async fn convert_item(item: BatchItemResult, tools: &ToolSet) -> BatchResultItem {
    let item_id = item.id().to_owned();
    match item {
        BatchItemResult::Text(item) => BatchResultItem::Text(Box::new(match *item {
            BatchItem::Succeeded { id, result } => {
                let refine = RefineToolInputs::default();
                let parse_ctx = ParseContext {
                    tools,
                    tool_choice: None,
                    repair: None,
                    refine: &refine,
                    system: None,
                    messages: &[],
                };
                let (content, _calls) = convert_content(&result.content, &parse_ctx, tools).await;
                BatchItem::Succeeded {
                    id,
                    result: TextBatchResult {
                        content,
                        finish_reason: result.finish_reason,
                        usage: result.usage,
                        warnings: result.warnings,
                        request: result.request,
                        response: result.response,
                        provider_metadata: result.provider_metadata,
                    },
                }
            }
            BatchItem::Failed {
                id,
                error,
                provider_metadata,
            } => BatchItem::Failed {
                id,
                error,
                provider_metadata,
            },
            BatchItem::Cancelled {
                id,
                error,
                provider_metadata,
            } => BatchItem::Cancelled {
                id,
                error,
                provider_metadata,
            },
            BatchItem::Expired {
                id,
                error,
                provider_metadata,
            } => BatchItem::Expired {
                id,
                error,
                provider_metadata,
            },
            #[allow(unreachable_patterns, reason = "BatchItem is non-exhaustive")]
            _ => unsupported_status(item_id),
        })),
        BatchItemResult::Image(item) => BatchResultItem::Image(Box::new(match *item {
            BatchItem::Succeeded { id, result } => BatchItem::Succeeded {
                id,
                result: ImageBatchResult {
                    images: convert_images(&result),
                    warnings: result.warnings,
                    response: result.response,
                    provider_metadata: result.provider_metadata,
                    usage: result.usage,
                },
            },
            BatchItem::Failed {
                id,
                error,
                provider_metadata,
            } => BatchItem::Failed {
                id,
                error,
                provider_metadata,
            },
            BatchItem::Cancelled {
                id,
                error,
                provider_metadata,
            } => BatchItem::Cancelled {
                id,
                error,
                provider_metadata,
            },
            BatchItem::Expired {
                id,
                error,
                provider_metadata,
            } => BatchItem::Expired {
                id,
                error,
                provider_metadata,
            },
            #[allow(unreachable_patterns, reason = "BatchItem is non-exhaustive")]
            _ => unsupported_status(item_id),
        })),
        #[allow(unreachable_patterns, reason = "BatchItemResult is non-exhaustive")]
        _ => BatchResultItem::Text(Box::new(unsupported_status(item_id))),
    }
}
