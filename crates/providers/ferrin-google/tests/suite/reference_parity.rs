//! Existing modality contracts compared with AI SDK revision 6c6c221.

use bytes::Bytes;
use ferrin_spec::CallOptions;
use ferrin_spec::Content;
use ferrin_spec::EmbeddingModel;
use ferrin_spec::Files;
use ferrin_spec::LanguageModel;
use ferrin_spec::PromptMessage;
use ferrin_spec::SpeechModel;
use ferrin_spec::TranscriptionModel;
use ferrin_spec::embedding_model::EmbedOptions;
use ferrin_spec::files::UploadFileOptions;
use ferrin_spec::image_model::ImageOptions;
use ferrin_spec::language_model::prompt::AssistantPromptPart;
use ferrin_spec::language_model::prompt::ToolResultOutput;
use ferrin_spec::language_model::prompt::ToolResultPart;
use ferrin_spec::speech_model::SpeechOptions;
use ferrin_spec::transcription_model::TranscriptionOptions;
use ferrin_spec::video_model::VideoModel;
use ferrin_testing::Fixture;
use http::Method;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;
use super::common::google_options;

#[tokio::test]
async fn builtin_results_replay_native_json_and_mcp_tool_names() {
    let test = TestProvider::start().await;
    let steps = json!([
        {"type":"google_search_result","call_id":"search","result":[{"url":"https://example.com","title":"Example"}]},
        {"type":"code_execution_result","call_id":"code","result":{"stdout":"1"},"signature":"signature"},
        {"type":"mcp_server_tool_result","call_id":"remote","name":"weather","result":{"temp":20},"is_error":true},
    ]);
    test.mount_fixture(
        Method::POST,
        "/v1beta/interactions",
        Fixture::json(&json!({"id":"interaction","status":"completed","steps":steps})),
    );
    let model = test.provider.interactions("gemini-test");
    let result = model
        .do_generate(CallOptions::new(vec![PromptMessage::user_text("run")]))
        .await
        .unwrap();
    let parts = result
        .content
        .into_iter()
        .filter_map(|content| match content {
            Content::ToolResult(result) => Some(AssistantPromptPart::ToolResult(ToolResultPart {
                tool_call_id: result.tool_call_id,
                tool_name: result.tool_name,
                output: if result.is_error {
                    ToolResultOutput::error_json(result.result)
                } else {
                    ToolResultOutput::json(result.result)
                },
                provider_options: result.provider_metadata,
            })),
            _ => None,
        })
        .collect();
    let body = model
        .prepare_request(&CallOptions::new(vec![PromptMessage::assistant(parts)]))
        .unwrap();
    assert_eq!(
        body["input"],
        json!([
            {"type":"google_search_result","call_id":"search","result":[{"url":"https://example.com","title":"Example"}],"is_error":false},
            {"type":"code_execution_result","call_id":"code","result":{"stdout":"1"},"signature":"signature","is_error":false},
            {"type":"mcp_server_tool_result","call_id":"remote","name":"weather","result":{"temp":20},"is_error":true},
        ])
    );
}

#[tokio::test]
async fn embedding_option_shapes_match_the_reference_schema() {
    let test = TestProvider::start().await;
    for value in [
        json!({"taskType":"UNKNOWN"}),
        json!({"content":[[]]}),
        json!({"content":[[{"text":3}]]}),
        json!({"content":[[{"inlineData":{"data":"AA=="}}]]}),
        json!({"content":[[{"fileData":{"mimeType":"image/png"}}]]}),
    ] {
        let mut options = EmbedOptions::new(vec!["input".to_owned()]);
        options.provider_options = google_options(value);
        assert!(
            test.provider
                .embedding("gemini-embedding-2")
                .do_embed(options)
                .await
                .is_err()
        );
    }
    let mut options = EmbedOptions::new(vec![String::new(), "text".to_owned()]);
    options.provider_options = google_options(
        json!({"taskType":"RETRIEVAL_DOCUMENT","content":[[{"inlineData":{"mimeType":"image/png","data":"AA=="}}],null]}),
    );
    assert_eq!(
        test.provider
            .embedding("gemini-embedding-2")
            .prepare_request(&options)
            .unwrap()
            .body,
        json!({"requests":[
            {"model":"models/gemini-embedding-2","content":{"role":"user","parts":[{"inlineData":{"mimeType":"image/png","data":"AA=="}}]},"taskType":"RETRIEVAL_DOCUMENT"},
            {"model":"models/gemini-embedding-2","content":{"role":"user","parts":[{"text":"text"}]},"taskType":"RETRIEVAL_DOCUMENT"},
        ]})
    );
    assert_eq!(test.server.received_count(), 0);
}

