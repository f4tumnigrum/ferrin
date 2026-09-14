//! Skills API.

use ferrin_spec::Skills;
use ferrin_spec::skills::SkillFile;
use ferrin_spec::skills::SkillFileData;
use ferrin_spec::skills::UploadSkillOptions;
use http::Method;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;

#[tokio::test]
async fn upload_sends_every_file_and_maps_the_result() {
    let test = TestProvider::start().await;
    test.mount(Method::POST, "/v1/skills", "skills", "upload");
    let mut options = UploadSkillOptions::new(vec![
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
    ]);
    options.display_title = Some("Weather".to_owned());
    let result = test.provider.skills().upload_skill(options).await.unwrap();
    assert_eq!(result.provider_reference["openai"], "skill_abc");
    assert_eq!(result.name.as_deref(), Some("weather-helper"));
    assert_eq!(result.latest_version.as_deref(), Some("v1"));
    let metadata = result.provider_metadata.unwrap();
    assert_eq!(metadata["openai"]["defaultVersion"], json!("v1"));
    assert_eq!(result.warnings.len(), 1, "{:?}", result.warnings);
    let body = test.only_request().body_text();
    assert!(
        body.contains("name=\"files[]\"; filename=\"weather-helper/SKILL.md\""),
        "{body}"
    );
    assert!(
        body.contains("name=\"files[]\"; filename=\"weather-helper/scripts/run.py\""),
        "{body}"
    );
}
