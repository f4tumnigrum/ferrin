//! PV-001: can `dynosaur` 0.3.1 generate an object-safe adapter for an RPITIT
//! trait whose methods return `impl Future + Send`, usable as
//! `Arc<DynLanguageModel<'static>>` across tasks?

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use futures_core::Stream;

pub type BoxStream<'a, T> = Pin<Box<dyn Stream<Item = T> + Send + 'a>>;

#[derive(Debug, Clone, PartialEq)]
pub struct CallOptions {
    pub prompt: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GenerateResult {
    pub text: String,
}

#[derive(Debug)]
pub struct ProviderError(pub String);

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ProviderError {}

/// Variant A: RPITIT with explicit `+ Send` on every future. This is the
/// shape mandated by docs/03-engineering/03-coding-standards.md §4.
#[dynosaur::dynosaur(pub DynLanguageModel = dyn(box) LanguageModel, bridge(dyn))]
pub trait LanguageModel: Send + Sync {
    fn provider(&self) -> &str;
    fn model_id(&self) -> &str;
    fn generate(
        &self,
        options: CallOptions,
    ) -> impl Future<Output = Result<GenerateResult, ProviderError>> + Send;
    fn stream(
        &self,
        options: CallOptions,
    ) -> impl Future<Output = Result<BoxStream<'static, String>, ProviderError>> + Send;
}

#[derive(Debug)]
pub struct EchoModel;

impl LanguageModel for EchoModel {
    fn provider(&self) -> &str {
        "echo"
    }
    fn model_id(&self) -> &str {
        "echo-1"
    }
    async fn generate(&self, options: CallOptions) -> Result<GenerateResult, ProviderError> {
        tokio::task::yield_now().await;
        Ok(GenerateResult { text: options.prompt })
    }
    async fn stream(&self, options: CallOptions) -> Result<BoxStream<'static, String>, ProviderError> {
        let words: Vec<String> = options.prompt.split(' ').map(str::to_owned).collect();
        Ok(Box::pin(futures_util::stream::iter(words)))
    }
}

/// The adapter type the core would hold.
pub type SharedModel = Arc<DynLanguageModel<'static>>;

pub fn share(model: impl LanguageModel + 'static) -> SharedModel {
    DynLanguageModel::new_arc(model)
}

fn assert_send_sync<T: Send + Sync>() {}
fn assert_static<T: 'static>() {}

pub fn static_checks() {
    assert_send_sync::<SharedModel>();
    assert_static::<SharedModel>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;

    #[tokio::test]
    async fn dyn_model_is_usable_across_tasks() {
        let model: SharedModel = share(EchoModel);
        let mut set = tokio::task::JoinSet::new();
        for i in 0..8 {
            let model = Arc::clone(&model);
            // Requires the boxed future returned by `generate` to be `Send`.
            set.spawn(async move {
                let result = model
                    .generate(CallOptions { prompt: format!("hello {i}") })
                    .await
                    .unwrap();
                result.text
            });
        }
        let mut texts = Vec::new();
        while let Some(text) = set.join_next().await {
            texts.push(text.unwrap());
        }
        texts.sort();
        assert_eq!(texts.len(), 8);
        assert_eq!(texts[0], "hello 0");
    }

    #[tokio::test]
    async fn dyn_model_streams() {
        let model: SharedModel = share(EchoModel);
        let stream = model.stream(CallOptions { prompt: "a b c".into() }).await.unwrap();
        let parts: Vec<String> = stream.collect().await;
        assert_eq!(parts, vec!["a", "b", "c"]);
        assert_eq!(model.provider(), "echo");
    }

    #[test]
    fn generic_call_still_static_dispatch() {
        // `DynLanguageModel<'_>` is unsized, so generic code needs `?Sized` to accept it.
        fn takes_generic<M: LanguageModel + ?Sized>(m: &M) -> &str {
            m.model_id()
        }
        assert_eq!(takes_generic(&EchoModel), "echo-1");
        // The Dyn type itself implements the trait (bridge), so generic code
        // accepts it too.
        let shared = share(EchoModel);
        assert_eq!(takes_generic(&*shared), "echo-1");
    }
}
