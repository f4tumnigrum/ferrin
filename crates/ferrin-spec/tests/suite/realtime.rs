use ferrin_spec::realtime_model::WebSocketConfig;
use pretty_assertions::assert_eq;
use url::Url;

#[test]
fn websocket_debug_redacts_credentials_in_every_connection_field() {
    let config = WebSocketConfig {
        url: Url::parse("wss://example.com/token-in-path?access_token=token-in-query").unwrap(),
        protocols: vec!["realtime".to_owned(), "token-in-protocol".to_owned()],
    };
    assert_eq!(
        format!("{config:?}"),
        "WebSocketConfig { url: \"***\", protocols: \"***\" }"
    );
    assert!(format!("{config:#?}").contains("***"));
    assert!(!format!("{config:#?}").contains("token-in-"));
}