#[tokio::test]
async fn missing_embedding_arrays_fail_instead_of_becoming_empty_vectors() {
    for (values, response) in [
        (vec!["one"], json!({"embedding":{}})),
        (vec!["one", "two"], json!({})),
    ] {
        let test = TestProvider::start().await;
        let action = if values.len() == 1 {
            "embedContent"
        } else {
            "batchEmbedContents"
        };
        test.mount_fixture(
            Method::POST,
            &format!("/v1beta/models/gemini-embedding-2:{action}"),
            Fixture::json(&response),
        );
        let result = test
            .provider
            .embedding("gemini-embedding-2")
            .do_embed(EmbedOptions::new(
                values.into_iter().map(str::to_owned).collect(),
            ))
            .await;
        assert!(result.is_err());
    }
}

#[tokio::test]
async fn malformed_speech_transcription_and_image_options_fail_before_http() {
    let test = TestProvider::start().await;
    for value in [
        json!({}),
        json!({"speakerVoiceConfigs":[{"speaker":"Joe","voiceConfig":{}}]}),
    ] {
        let mut options = SpeechOptions::new("Joe: Hi");
        options.provider_options = google_options(json!({"multiSpeakerVoiceConfig":value}));
        assert!(
            test.provider
                .speech("gemini-tts")
                .do_generate(options)
                .await
                .is_err()
        );
    }
    let mut options = TranscriptionOptions::new(Bytes::from_static(b"audio"), "audio/wav");
    options.provider_options = google_options(json!({"mode":"unknown"}));
    assert!(
        test.provider
            .transcription("gemini-transcribe")
            .do_generate(options)
            .await
            .is_err()
    );
    for value in [
        json!({"googleSearch":true}),
        json!({"imageConfig":[]}),
        json!({"googleSearch":{"searchTypes":{"webSearch":false}}}),
        json!({"googleSearch":{"timeRangeFilter":{"startTime":"2026-01-01"}}}),
    ] {
        let mut options = ImageOptions::new("a cat");
        options.provider_options = google_options(value);
        assert!(
            test.provider
                .image("gemini-image")
                .prepare_call(&options)
                .is_err()
        );
    }
    assert_eq!(test.server.received_count(), 0);
}

#[tokio::test]
async fn speech_optional_inline_fields_use_the_reference_fallback() {
    for mime in [None, Some(serde_json::Value::Null)] {
        let test = TestProvider::start().await;
        let mut audio = json!({"data":"AQACAA=="});
        if let Some(mime) = mime {
            audio["mimeType"] = mime;
        }
        test.mount_fixture(Method::POST,"/v1beta/models/gemini-tts:generateContent",Fixture::json(&json!({"candidates":[{"content":{"parts":[{"inlineData":{"data":null,"mimeType":null}},{"inlineData":audio}]}}]})));
        let result = test
            .provider
            .speech("gemini-tts")
            .do_generate(SpeechOptions::new("hello"))
            .await
            .unwrap();
        assert_eq!(
            result.audio,
            ferrin_google::speech::add_wav_header(&[1, 0, 2, 0], 24000)
        );
        assert_eq!(
            result.provider_metadata.unwrap()["google"],
            serde_json::from_value::<ferrin_spec::JsonObject>(
                json!({"sampleRate":24000,"mimeType":null})
            )
            .unwrap()
        );
    }
}

