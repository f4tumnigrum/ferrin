use std::sync::Arc;
use std::sync::Mutex;

use bytes::Bytes;
use ferrin_core::Error;
use ferrin_core::generate_text;
use ferrin_core::prompt::DownloadFn;
use ferrin_core::prompt::DownloadRequest;
use ferrin_core::prompt::DownloadedFile;
use ferrin_message::Message;
use ferrin_message::UserPart;
use ferrin_spec::BoxFuture;
use ferrin_spec::FileData;
use ferrin_spec::PromptMessage;
use ferrin_spec::SupportedUrls;
use ferrin_spec::Usage;
use ferrin_spec::language_model::prompt::UserPromptPart;
use pretty_assertions::assert_eq;
use tokio_util::sync::CancellationToken;
use url::Url;

use super::common::mock;
use super::common::text_result;

struct RecordingDownloader {
    requests: Arc<Mutex<Vec<Vec<DownloadRequest>>>>,
    file: Option<DownloadedFile>,
}

impl DownloadFn for RecordingDownloader {
    fn download(
        &self,
        requests: Vec<DownloadRequest>,
        _: CancellationToken,
    ) -> BoxFuture<'_, Result<Vec<Option<DownloadedFile>>, Error>> {
        self.requests.lock().unwrap().push(requests.clone());
        let files = vec![self.file.clone(); requests.len()];
        Box::pin(async move { Ok(files) })
    }
}

fn image_message() -> Message {
    Message::user_parts([UserPart::image_url(
        Url::parse("https://example.com/private.png").unwrap(),
    )])
}

fn downloaded_file() -> DownloadedFile {
    DownloadedFile {
        data: Bytes::from_static(b"image"),
        media_type: Some("image/png".into()),
    }
}

fn first_file(prompt: &[PromptMessage]) -> &FileData {
    let PromptMessage::User { content, .. } = &prompt[0] else {
        panic!("expected user message")
    };
    let UserPromptPart::File(file) = &content[0] else {
        panic!("expected file")
    };
    &file.data
}

#[tokio::test]
async fn tool_result_files_download_for_both_roles_and_preserve_declared_media_types() {
    for role in ["assistant", "tool"] {
        for media_type in ["image", "application/json"] {
            for streaming in [false, true] {
                let model = mock()
                    .generate(text_result("done"))
                    .stream(ferrin_testing::text_parts(["done"], Usage::default()))
                    .build_shared();
                let requests = Arc::new(Mutex::new(Vec::new()));
                let downloader: Arc<dyn DownloadFn> = Arc::new(RecordingDownloader {
                    requests: Arc::clone(&requests),
                    file: Some(downloaded_file()),
                });
                let mut message = serde_json::json!({"role":role,"content":[{
                    "type":"tool-result","tool_call_id":"one","tool_name":"test",
                    "output":{"type":"content","value":[{
                        "type":"file","data":{"type":"url","url":"https://example.com/private.png"},
                        "media_type":media_type,"filename":"result.bin","provider_options":{"test":{"keep":true}}
                    }]}
                }]});
                let messages: Vec<Message> =
                    serde_json::from_value(serde_json::json!([message])).unwrap();
                let calls = if streaming {
                    ferrin_core::stream_text(Arc::clone(&model))
                        .messages(messages)
                        .download(downloader)
                        .await
                        .unwrap()
                        .consume()
                        .await
                        .unwrap();
                    model.stream_calls()
                } else {
                    generate_text(Arc::clone(&model))
                        .messages(messages)
                        .download(downloader)
                        .await
                        .unwrap();
                    model.generate_calls()
                };
                message["content"][0]["output"]["value"][0]["data"] =
                    serde_json::json!({"type":"data","data":"aW1hZ2U="});
                message["content"][0]["output"]["value"][0]["media_type"] =
                    serde_json::json!(if media_type == "image" {
                        "image/png"
                    } else {
                        media_type
                    });
                assert_eq!(
                    calls[0].prompt,
                    serde_json::from_value::<Vec<PromptMessage>>(serde_json::json!([message]))
                        .unwrap()
                );
                assert_eq!(
                    *requests.lock().unwrap(),
                    vec![vec![DownloadRequest {
                        url: Url::parse("https://example.com/private.png").unwrap(),
                        is_url_supported_by_model: false,
                    }]]
                );
            }
        }
    }
}

#[tokio::test]
async fn custom_downloaders_receive_supported_urls_and_may_preserve_them() {
    for supported in [false, true] {
        for inline in [false, true] {
            let model = mock()
                .supported_urls(if supported {
                    SupportedUrls::all()
                } else {
                    SupportedUrls::default()
                })
                .generate(text_result("done"))
                .stream(ferrin_testing::text_parts(["done"], Usage::default()))
                .build_shared();
            let requests = Arc::new(Mutex::new(Vec::new()));
            let downloader: Arc<dyn DownloadFn> = Arc::new(RecordingDownloader {
                requests: Arc::clone(&requests),
                file: inline.then(downloaded_file),
            });
            generate_text(Arc::clone(&model))
                .messages([image_message()])
                .download(Arc::clone(&downloader))
                .await
                .unwrap();
            ferrin_core::stream_text(Arc::clone(&model))
                .messages([image_message()])
                .download(downloader)
                .await
                .unwrap()
                .consume()
                .await
                .unwrap();
            let url = Url::parse("https://example.com/private.png").unwrap();
            let expected_request = DownloadRequest {
                url: url.clone(),
                is_url_supported_by_model: supported,
            };
            assert_eq!(
                *requests.lock().unwrap(),
                vec![vec![expected_request.clone()], vec![expected_request]]
            );
            let expected_data = if inline {
                FileData::bytes(downloaded_file().data)
            } else {
                FileData::url(url)
            };
            for call in model
                .generate_calls()
                .iter()
                .chain(model.stream_calls().iter())
            {
                assert_eq!(first_file(&call.prompt), &expected_data);
            }
        }
    }
}

