//! Stream uploads must reach the transport before the producer finishes.

use crate::common::upload::StreamingUploadProbe;
use ferrin_anthropic::AnthropicSettings;
use ferrin_anthropic::create_anthropic;
use ferrin_spec::Files;
use ferrin_spec::files::UploadFileOptions;
use pretty_assertions::assert_eq;
use secrecy::SecretString;

#[tokio::test]
async fn multipart_stream_reaches_transport_without_eager_collection() {
    let transport = StreamingUploadProbe::new();
    let data = transport.data();
    let provider = create_anthropic(AnthropicSettings {
        api_key: Some(SecretString::from("test-key")),
        transport: Some(transport),
        ..Default::default()
    })
    .unwrap();
    let result = provider
        .files()
        .upload_file(UploadFileOptions::new(data, "text/plain"))
        .await
        .unwrap();
    assert_eq!(result.provider_reference["anthropic"], "file-stream");
}