#[tokio::test]
async fn file_resource_ids_are_encoded_without_path_traversal_or_query_injection() {
    for (name, path) in [
        ("files/..", "/v1beta/files/%252E%252E"),
        (
            "files/a?key=other#fragment",
            "/v1beta/files/a%3Fkey=other%23fragment",
        ),
        ("files/a/b", "/v1beta/files%2Fa%2Fb"),
    ] {
        let test = TestProvider::start().await;
        test.mount_fixture(Method::GET,path,Fixture::json(&json!({"name":name,"state":"ACTIVE","uri":"https://example.com/file","mimeType":"text/plain"})));
        test.provider
            .files()
            .fetch_file(name, &Default::default(), Default::default())
            .await
            .unwrap();
        let request = test.only_request();
        assert_eq!((request.path, request.query), (path.to_owned(), None));
    }
}

#[tokio::test]
async fn invalid_file_options_and_cancelled_input_fail_before_upload() {
    let test = TestProvider::start().await;
    for options in [
        json!({"pollIntervalMs":0}),
        json!({"pollTimeoutMs":-1}),
        json!({"displayName":false}),
    ] {
        let mut call = UploadFileOptions::new(Bytes::from_static(b"data"), "text/plain");
        call.provider_options = google_options(options);
        assert!(test.provider.files().upload_file(call).await.is_err());
    }
    let mut call = UploadFileOptions::new(Bytes::new(), "text/plain");
    call.data = ferrin_spec::files::UploadData::Stream(Box::pin(futures_util::stream::pending()));
    call.cancellation.cancel();
    assert!(matches!(
        test.provider.files().upload_file(call).await,
        Err(ferrin_spec::error::ProviderError::Cancelled)
    ));
    assert_eq!(test.server.received_count(), 0);
}

#[test]
fn batch_missing_zero_counters_keep_complete_count_summaries() {
    for stats in [
        json!({"requestCount":"2","successfulRequestCount":"2"}),
        json!({"requestCount":0}),
    ] {
        let operation: ferrin_google::batch::BatchOperation = serde_json::from_value(
            json!({"name":"batches/test","done":true,"metadata":{"batchStats":stats}}),
        )
        .unwrap();
        let total = operation
            .metadata
            .as_ref()
            .unwrap()
            .batch_stats
            .as_ref()
            .unwrap()
            .request_count
            .unwrap();
        let mut expected =
            ferrin_spec::batch::BatchStatus::new(ferrin_spec::batch::BatchState::Completed);
        expected.request_counts = Some(ferrin_spec::batch::BatchRequestCounts {
            total,
            pending: 0,
            completed: total,
            failed: 0,
        });
        assert_eq!(ferrin_google::batch::map_status(&operation), expected);
    }
}

#[tokio::test]
async fn video_download_uses_the_configured_header_override_without_cross_origin_leakage() {
    let test = TestProvider::start_with(|mut settings| {
        settings
            .headers
            .insert("x-goog-api-key", "configured-video-key")
            .unwrap();
        settings
    })
    .await;
    let same = test.server.url().join("video/result").unwrap();
    test.mount_fixture(
        Method::GET,
        "/v1beta/models/veo/operations/test",
        Fixture::json(
            &json!({"done":true,"response":{"generateVideoResponse":{"generatedSamples":[
                {"video":{"uri":same}},{"video":{"uri":"https://cdn.example.com/video"}}
            ]}}}),
        ),
    );
    let result = test
        .provider
        .video("veo")
        .do_status(ferrin_spec::video_model::VideoStatusOptions {
            operation: json!({"operationName":"models/veo/operations/test"}),
            headers: Default::default(),
            cancellation: Default::default(),
        })
        .await
        .unwrap();
    let ferrin_spec::video_model::VideoStatusResult::Completed { videos, .. } = result else {
        panic!("expected completed")
    };
    let mut authenticated = same;
    authenticated
        .query_pairs_mut()
        .append_pair("key", "configured-video-key");
    assert_eq!(
        videos,
        vec![
            ferrin_spec::video_model::VideoData {
                data: ferrin_spec::FileData::url(authenticated),
                media_type: "video/mp4".into()
            },
            ferrin_spec::video_model::VideoData {
                data: ferrin_spec::FileData::url("https://cdn.example.com/video".parse().unwrap()),
                media_type: "video/mp4".into()
            },
        ]
    );
}

