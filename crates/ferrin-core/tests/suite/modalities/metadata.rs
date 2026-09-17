use std::sync::Arc;

use bytes::Bytes;
use ferrin_core::embed_many;
use ferrin_core::generate_image;
use ferrin_spec::EmbeddingModel;
use ferrin_spec::ImageModel;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::embedding_model::EmbedOptions;
use ferrin_spec::embedding_model::EmbedResult;
use ferrin_spec::embedding_model::EmbeddingUsage;
use ferrin_spec::error::ProviderError;
use ferrin_spec::image_model::GeneratedImage;
use ferrin_spec::image_model::ImageOptions;
use ferrin_spec::image_model::ImageResult;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::EmbedMock;
use super::common::PNG_BYTES;

struct MetadataModel {
    provider: ProviderId,
    model: ModelId,
}

impl MetadataModel {
    fn new() -> Self {
        Self {
            provider: ProviderId::new("mock"),
            model: ModelId::new("metadata"),
        }
    }
}

impl EmbeddingModel for MetadataModel {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }
    fn model_id(&self) -> &ModelId {
        &self.model
    }
    fn max_embeddings_per_call(&self) -> Option<usize> {
        Some(1)
    }
    fn supports_parallel_calls(&self) -> bool {
        true
    }
    async fn do_embed(&self, options: EmbedOptions) -> Result<EmbedResult, ProviderError> {
        let value = options.values.first().unwrap();
        Ok(EmbedResult {
            embeddings: vec![vec![1.0]; options.values.len()],
            usage: Some(EmbeddingUsage { tokens: 1 }),
            provider_metadata: Some(
                serde_json::from_value(json!({
                    "mock": {"labels":[value], "nested":{"last":value}},
                    value: {"retained": true}
                }))
                .unwrap(),
            ),
            response: ResponseMetadata::default(),
            warnings: Vec::new(),
        })
    }
}

impl ImageModel for MetadataModel {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }
    fn model_id(&self) -> &ModelId {
        &self.model
    }
    fn max_images_per_call(&self) -> Option<usize> {
        Some(2)
    }
    async fn do_generate(&self, options: ImageOptions) -> Result<ImageResult, ProviderError> {
        let image_metadata: Vec<_> = (0..options.n)
            .map(|index| json!({"batch":options.n,"index":index}))
            .collect();
        let cost = if options.n == 2 {
            "999999999999999999999.1"
        } else {
            "0.02"
        };
        Ok(ImageResult {
            images: (0..options.n)
                .map(|_| GeneratedImage {
                    data: Bytes::from_static(PNG_BYTES),
                    media_type: None,
                })
                .collect(),
            is_retryable: None,
            warnings: Vec::new(),
            provider_metadata: Some(
                serde_json::from_value(json!({
                    "mock":{"images":image_metadata},
                    "gateway":{"cost":cost,"marketCost":"0.0001","images":[],"last":options.n}
                }))
                .unwrap(),
            ),
            response: ResponseMetadata::default(),
            usage: None,
        })
    }
}

#[tokio::test]
async fn embedding_metadata_replaces_arrays_in_input_chunk_order() {
    let result = embed_many(Arc::new(MetadataModel::new()), ["first", "last"])
        .await
        .unwrap();
    assert_eq!(
        result.provider_metadata,
        Some(
            serde_json::from_value(json!({
                "mock":{"labels":["last"],"nested":{"last":"last"}},
                "first":{"retained":true}, "last":{"retained":true}
            }))
            .unwrap()
        )
    );
}

#[tokio::test]
async fn empty_embedding_input_keeps_unlimited_single_call_behavior() {
    for bounded in [false, true] {
        let mut model = EmbedMock::new();
        model.max_per_call = bounded.then_some(2);
        let model = Arc::new(model);
        let result = embed_many(Arc::clone(&model), Vec::<String>::new())
            .await
            .unwrap();
        assert_eq!(
            (
                model.calls(),
                result.embeddings,
                result.responses.len(),
                result.usage.tokens
            ),
            (
                if bounded { vec![] } else { vec![vec![]] },
                vec![],
                usize::from(!bounded),
                Some(0)
            ),
        );
    }
}

#[tokio::test]
async fn image_metadata_concatenates_images_and_sums_decimal_gateway_costs() {
    let result = generate_image(Arc::new(MetadataModel::new()), "three images")
        .n(3)
        .await
        .unwrap();
    assert_eq!(
        result.provider_metadata,
        serde_json::from_value(json!({
            "mock":{"images":[{"batch":2,"index":0},{"batch":2,"index":1},{"batch":1,"index":0}]},
            "gateway":{"cost":"999999999999999999999.12","marketCost":"0.0002","last":1}
        }))
        .unwrap()
    );
    assert_eq!(
        result
            .images
            .iter()
            .map(|image| image.provider_metadata.clone())
            .collect::<Vec<_>>(),
        vec![
            Some(serde_json::from_value(json!({"mock":{"batch":2,"index":0}})).unwrap()),
            Some(serde_json::from_value(json!({"mock":{"batch":2,"index":1}})).unwrap()),
            Some(serde_json::from_value(json!({"mock":{"batch":1,"index":0}})).unwrap()),
        ]
    );
}
