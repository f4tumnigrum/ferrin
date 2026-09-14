//! Generic reference wrappers.

use std::sync::Arc;

/// A reference to a model: either a resolved instance or a `provider:model`
/// id that the application layer resolves through its registry.
///
/// `D` is the object-safe trait object type (`dyn DynLanguageModel`, ...).
/// Provider implementations always return resolved references; the id form
/// exists so that entry points can accept `impl Into<LanguageModelRef>` for
/// both model instances and strings.
pub struct ModelRef<D: ?Sized>(Inner<D>);

enum Inner<D: ?Sized> {
    Model(Arc<D>),
    Id(String),
}

impl<D: ?Sized> ModelRef<D> {
    /// Wraps a resolved model.
    #[must_use]
    pub fn from_arc(model: Arc<D>) -> Self {
        Self(Inner::Model(model))
    }

    /// Creates an unresolved reference by id (for example `openai:gpt-5`).
    #[must_use]
    pub fn from_id(id: impl Into<String>) -> Self {
        Self(Inner::Id(id.into()))
    }

    /// Returns the model when the reference is resolved.
    #[must_use]
    pub fn model(&self) -> Option<&Arc<D>> {
        match &self.0 {
            Inner::Model(model) => Some(model),
            Inner::Id(_) => None,
        }
    }

    /// Returns the id when the reference is unresolved.
    #[must_use]
    pub fn unresolved_id(&self) -> Option<&str> {
        match &self.0 {
            Inner::Model(_) => None,
            Inner::Id(id) => Some(id),
        }
    }

    /// Returns `true` when the reference holds a model instance.
    #[must_use]
    pub fn is_resolved(&self) -> bool {
        matches!(self.0, Inner::Model(_))
    }

    /// Unwraps the model, or returns the unresolved id as the error.
    ///
    /// # Errors
    ///
    /// Returns the id when the reference was created with [`ModelRef::from_id`].
    pub fn into_model(self) -> Result<Arc<D>, String> {
        match self.0 {
            Inner::Model(model) => Ok(model),
            Inner::Id(id) => Err(id),
        }
    }
}

impl<D: ?Sized> Clone for ModelRef<D> {
    fn clone(&self) -> Self {
        Self(match &self.0 {
            Inner::Model(model) => Inner::Model(Arc::clone(model)),
            Inner::Id(id) => Inner::Id(id.clone()),
        })
    }
}

impl<D: ?Sized> std::fmt::Debug for ModelRef<D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Inner::Model(_) => f.write_str("ModelRef::Model(..)"),
            Inner::Id(id) => f.debug_tuple("ModelRef::Id").field(id).finish(),
        }
    }
}

impl<D: ?Sized> From<Arc<D>> for ModelRef<D> {
    fn from(model: Arc<D>) -> Self {
        Self::from_arc(model)
    }
}

impl<D: ?Sized> From<String> for ModelRef<D> {
    fn from(id: String) -> Self {
        Self::from_id(id)
    }
}

impl<D: ?Sized> From<&str> for ModelRef<D> {
    fn from(id: &str) -> Self {
        Self::from_id(id)
    }
}

/// A reference to a provider service (files, skills, batch, realtime
/// factory): always resolved.
pub struct ServiceRef<D: ?Sized>(Arc<D>);

impl<D: ?Sized> ServiceRef<D> {
    /// Wraps a service instance.
    #[must_use]
    pub fn from_arc(service: Arc<D>) -> Self {
        Self(service)
    }

    /// Returns the service.
    #[must_use]
    pub fn inner(&self) -> &Arc<D> {
        &self.0
    }

    /// Unwraps the service.
    #[must_use]
    pub fn into_inner(self) -> Arc<D> {
        self.0
    }
}

impl<D: ?Sized> Clone for ServiceRef<D> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<D: ?Sized> std::fmt::Debug for ServiceRef<D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ServiceRef(..)")
    }
}

impl<D: ?Sized> std::ops::Deref for ServiceRef<D> {
    type Target = D;

    fn deref(&self) -> &D {
        &self.0
    }
}

impl<D: ?Sized> From<Arc<D>> for ServiceRef<D> {
    fn from(service: Arc<D>) -> Self {
        Self::from_arc(service)
    }
}

/// Implements `From<T>` and `From<Arc<T>>` for a reference alias.
macro_rules! ref_conversions {
    ($alias:ident, $trait_:ident, $dyn_trait:ident) => {
        impl<T: $trait_> From<T> for $alias {
            fn from(model: T) -> Self {
                Self::from_arc(::std::sync::Arc::new(model))
            }
        }

        impl<T: $trait_> From<::std::sync::Arc<T>> for $alias {
            fn from(model: ::std::sync::Arc<T>) -> Self {
                Self::from_arc(model)
            }
        }
    };
}

pub(crate) use ref_conversions;