#[tokio::test]
async fn video_zero_seed_and_duration_are_omitted_as_in_the_reference() {
    let test = TestProvider::start().await;
    let mut options = ferrin_spec::video_model::VideoOptions::new("a wave");
    options.seed = Some(0);
    options.duration = Some(0.0);
    assert_eq!(
        test.provider.video("veo").prepare_request(&options).body,
        json!({"instances":[{"prompt":"a wave"}],"parameters":{"sampleCount":1}})
    );
}

#[tokio::test]
async fn interactions_legacy_image_settings_and_empty_stop_sequences_follow_reference() {
    let test = TestProvider::start().await;
    let model = test.provider.interactions("gemini-test");
    let mut call = CallOptions::new(vec![PromptMessage::user_text("draw")]);
    call.stop_sequences = Some(Vec::new());
    call.reasoning = ferrin_spec::language_model::ReasoningEffort::High;
    call.provider_options = google_options(
        json!({"imageConfig":{"aspectRatio":"1:1","imageSize":"1K"},"signature":"ignored-call-signature","interactionId":"ignored-call-id"}),
    );
    assert_eq!(
        model.prepare_request(&call).unwrap(),
        json!({
            "model":"gemini-test","input":[{"type":"user_input","content":[{"type":"text","text":"draw"}]}],
            "response_format":[{"type":"image","mime_type":"image/png","aspect_ratio":"1:1","image_size":"1K"}],
        })
    );
    call.provider_options = google_options(
        json!({"imageConfig":{"aspectRatio":"1:1"},"responseFormat":[{"type":"image","aspectRatio":"16:9"}]}),
    );
    assert_eq!(
        model.prepare_request(&call).unwrap()["response_format"],
        json!([{"type":"image","aspect_ratio":"16:9"}])
    );
}

#[tokio::test]
async fn linked_history_removes_matching_tool_results_and_merges_adjacent_user_text() {
    use ferrin_spec::language_model::prompt::TextPart;
    use ferrin_spec::language_model::prompt::ToolCallPart;
    use ferrin_spec::language_model::prompt::ToolPromptPart;
    use ferrin_spec::language_model::prompt::UserPromptPart;
    let test = TestProvider::start().await;
    let mut call = CallOptions::new(vec![
        PromptMessage::user(vec![
            UserPromptPart::Text(TextPart::new("one")),
            UserPromptPart::Text(TextPart::new("two")),
        ]),
        PromptMessage::assistant(vec![AssistantPromptPart::ToolCall(ToolCallPart {
            tool_call_id: "old".into(),
            tool_name: "run".into(),
            input: json!({}),
            provider_executed: false,
            provider_options: Some(google_options(json!({"interactionId":"previous"}))),
        })]),
        PromptMessage::tool(vec![ToolPromptPart::ToolResult(ToolResultPart {
            tool_call_id: "old".into(),
            tool_name: "run".into(),
            output: ToolResultOutput::text("old result"),
            provider_options: None,
        })]),
    ]);
    call.provider_options = google_options(json!({"previousInteractionId":"previous"}));
    assert_eq!(
        test.provider
            .interactions("gemini-test")
            .prepare_request(&call)
            .unwrap()["input"],
        json!([
            {"type":"user_input","content":[{"type":"text","text":"one\n\ntwo"}]},
        ])
    );
}

