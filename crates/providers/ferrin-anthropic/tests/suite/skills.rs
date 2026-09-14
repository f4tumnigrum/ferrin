//! Skills API.

use ferrin_spec::Skills;
use ferrin_spec::skills::SkillFile;
use ferrin_spec::skills::SkillFileData;
use ferrin_spec::skills::UploadSkillOptions;
use http::Method;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;

fn files() -> Vec<SkillFile> {
    vec![
        SkillFile {
            path: "weather-helper/SKILL.md".to_owned(),
            data: SkillFileData::Text {
                text: "# Weather helper".to_owned(),
            },
        },
        SkillFile {
            path: "weather-helper/scripts/run.py".to_owned(),
            data: SkillFileData::Data {
                data: bytes::Bytes::from_static(b"print('hi')"),
            },
        },
    ]
}

#[tokio::test]
async fn upload_sends_every_file_then_reads_the_latest_version() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/skills", "skills", "upload");
    test.mount(
        Method::GET,
        "/v1/skills/skill_abc/versions/v1.0",
        "skills",
        "version",
    );
    let mut options = UploadSkillOptions::new(files());
    options.display_title = Some("Weather".to_owned());
    let result = test.provider.skills().upload_skill(options).await.unwrap();
    assert_eq!(result.provider_reference["anthropic"], "skill_abc");
    assert_eq!(result.display_title.as_deref(), Some("Weather"));
    assert_eq!(result.name.as_deref(), Some("weather-helper"));
    assert_eq!(result.description.as_deref(), Some("Weather lookups"));
    assert_eq!(result.latest_version.as_deref(), Some("v1.0"));
    let metadata = result.provider_metadata.unwrap();
    assert_eq!(metadata["anthropic"]["source"], json!("custom"));
    assert_eq!(
        metadata["anthropic"]["createdAt"],
        json!("2026-09-13T10:00:00Z")
    );
    assert!(result.warnings.is_empty());

    let received = test.server.received();
    assert_eq!(received.len(), 2);
    for request in &received {
        assert_eq!(request.header("anthropic-beta"), Some("skills-2025-10-02"));
    }
    let body = received[0].body_text();
    assert!(
        body.contains("name=\"display_title\"\r\n\r\nWeather"),
        "{body}"
    );
    assert!(
        body.contains("name=\"files[]\"; filename=\"weather-helper/SKILL.md\""),
        "{body}"
    );
    assert!(
        body.contains("name=\"files[]\"; filename=\"weather-helper/scripts/run.py\""),
        "{body}"
    );
}

#[tokio::test]
async fn upload_without_a_version_uses_the_skill_response() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/skills", "skills", "upload-no-version");
    let result = test
        .provider
        .skills()
        .upload_skill(UploadSkillOptions::new(files()))
        .await
        .unwrap();
    assert_eq!(result.provider_reference["anthropic"], "skill_pending");
    assert_eq!(result.display_title, None);
    assert_eq!(result.name.as_deref(), Some("weather-helper"));
    assert_eq!(result.latest_version, None);
    assert_eq!(test.server.received_count(), 1);
}
