//! Object-safe video model.

use super::BoxFuture;
use super::ModelRef;
use super::model_ref::ref_conversions;
use crate::error::ProviderError;
use crate::shared::ModelId;
use crate::shared::ProviderId;
use crate::video_model::VideoModel;
use crate::video_model::VideoOptions;
use crate::video_model::VideoResult;
use crate::video_model::VideoStartOptions;
use crate::video_model::VideoStartResult;
use crate::video_model::VideoStatusOptions;
use crate::video_model::VideoStatusResult;
use crate::video_model::WebhookFactory;
use crate::video_model::WebhookHandle;

/// Object-safe counterpart of [`VideoModel`].
pub trait DynVideoModel: Send + Sync + 'static {
    /// See [`VideoModel::provider`].
    fn provider(&self) -> &ProviderId;
    /// See [`VideoModel::model_id`].
    fn model_id(&self) -> &ModelId;
    /// See [`VideoModel::max_videos_per_call`].
    fn max_videos_per_call(&self) -> Option<usize>;
    /// See [`VideoModel::supports_generate`].
    fn supports_generate(&self) -> bool;
    /// See [`VideoModel::do_generate`].
    fn do_generate(
        &self,
        options: VideoOptions,
    ) -> BoxFuture<'_, Result<VideoResult, ProviderError>>;
    /// See [`VideoModel::supports_operations`].
    fn supports_operations(&self) -> bool;
    /// See [`VideoModel::do_start`].
    fn do_start(
        &self,
        options: VideoStartOptions,
    ) -> BoxFuture<'_, Result<VideoStartResult, ProviderError>>;
    /// See [`VideoModel::do_status`].
    fn do_status(
        &self,
        options: VideoStatusOptions,
    ) -> BoxFuture<'_, Result<VideoStatusResult, ProviderError>>;
    /// See [`VideoModel::supports_webhook`].
    fn supports_webhook(&self) -> bool;
    /// See [`VideoModel::handle_webhook`].
    fn handle_webhook(
        &self,
        factory: WebhookFactory,
    ) -> BoxFuture<'_, Result<WebhookHandle, ProviderError>>;
}

impl<T: VideoModel> DynVideoModel for T {
    fn provider(&self) -> &ProviderId {
        VideoModel::provider(self)
    }

    fn model_id(&self) -> &ModelId {
        VideoModel::model_id(self)
    }

    fn max_videos_per_call(&self) -> Option<usize> {
        VideoModel::max_videos_per_call(self)
    }

    fn supports_generate(&self) -> bool {
        VideoModel::supports_generate(self)
    }

    fn do_generate(
        &self,
        options: VideoOptions,
    ) -> BoxFuture<'_, Result<VideoResult, ProviderError>> {
        Box::pin(VideoModel::do_generate(self, options))
    }

    fn supports_operations(&self) -> bool {
        VideoModel::supports_operations(self)
    }

    fn do_start(
        &self,
        options: VideoStartOptions,
    ) -> BoxFuture<'_, Result<VideoStartResult, ProviderError>> {
        Box::pin(VideoModel::do_start(self, options))
    }

    fn do_status(
        &self,
        options: VideoStatusOptions,
    ) -> BoxFuture<'_, Result<VideoStatusResult, ProviderError>> {
        Box::pin(VideoModel::do_status(self, options))
    }

    fn supports_webhook(&self) -> bool {
        VideoModel::supports_webhook(self)
    }

    fn handle_webhook(
        &self,
        factory: WebhookFactory,
    ) -> BoxFuture<'_, Result<WebhookHandle, ProviderError>> {
        Box::pin(VideoModel::handle_webhook(self, factory))
    }
}

/// Shared reference to a video model (or an unresolved model id).
pub type VideoModelRef = ModelRef<dyn DynVideoModel>;

ref_conversions!(VideoModelRef, VideoModel, DynVideoModel);