#[tokio::test]
async fn interactions_agent_keeps_provider_formats_and_normalizes_environment() {
    let test = TestProvider::start().await;
    let model = test.provider.interactions("ignored");
    let mut call = CallOptions::new(vec![PromptMessage::user_text("report")]);
    call.response_format = Some(ferrin_spec::language_model::ResponseFormat::json(json!({
        "type": "object"
    })));
    call.provider_options = google_options(json!({
        "agent":"dynamic-agent", "unknownOption":"ignored",
        "agentConfig":{"type":"dynamic","unknownField":"ignored"},
        "responseFormat":[
            {"type":"text","mimeType":null,"schema":{"keepCamelCase":true},"extraField":1},
            {"type":"video","aspectRatio":"16:9","resolution":"720p","duration":"5s","delivery":"uri","gcsUri":"gs://bucket/movie","extraField":1}
        ],
        "environment":{"type":"remote","extra":true,"sources":[],"network":{"allowlist":[{"domain":"example.com","extra":true,"transform":[{"X-Header":"value"}]}]}}
    }));
    assert_eq!(
        model.prepare_request(&call).unwrap(),
        json!({
            "agent":"dynamic-agent","input":[{"type":"user_input","content":[{"type":"text","text":"report"}]}],
            "agent_config":{"type":"dynamic"},
            "response_format":[{"type":"text","schema":{"keepCamelCase":true}},{"type":"video","aspect_ratio":"16:9","resolution":"720p","duration":"5s","delivery":"uri","gcs_uri":"gs://bucket/movie"}],
            "environment":{"type":"remote","network":{"allowlist":[{"domain":"example.com","transform":[{"X-Header":"value"}]}]}}
        })
    );
}

#[tokio::test]
async fn interactions_rejects_malformed_known_option_shapes() {
    let test = TestProvider::start().await;
    for options in [
        json!({"responseFormat":[{"type":"unknown"}]}),
        json!({"responseFormat":[{"type":"video","duration":5}]}),
        json!({"agentConfig":{"type":"deep-research","thinkingSummaries":"invalid"}}),
        json!({"environment":{"type":"remote","sources":[{"type":"inline","target":"file"}]}}),
        json!({"environment":{"type":"remote","network":{"allowlist":[{"domain":"example.com","transform":[{"header":1}]}]}}}),
        json!({"thinkingLevel":"invalid"}),
    ] {
        let mut call = CallOptions::new(vec![PromptMessage::user_text("report")]);
        call.provider_options = google_options(options);
        assert!(
            test.provider
                .interactions("model")
                .prepare_request(&call)
                .is_err()
        );
    }
}

#[tokio::test]
async fn interactions_video_processing_drops_unknown_and_non_numeric_fields() {
    use ferrin_spec::FileData;
    use ferrin_spec::language_model::prompt::FilePart;
    use ferrin_spec::language_model::prompt::UserPromptPart;
    let test = TestProvider::start().await;
    let mut video = FilePart::new(
        FileData::Bytes {
            data: Bytes::from_static(b"video"),
        },
        "video/mp4",
    );
    video.provider_options = Some(google_options(
        json!({"processing":{"type":"static","startOffset":2,"endOffset":"bad","fps":0.5,"extra":true}}),
    ));
    let mut image = video.clone();
    image.media_type = "image/png".into();
    let call = CallOptions::new(vec![PromptMessage::user(vec![
        UserPromptPart::File(video),
        UserPromptPart::File(image),
    ])]);
    assert_eq!(
        test.provider
            .interactions("model")
            .prepare_request(&call)
            .unwrap()["input"][0]["content"],
        json!([
            {"type":"video","data":"dmlkZW8=","mime_type":"video/mp4","processing":{"type":"static","start_offset":2,"fps":0.5}},
            {"type":"image","data":"dmlkZW8=","mime_type":"image/png"}
        ])
    );
}

