//! Filters logical modality events before dispatching integrations.

use crate::embed::EmbedCallEndEvent;
use crate::embed::EmbedCallStartEvent;
use crate::embed::EmbeddingResponse;
use crate::rerank::RerankCallEndEvent;
use crate::rerank::RerankCallStartEvent;
use crate::telemetry::dispatcher::TelemetryDispatcher;
use crate::telemetry::redact;

impl TelemetryDispatcher {
    pub(crate) async fn on_embed_operation_start(&self, event: &EmbedCallStartEvent) {
        if !self.options.enabled {
            return;
        }
        let mut event = event.clone();
        if !self.options.include_runtime_context {
            event.runtime_context = None;
        }
        if !self.record_inputs() {
            event.value = None;
            event.headers = Default::default();
            event.provider_options = Default::default();
        }
        self.dispatch(|integration| integration.on_embed_operation_start(&event))
            .await;
    }

    pub(crate) async fn on_embed_operation_end(&self, event: &EmbedCallEndEvent) {
        if !self.options.enabled {
            return;
        }
        let mut event = event.clone();
        if !self.options.include_runtime_context {
            event.runtime_context = None;
        }
        if !self.record_inputs() {
            event.value = None;
        }
        if !self.record_outputs() {
            event.embedding = None;
            event.provider_metadata = None;
            match &mut event.response {
                EmbeddingResponse::Single(response) => response.body = None,
                EmbeddingResponse::Many(responses) => {
                    for response in responses {
                        response.body = None;
                    }
                }
            }
        }
        if !(self.record_inputs() && self.record_outputs()) {
            event.warnings = redact::warnings(&event.warnings);
        }
        self.dispatch(|integration| integration.on_embed_operation_end(&event))
            .await;
    }

    pub(crate) async fn on_rerank_operation_start(&self, event: &RerankCallStartEvent) {
        if !self.options.enabled {
            return;
        }
        let mut event = event.clone();
        if !self.options.include_runtime_context {
            event.runtime_context = None;
        }
        if !self.record_inputs() {
            event.documents = None;
            event.query = None;
            event.headers = Default::default();
            event.provider_options = Default::default();
        }
        self.dispatch(|integration| integration.on_rerank_operation_start(&event))
            .await;
    }

    pub(crate) async fn on_rerank_operation_end(&self, event: &RerankCallEndEvent) {
        if !self.options.enabled {
            return;
        }
        let mut event = event.clone();
        if !self.options.include_runtime_context {
            event.runtime_context = None;
        }
        if !self.record_inputs() {
            event.documents = None;
            event.query = None;
        }
        // Ranked documents contain original inputs as well as output scores.
        if !(self.record_inputs() && self.record_outputs()) {
            event.ranking = None;
            event.warnings = redact::warnings(&event.warnings);
        }
        if !self.record_outputs() {
            event.provider_metadata = None;
            event.response.body = None;
        }
        self.dispatch(|integration| integration.on_rerank_operation_end(&event))
            .await;
    }
}
