//! PV-005: baseline rustc diagnostics when a tool closure captures a
//! non-`'static` reference. The `#[ferrin::tool]` macro will lower to the same
//! bounds as `Tool::function`, so the error text below is what users see.

use std::future::Future;
use std::pin::Pin;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub struct Tool {
    pub execute: Box<dyn Fn(String) -> BoxFuture<'static, String> + Send + Sync + 'static>,
}

impl Tool {
    /// Same bounds as the designed `Tool::function::<I>()`.
    pub fn function<F, Fut>(f: F) -> Tool
    where
        F: Fn(String) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = String> + Send + 'static,
    {
        Tool { execute: Box::new(move |input| Box::pin(f(input))) }
    }
}

/// Second design candidate: the macro generates a free function that must be
/// `'static`-callable; a helper asserts it at definition time so the error is
/// anchored on the function item instead of on a closure body.
pub fn assert_tool_fn<F, Fut>(_f: F)
where
    F: Fn(String) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = String> + Send + 'static,
{
}
