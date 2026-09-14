//! Provider option key resolution.

use ferrin_openai_compatible::options_key::merged_options;
use ferrin_openai_compatible::options_key::option_keys;
use ferrin_openai_compatible::options_key::passthrough_options;
use ferrin_openai_compatible::options_key::resolve_metadata_key;
use ferrin_openai_compatible::options_key::shared_extra_fields;
use ferrin_openai_compatible::options_key::to_camel_case;
use ferrin_openai_compatible::options_key::warn_if_deprecated_key;
use ferrin_spec::ProviderOptions;
use ferrin_spec::Warning;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::options_under;

#[test]
fn camel_case_replaces_separators_before_lowercase_letters_only() {
    let cases = [
        ("my-provider", "myProvider"),
        ("my_provider", "myProvider"),
        ("already", "already"),
        ("trailing-", "trailing-"),
        ("x-1", "x-1"),
        ("a--b", "a-B"),
        ("Snake_Case-Kebab", "Snake_Case-Kebab"),
    ];
    for (input, expected) in cases {
        assert_eq!(to_camel_case(input), expected, "{input}");
    }
}

#[test]
fn keys_are_read_in_precedence_order() {
    assert_eq!(
        option_keys("my-provider"),
        vec![
            "openai-compatible",
            "openaiCompatible",
            "my-provider",
            "myProvider"
        ]
    );
    assert_eq!(
        option_keys("plain"),
        vec!["openai-compatible", "openaiCompatible", "plain"]
    );
    let mut options = options_under("openaiCompatible", json!({"user": "shared", "a": 1}));
    options.extend(options_under("my-provider", json!({"user": "raw"})));
    options.extend(options_under(
        "myProvider",
        json!({"user": "camel", "b": 2}),
    ));
    let merged = merged_options(&option_keys("my-provider"), &options).unwrap();
    assert_eq!(
        serde_json::Value::Object(merged),
        json!({"user": "camel", "a": 1, "b": 2})
    );
    assert!(merged_options(&option_keys("other"), &ProviderOptions::new()).is_none());
}

#[test]
fn metadata_key_prefers_camel_case_when_the_caller_used_it() {
    let camel = options_under("myProvider", json!({}));
    assert_eq!(resolve_metadata_key("my-provider", &camel), "myProvider");
    let raw = options_under("my-provider", json!({}));
    assert_eq!(resolve_metadata_key("my-provider", &raw), "my-provider");
    assert_eq!(
        resolve_metadata_key("my-provider", &ProviderOptions::new()),
        "my-provider"
    );
    let mut warnings = Vec::new();
    warn_if_deprecated_key("my-provider", &raw, &mut warnings);
    warn_if_deprecated_key("my-provider", &camel, &mut warnings);
    warn_if_deprecated_key("plain", &options_under("plain", json!({})), &mut warnings);
    assert_eq!(
        warnings,
        vec![Warning::deprecated(
            "providerOptions key 'my-provider'",
            "Use 'myProvider' instead."
        )]
    );
}

#[test]
fn passthrough_skips_known_keys_and_shared_fields_come_from_the_shared_key() {
    let mut options = options_under("my-provider", json!({"user": "u", "custom": 1}));
    options.extend(options_under("myProvider", json!({"other": true})));
    options.extend(options_under("openaiCompatible", json!({"ignored": true})));
    let passthrough = passthrough_options("my-provider", &options, &["user"]);
    assert_eq!(
        serde_json::Value::Object(passthrough),
        json!({"custom": 1, "other": true})
    );
    assert_eq!(
        serde_json::Value::Object(shared_extra_fields(Some(&options))),
        json!({"ignored": true})
    );
    assert!(shared_extra_fields(None).is_empty());
}
