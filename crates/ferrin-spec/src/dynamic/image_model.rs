//! Object-safe image model.

use super::BoxFuture;
use super::ModelRef;
use super::model_ref::ref_conversions;
use crate::error::ProviderError;
use crate::image_model::ImageModel;
use crate::image_model::ImageOptions;
use crate::image_model::ImageResult;
use crate::shared::ModelId;
use crate::shared::ProviderId;

/// Object-safe counterpart of [`ImageModel`].
pub trait DynImageModel: Send + Sync + 'static {
    /// See [`ImageModel::provider`].
    fn provider(&self) -> &ProviderId;
    /// See [`ImageModel::model_id`].
    fn model_id(&self) -> &ModelId;
    /// See [`ImageModel::max_images_per_call`].
    fn max_images_per_call(&self) -> Option<usize>;
    /// See [`ImageModel::do_generate`].
    fn do_generate(
        &self,
        options: ImageOptions,
    ) -> BoxFuture<'_, Result<ImageResult, ProviderError>>;
}

impl<T: ImageModel> DynImageModel for T {
    fn provider(&self) -> &ProviderId {
        ImageModel::provider(self)
    }

    fn model_id(&self) -> &ModelId {
        ImageModel::model_id(self)
    }

    fn max_images_per_call(&self) -> Option<usize> {
        ImageModel::max_images_per_call(self)
    }

    fn do_generate(
        &self,
        options: ImageOptions,
    ) -> BoxFuture<'_, Result<ImageResult, ProviderError>> {
        Box::pin(ImageModel::do_generate(self, options))
    }
}

/// Shared reference to an image model (or an unresolved model id).
pub type ImageModelRef = ModelRef<dyn DynImageModel>;

ref_conversions!(ImageModelRef, ImageModel, DynImageModel);
