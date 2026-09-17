//! Citation variants and invalid executable calls at the Interactions boundary.

use ferrin_spec::CallOptions;
use ferrin_spec::Content;
use ferrin_spec::LanguageModel;
use ferrin_spec::PromptMessage;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::Source;
use ferrin_testing::Fixture;
use http::Method;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;

fn without_id(source: Source) -> Source {
    match source {
        Source::Url {
            url,
            title,
            provider_metadata,
            ..
        } => Source::Url {
            id: String::new(),
            url,
            title,
            provider_metadata,
        },
        Source::Document {
            media_type,
            title,
            filename,
            provider_metadata,
            ..
        } => Source::Document {
            id: String::new(),
            media_type,
            title,
            filename,
            provider_metadata,
        },
        _ => source,
    }
}

fn url(url: &str, title: &str) -> Source {
    Source::Url {
        id: String::new(),
        url: url.to_owned(),
        title: Some(title.to_owned()),
        provider_metadata: None,
    }
}

fn document(filename: &str, media_type: &str, title: &str) -> Source {
    Source::Document {
        id: String::new(),
        filename: Some(filename.to_owned()),
        media_type: media_type.into(),
        title: title.to_owned(),
        provider_metadata: None,
    }
}

#[tokio::test]
async fn annotations_map_url_document_and_place_citations_and_deduplicate() {
    let test = TestProvider::start().await;
    let response = json!({"id":"interaction-1","status":"completed","steps":[{"type":"model_output","content":[{
        "type":"text","text":"Sources","annotations":[
            {"type":"url_citation","url":"https://example.com/a","title":"A"},
            {"type":"url_citation","url":"https://example.com/a","title":"Repeated"},
            {"type":"file_citation","document_uri":"gs://bucket/report.pdf","file_name":"report.pdf"},
            {"type":"file_citation","url":"https://example.com/report","file_name":"Report"},
            {"type":"place_citation","url":"https://example.com/map","name":"Place"},
            {"type":"unsupported_citation","url":"https://example.com/ignored"},
            {"type":"url_citation","url":""}
        ]
    }]}]});
    test.mount_fixture(
        Method::POST,
        "/v1beta/interactions",
        Fixture::json(&response),
    );
    let result = test
        .provider
        .interactions("gemini-test")
        .do_generate(CallOptions::new(vec![PromptMessage::user_text("sources")]))
        .await
        .unwrap();
    let sources = result
        .content
        .into_iter()
        .filter_map(|part| match part {
            Content::Source(source) => Some(without_id(source)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        sources,
        vec![
            url("https://example.com/a", "A"),
            document("report.pdf", "application/pdf", "report.pdf"),
            url("https://example.com/report", "Report"),
            url("https://example.com/map", "Place"),
        ]
    );
}

#[tokio::test]
async fn builtin_results_extract_nested_maps_documents_and_only_successful_urls() {
    let test = TestProvider::start().await;
    let response = json!({"status":"completed","steps":[
        {"type":"url_context_result","call_id":"url","result":[
            {"url":"https://example.com/good","status":"success","title":"Good"},
            {"url":"https://example.com/bad","status":"failed"}
        ]},
        {"type":"google_maps_result","call_id":"maps","result":[{"places":[
            {"url":"https://example.com/place","name":"Place"},
            {"url":"https://example.com/place","name":"Duplicate"}
        ]}]},
        {"type":"file_search_result","call_id":"files","result":[
            {"document_uri":"gs://bucket/guide.md","title":"Guide"},
            {"file_name":"brief.docx"},
            {"url":"https://example.com/file","title":"File"}
        ]},
        {"type":"google_search_result","call_id":"search","result":[
            {"url":"https://example.com/search","title":"Search"},{"html":"widget"}
        ]}
    ]});
    test.mount_fixture(
        Method::POST,
        "/v1beta/interactions",
        Fixture::json(&response),
    );
    let result = test
        .provider
        .interactions("gemini-test")
        .do_generate(CallOptions::new(vec![PromptMessage::user_text("sources")]))
        .await
        .unwrap();
    let sources = result
        .content
        .into_iter()
        .filter_map(|part| match part {
            Content::Source(source) => Some(without_id(source)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        sources,
        vec![
            url("https://example.com/good", "Good"),
            url("https://example.com/place", "Place"),
            document("guide.md", "text/markdown", "Guide"),
            document(
                "brief.docx",
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                "brief.docx"
            ),
            url("https://example.com/file", "File"),
            url("https://example.com/search", "Search"),
        ]
    );
}

#[tokio::test]
async fn malformed_client_tool_calls_fail_instead_of_inventing_executable_calls() {
    for step in [
        json!({"type":"function_call","name":"run","arguments":{}}),
        json!({"type":"function_call","id":"call","arguments":{}}),
        json!({"type":"function_call","id":"","name":"run","arguments":{}}),
        json!({"type":"function_call","id":"call","name":"","arguments":{}}),
        json!({"type":"function_call","id":"call","name":"run","arguments":"{}"}),
        json!({"type":"function_call","id":"call","name":"run","arguments":[1,2]}),
    ] {
        let test = TestProvider::start().await;
        test.mount_fixture(
            Method::POST,
            "/v1beta/interactions",
            Fixture::json(&json!({"status":"requires_action","steps":[step]})),
        );
        let result = test
            .provider
            .interactions("gemini-test")
            .do_generate(CallOptions::new(vec![PromptMessage::user_text("run")]))
            .await;
        assert!(
            matches!(result, Err(ProviderError::InvalidResponseData(_))),
            "{result:?}"
        );
    }
}