#[tokio::test]
async fn successful_downloads_are_cached_per_invocation_across_model_changes() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let downloader: Arc<dyn DownloadFn> = Arc::new(RecordingDownloader {
        requests: Arc::clone(&requests),
        file: Some(downloaded_file()),
    });
    for streaming in [false, true] {
        let first = mock()
            .generate(super::common::tool_call_result(
                "call",
                "get_weather",
                &serde_json::json!({"city":"Rome"}),
            ))
            .stream(vec![
                ferrin_spec::StreamPart::stream_start(),
                ferrin_spec::StreamPart::ToolCall(ferrin_spec::ToolCall::new(
                    "call",
                    "get_weather",
                    "{\"city\":\"Rome\"}",
                )),
                ferrin_spec::StreamPart::finish(
                    ferrin_spec::FinishReason::tool_calls(),
                    Usage::default(),
                ),
            ])
            .build_shared();
        let second = mock()
            .supported_urls(SupportedUrls::all())
            .generate(text_result("done"))
            .stream(ferrin_testing::text_parts(["done"], Usage::default()))
            .build_shared();
        let next_model = Arc::clone(&second);
        let prepare = move |ctx: &generate_text::PrepareStepContext<'_>| {
            if ctx.step_number == 0 {
                generate_text::StepOverrides::none()
            } else {
                generate_text::StepOverrides::none().with_model(Arc::clone(&next_model))
            }
        };
        if streaming {
            ferrin_core::stream_text(Arc::clone(&first))
                .messages([image_message()])
                .tools(super::common::weather_tools())
                .download(Arc::clone(&downloader))
                .prepare_step(prepare)
                .stop_when(ferrin_core::step_count(2))
                .await
                .unwrap()
                .consume()
                .await
                .unwrap();
        } else {
            generate_text(Arc::clone(&first))
                .messages([image_message()])
                .tools(super::common::weather_tools())
                .download(Arc::clone(&downloader))
                .prepare_step(prepare)
                .stop_when(ferrin_core::step_count(2))
                .await
                .unwrap();
        }
        for call in first
            .generate_calls()
            .iter()
            .chain(first.stream_calls().iter())
            .chain(second.generate_calls().iter())
            .chain(second.stream_calls().iter())
        {
            assert_eq!(
                first_file(&call.prompt),
                &FileData::bytes(downloaded_file().data)
            );
        }
    }
    // Reusing the same downloader for a separate call starts a new cache.
    assert_eq!(requests.lock().unwrap().len(), 2);
}

struct SupportAwareDownloader(Arc<Mutex<Vec<bool>>>);
impl DownloadFn for SupportAwareDownloader {
    fn download(
        &self,
        requests: Vec<DownloadRequest>,
        _: CancellationToken,
    ) -> BoxFuture<'_, Result<Vec<Option<DownloadedFile>>, Error>> {
        let mut seen = self.0.lock().unwrap();
        let files = requests
            .into_iter()
            .map(|request| {
                seen.push(request.is_url_supported_by_model);
                (!request.is_url_supported_by_model).then(downloaded_file)
            })
            .collect();
        Box::pin(async move { Ok(files) })
    }
}

#[tokio::test]
async fn preserved_urls_are_reconsidered_when_the_model_changes() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let first = mock()
        .supported_urls(SupportedUrls::all())
        .generate(super::common::tool_call_result(
            "call",
            "get_weather",
            &serde_json::json!({"city":"Rome"}),
        ))
        .build_shared();
    let second = mock().generate(text_result("done")).build_shared();
    let next_model = Arc::clone(&second);
    generate_text(Arc::clone(&first))
        .messages([image_message()])
        .tools(super::common::weather_tools())
        .download(Arc::new(SupportAwareDownloader(Arc::clone(&seen))))
        .stop_when(ferrin_core::step_count(2))
        .prepare_step(move |ctx: &generate_text::PrepareStepContext<'_>| {
            if ctx.step_number == 0 {
                generate_text::StepOverrides::none()
            } else {
                generate_text::StepOverrides::none().with_model(Arc::clone(&next_model))
            }
        })
        .await
        .unwrap();
    assert_eq!(*seen.lock().unwrap(), vec![true, false]);
    assert_eq!(
        first_file(&first.generate_calls()[0].prompt),
        &FileData::url(Url::parse("https://example.com/private.png").unwrap())
    );
    assert_eq!(
        first_file(&second.generate_calls()[0].prompt),
        &FileData::bytes(downloaded_file().data)
    );
}
