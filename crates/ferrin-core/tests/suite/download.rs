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
