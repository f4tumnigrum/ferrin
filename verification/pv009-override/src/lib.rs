//! PV-009: readability prototype for expressing "keep / clear / set" in
//! `prepare_call` / `prepare_step` overrides.

/// Three-state override for an optional outer setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Override<T> {
    /// Keep the outer value (the default when a field is not mentioned).
    #[default]
    Keep,
    /// Remove the outer value for this call.
    Clear,
    /// Replace the outer value.
    Set(T),
}

impl<T> Override<T> {
    pub fn apply(self, outer: Option<T>) -> Option<T> {
        match self {
            Override::Keep => outer,
            Override::Clear => None,
            Override::Set(value) => Some(value),
        }
    }
}

impl<T> From<T> for Override<T> {
    fn from(value: T) -> Self {
        Override::Set(value)
    }
}

impl<T> From<Option<T>> for Override<T> {
    /// `None` means clear: an explicit `None` drops the outer setting instead
    /// of keeping it.
    fn from(value: Option<T>) -> Self {
        value.map_or(Override::Clear, Override::Set)
    }
}

#[derive(Debug, Default)]
pub struct PreparedCall {
    pub system: Override<String>,
    pub temperature: Override<f32>,
    pub max_output_tokens: Override<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub system: Option<String>,
    pub temperature: Option<f32>,
    pub max_output_tokens: Option<u32>,
}

impl Settings {
    pub fn with(self, prepared: PreparedCall) -> Settings {
        Settings {
            system: prepared.system.apply(self.system),
            temperature: prepared.temperature.apply(self.temperature),
            max_output_tokens: prepared.max_output_tokens.apply(self.max_output_tokens),
        }
    }
}

/// Alternative B for comparison: `Option<Option<T>>`.
#[derive(Debug, Default)]
pub struct PreparedCallNested {
    pub system: Option<Option<String>>,
    pub temperature: Option<Option<f32>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_reads_naturally() {
        let outer = Settings { system: Some("be brief".into()), temperature: Some(0.7), max_output_tokens: Some(512) };
        let prepared = PreparedCall {
            system: Override::Clear,                 // drop the outer system prompt
            temperature: 0.2.into(),                 // set
            ..PreparedCall::default()                // keep max_output_tokens
        };
        assert_eq!(
            outer.with(prepared),
            Settings { system: None, temperature: Some(0.2), max_output_tokens: Some(512) }
        );
    }

    #[test]
    fn nested_option_is_ambiguous_to_read() {
        // `Some(None)` vs `None` carries the same information but the reader must
        // remember which level means what.
        let prepared = PreparedCallNested { system: Some(None), temperature: None };
        assert!(matches!(prepared.system, Some(None)));
        assert!(prepared.temperature.is_none());
    }
}
