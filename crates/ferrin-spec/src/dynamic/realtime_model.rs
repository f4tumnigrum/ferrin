//! Object-safe realtime model and factory.

use url::Url;

use super::BoxFuture;
use super::ModelRef;
use super::ServiceRef;
use super::model_ref::ref_conversions;
use crate::error::NoSuchModelError;
use crate::error::ProviderError;
use crate::json::JsonValue;
use crate::realtime_model::ClientSecret;
use crate::realtime_model::ClientSecretOptions;
use crate::realtime_model::GetTokenOptions;
use crate::realtime_model::RealtimeClientEvent;
use crate::realtime_model::RealtimeFactory;
use crate::realtime_model::RealtimeModel;
use crate::realtime_model::RealtimeServerEvent;
use crate::realtime_model::RealtimeSessionConfig;
use crate::realtime_model::WebSocketConfig;
use crate::shared::ModelId;
use crate::shared::ProviderId;

/// Object-safe counterpart of [`RealtimeModel`].
pub trait DynRealtimeModel: Send + Sync + 'static {
    /// See [`RealtimeModel::provider`].
    fn provider(&self) -> &ProviderId;
    /// See [`RealtimeModel::model_id`].
    fn model_id(&self) -> &ModelId;
    /// See [`RealtimeModel::do_create_client_secret`].
    fn do_create_client_secret(
        &self,
        options: ClientSecretOptions,
    ) -> BoxFuture<'_, Result<ClientSecret, ProviderError>>;
    /// See [`RealtimeModel::websocket_config`].
    fn websocket_config(&self, token: &str, url: &Url) -> WebSocketConfig;
    /// See [`RealtimeModel::parse_server_event`].
    fn parse_server_event(&self, raw: JsonValue)
    -> Result<Vec<RealtimeServerEvent>, ProviderError>;
    /// See [`RealtimeModel::serialize_client_event`].
    fn serialize_client_event(
        &self,
        event: RealtimeClientEvent,
    ) -> BoxFuture<'_, Result<JsonValue, ProviderError>>;
    /// See [`RealtimeModel::build_session_config`].
    fn build_session_config(
        &self,
        config: &RealtimeSessionConfig,
    ) -> Result<JsonValue, ProviderError>;
    /// See [`RealtimeModel::health_check_response`].
    fn health_check_response(&self, raw: &JsonValue) -> Option<JsonValue>;
}

impl<T: RealtimeModel> DynRealtimeModel for T {
    fn provider(&self) -> &ProviderId {
        RealtimeModel::provider(self)
    }

    fn model_id(&self) -> &ModelId {
        RealtimeModel::model_id(self)
    }

    fn do_create_client_secret(
        &self,
        options: ClientSecretOptions,
    ) -> BoxFuture<'_, Result<ClientSecret, ProviderError>> {
        Box::pin(RealtimeModel::do_create_client_secret(self, options))
    }

    fn websocket_config(&self, token: &str, url: &Url) -> WebSocketConfig {
        RealtimeModel::websocket_config(self, token, url)
    }

    fn parse_server_event(
        &self,
        raw: JsonValue,
    ) -> Result<Vec<RealtimeServerEvent>, ProviderError> {
        RealtimeModel::parse_server_event(self, raw)
    }

    fn serialize_client_event(
        &self,
        event: RealtimeClientEvent,
    ) -> BoxFuture<'_, Result<JsonValue, ProviderError>> {
        Box::pin(RealtimeModel::serialize_client_event(self, event))
    }

    fn build_session_config(
        &self,
        config: &RealtimeSessionConfig,
    ) -> Result<JsonValue, ProviderError> {
        RealtimeModel::build_session_config(self, config)
    }

    fn health_check_response(&self, raw: &JsonValue) -> Option<JsonValue> {
        RealtimeModel::health_check_response(self, raw)
    }
}

/// Shared reference to a realtime model (or an unresolved model id).
pub type RealtimeModelRef = ModelRef<dyn DynRealtimeModel>;

ref_conversions!(RealtimeModelRef, RealtimeModel, DynRealtimeModel);

/// Object-safe counterpart of [`RealtimeFactory`].
pub trait DynRealtimeFactory: Send + Sync + 'static {
    /// See [`RealtimeFactory::provider`].
    fn provider(&self) -> &ProviderId;
    /// See [`RealtimeFactory::model`].
    fn model(&self, model_id: &str) -> Result<RealtimeModelRef, NoSuchModelError>;
    /// See [`RealtimeFactory::get_token`].
    fn get_token(
        &self,
        options: GetTokenOptions,
    ) -> BoxFuture<'_, Result<ClientSecret, ProviderError>>;
}

impl<T: RealtimeFactory> DynRealtimeFactory for T {
    fn provider(&self) -> &ProviderId {
        RealtimeFactory::provider(self)
    }

    fn model(&self, model_id: &str) -> Result<RealtimeModelRef, NoSuchModelError> {
        RealtimeFactory::model(self, model_id)
    }

    fn get_token(
        &self,
        options: GetTokenOptions,
    ) -> BoxFuture<'_, Result<ClientSecret, ProviderError>> {
        Box::pin(RealtimeFactory::get_token(self, options))
    }
}

/// Shared reference to a realtime factory.
pub type RealtimeFactoryRef = ServiceRef<dyn DynRealtimeFactory>;

ref_conversions!(RealtimeFactoryRef, RealtimeFactory, DynRealtimeFactory);
