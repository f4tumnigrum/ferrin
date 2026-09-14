use ferrin_provider_util::load_api_key;
use ferrin_provider_util::load_optional_setting;
use ferrin_provider_util::load_setting;
use ferrin_provider_util::settings::ApiKeyConfig;
use ferrin_provider_util::settings::SettingConfig;
use pretty_assertions::assert_eq;
use secrecy::ExposeSecret;
use secrecy::SecretString;

#[test]
fn parameter_wins_over_environment() {
    let key = load_api_key(ApiKeyConfig {
        api_key: Some(SecretString::from("sk-param")),
        environment_variable: "FERRIN_TEST_KEY_UNSET",
        parameter_name: "api_key",
        description: "Test",
    })
    .unwrap();
    assert_eq!(key.expose_secret(), "sk-param");
}

#[test]
fn missing_key_reports_both_sources() {
    let error = load_api_key(ApiKeyConfig {
        api_key: None,
        environment_variable: "FERRIN_TEST_KEY_UNSET",
        parameter_name: "api_key",
        description: "Test",
    })
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Test API key is missing. Pass it using the 'api_key' parameter or the FERRIN_TEST_KEY_UNSET environment variable."
    );
}

#[test]
fn settings_fall_back_to_none() {
    assert_eq!(
        load_optional_setting(None, "FERRIN_TEST_SETTING_UNSET"),
        None
    );
    assert_eq!(
        load_optional_setting(Some("x".to_owned()), "FERRIN_TEST_SETTING_UNSET"),
        Some("x".to_owned())
    );
    let error = load_setting(SettingConfig {
        value: None,
        environment_variable: "FERRIN_TEST_SETTING_UNSET",
        setting_name: "base_url",
        description: "Base URL",
    })
    .unwrap_err();
    assert!(error.to_string().starts_with("Base URL setting is missing"));
}