#[tokio::test]
async fn interactions_provider_tools_match_reference_fields_and_defaults() {
    let test = TestProvider::start().await;
    let definitions = [
        (
            "google_search",
            json!({"searchTypes":{"webSearch":{},"imageSearch":null},"timeRangeFilter":{"startTime":"ignored"},"extra":true}),
        ),
        ("google_search", json!({"searchTypes":{"webSearch":null}})),
        ("code_execution", json!({"extra":true})),
        ("url_context", json!({"extra":true})),
        (
            "file_search",
            json!({"fileSearchStoreNames":["store"],"topK":3,"metadataFilter":null,"extra":true}),
        ),
        (
            "google_maps",
            json!({"latitude":0,"longitude":1,"enableWidget":false,"extra":true}),
        ),
        (
            "computer_use",
            json!({"environment":null,"excludedPredefinedFunctions":["open_web_browser"],"extra":true}),
        ),
        (
            "mcp_server",
            json!({"name":"remote","url":"https://example.com/mcp","headers":{"X-Custom-Header":"value"},"allowedTools":["weather"],"extra":true}),
        ),
        (
            "retrieval",
            json!({"retrievalTypes":null,"vertexAiSearchConfig":{"datastores":["store"],"engine":"engine","opaqueKey":true},"extra":true}),
        ),
    ];
    let mut call = CallOptions::new(vec![PromptMessage::user_text("use tools")]);
    call.tool_choice = Some(ferrin_spec::language_model::ToolChoice::Required);
    call.tools = definitions
        .into_iter()
        .map(|(kind, args)| {
            ferrin_spec::ToolDefinition::provider(
                format!("google.{kind}"),
                kind,
                args.as_object().unwrap().clone(),
            )
        })
        .collect();
    let body = test
        .provider
        .interactions("model")
        .prepare_request(&call)
        .unwrap();
    assert_eq!(
        body,
        json!({
            "model":"model",
            "input":[{"type":"user_input","content":[{"type":"text","text":"use tools"}]}],
            "tools":[
                {"type":"google_search","search_types":["web_search"]},
                {"type":"google_search"},
                {"type":"code_execution"},
                {"type":"url_context"},
                {"type":"file_search","file_search_store_names":["store"],"top_k":3},
                {"type":"google_maps","latitude":0,"longitude":1,"enable_widget":false},
                {"type":"computer_use","environment":"browser","excludedPredefinedFunctions":["open_web_browser"]},
                {"type":"mcp_server","name":"remote","url":"https://example.com/mcp","headers":{"X-Custom-Header":"value"},"allowed_tools":["weather"]},
                {"type":"retrieval","retrieval_types":["vertex_ai_search"],"vertex_ai_search_config":{"datastores":["store"],"engine":"engine","opaqueKey":true}}
            ]
        })
    );
    assert_eq!(test.server.received_count(), 0);
}

#[tokio::test]
async fn interactions_unknown_tools_emit_reference_warning_without_wire_entries() {
    let test = TestProvider::start().await;
    test.mount_fixture(
        Method::POST,
        "/v1beta/interactions",
        Fixture::json(&json!({
            "id":"result","status":"completed","steps":[]
        })),
    );
    let mut call = CallOptions::new(vec![PromptMessage::user_text("use tools")]);
    call.tools.push(ferrin_spec::ToolDefinition::provider(
        "other.unknown",
        "unknown",
        Default::default(),
    ));
    let result = test
        .provider
        .interactions("model")
        .do_generate(call)
        .await
        .unwrap();
    assert_eq!(
        result.warnings,
        vec![ferrin_spec::Warning::unsupported_with_details(
            "provider-defined tool other.unknown",
            "provider-defined tool other.unknown is not supported by google.interactions; tool dropped.",
        )]
    );
    assert_eq!(
        test.only_request().body_json().unwrap(),
        json!({
            "model":"model","input":[{"type":"user_input","content":[{"type":"text","text":"use tools"}]}]
        })
    );
}
