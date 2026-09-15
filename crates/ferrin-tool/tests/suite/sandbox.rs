use bytes::Bytes;
use ferrin_tool::Sandbox;
use ferrin_tool::sandbox::LocalProcessSandbox;
use ferrin_tool::sandbox::ProcessOptions;
use ferrin_tool::sandbox::ReadFileOptions;
use ferrin_tool::sandbox::ReadTextFileOptions;
use ferrin_tool::sandbox::WriteFileOptions;
use futures_util::StreamExt;
use pretty_assertions::assert_eq;

fn temp_root(name: &str) -> std::path::PathBuf {
    let root =
        std::env::temp_dir().join(format!("ferrin-tool-sandbox-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

#[tokio::test]
async fn files_round_trip_with_line_ranges() {
    let root = temp_root("files");
    let sandbox = LocalProcessSandbox::new(&root);
    assert!(sandbox.description().contains("no isolation"));
    sandbox
        .write_text_file(WriteFileOptions::new(
            "nested/dir/notes.txt",
            "one\ntwo\nthree\n".to_owned(),
        ))
        .await
        .unwrap();
    let all = sandbox
        .read_text_file(ReadTextFileOptions::new("nested/dir/notes.txt"))
        .await
        .unwrap();
    assert_eq!(all.as_deref(), Some("one\ntwo\nthree\n"));
    let range = sandbox
        .read_text_file(ReadTextFileOptions::new("nested/dir/notes.txt").lines(2, 10))
        .await
        .unwrap();
    assert_eq!(range.as_deref(), Some("two\nthree\n"));
    let missing = sandbox
        .read_text_file(ReadTextFileOptions::new("nope.txt"))
        .await
        .unwrap();
    assert_eq!(missing, None);

    sandbox
        .write_binary_file(WriteFileOptions::new(
            "bin.dat",
            Bytes::from_static(&[1, 2, 3]),
        ))
        .await
        .unwrap();
    let bytes = sandbox
        .read_binary_file(ReadFileOptions::new("bin.dat"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&bytes[..], &[1, 2, 3]);

    let stream = futures_util::stream::iter(vec![
        Ok(Bytes::from_static(b"ab")),
        Ok(Bytes::from_static(b"cd")),
    ]);
    sandbox
        .write_file(WriteFileOptions::new("streamed.txt", Box::pin(stream) as _))
        .await
        .unwrap();
    let mut chunks = sandbox
        .read_file(ReadFileOptions::new("streamed.txt"))
        .await
        .unwrap()
        .unwrap();
    let mut collected = Vec::new();
    while let Some(chunk) = chunks.next().await {
        collected.extend_from_slice(&chunk.unwrap());
    }
    assert_eq!(collected, b"abcd");
    assert!(
        sandbox
            .read_text_file(ReadTextFileOptions {
                encoding: Some("latin1".to_owned()),
                ..ReadTextFileOptions::new("bin.dat")
            })
            .await
            .is_err()
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[cfg(unix)]
#[tokio::test]
async fn commands_run_and_spawn() {
    let root = temp_root("commands");
    let sandbox = LocalProcessSandbox::new(&root);
    let result = sandbox
        .run(ProcessOptions::new("printf hello; printf err 1>&2; exit 3").env("X", "1"))
        .await
        .unwrap();
    assert_eq!(result.exit_code, 3);
    assert_eq!(result.stdout, "hello");
    assert_eq!(result.stderr, "err");

    let result = sandbox.run(ProcessOptions::new("pwd")).await.unwrap();
    assert_eq!(
        std::fs::canonicalize(result.stdout.trim()).unwrap(),
        std::fs::canonicalize(&root).unwrap()
    );

    let mut process = sandbox
        .spawn(ProcessOptions::new("echo spawned"))
        .await
        .unwrap();
    assert!(process.pid().is_some());
    let mut stdout = process.take_stdout().unwrap();
    let mut out = Vec::new();
    while let Some(chunk) = stdout.next().await {
        out.extend_from_slice(&chunk.unwrap());
    }
    assert_eq!(out, b"spawned\n");
    assert_eq!(process.wait().await.unwrap(), 0);
    process.kill().await.unwrap();

    let token = tokio_util::sync::CancellationToken::new();
    token.cancel();
    let error = sandbox
        .run(ProcessOptions {
            cancellation: token,
            ..ProcessOptions::new("sleep 5")
        })
        .await
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::Interrupted);
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn file_stream_cancellation_is_checked_after_open_and_between_chunks() {
    for read_first in [false, true] {
        let root = temp_root(if read_first {
            "cancel-file-chunks"
        } else {
            "cancel-file-open"
        });
        let sandbox = LocalProcessSandbox::new(&root);
        std::fs::write(root.join("bytes"), vec![1; 24 * 1024]).unwrap();
        let cancellation = tokio_util::sync::CancellationToken::new();
        let mut stream = sandbox
            .read_file(ReadFileOptions {
                cancellation: cancellation.clone(),
                ..ReadFileOptions::new("bytes")
            })
            .await
            .unwrap()
            .unwrap();
        if read_first {
            assert!(!stream.next().await.unwrap().unwrap().is_empty());
        }
        cancellation.cancel();
        assert_eq!(
            stream.next().await.unwrap().unwrap_err().kind(),
            std::io::ErrorKind::Interrupted
        );
        assert!(stream.next().await.is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(unix)]
#[tokio::test]
async fn pre_cancelled_spawn_and_run_do_not_start_a_process() {
    let root = temp_root("cancel-before-spawn");
    let sandbox = LocalProcessSandbox::new(&root);
    let cancellation = tokio_util::sync::CancellationToken::new();
    cancellation.cancel();
    let options = ProcessOptions {
        cancellation,
        ..ProcessOptions::new("printf side-effect > created")
    };
    let Err(error) = sandbox.spawn(options.clone()).await else {
        panic!("cancelled spawn must fail before process creation");
    };
    assert_eq!(error.kind(), std::io::ErrorKind::Interrupted);
    assert_eq!(
        sandbox.run(options).await.unwrap_err().kind(),
        std::io::ErrorKind::Interrupted
    );
    assert!(!root.join("created").exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn cancellation_wakes_output_consumers_before_wait_is_polled() {
    let root = temp_root("cancel-process-output");
    let sandbox = LocalProcessSandbox::new(&root);
    let cancellation = tokio_util::sync::CancellationToken::new();
    let mut process = sandbox
        .spawn(ProcessOptions {
            cancellation: cancellation.clone(),
            ..ProcessOptions::new("printf ready; printf ready >&2; while :; do :; done")
        })
        .await
        .unwrap();
    let mut stdout = process.take_stdout().unwrap();
    let mut stderr = process.take_stderr().unwrap();
    assert_eq!(
        stdout.next().await.unwrap().unwrap(),
        Bytes::from_static(b"ready")
    );
    assert_eq!(
        stderr.next().await.unwrap().unwrap(),
        Bytes::from_static(b"ready")
    );
    // Register both read wakers before cancelling; neither pipe has more data.
    let mut output = Box::pin(futures_util::future::join(stdout.next(), stderr.next()));
    let first_poll =
        std::future::poll_fn(|cx| std::task::Poll::Ready(output.as_mut().poll(cx))).await;
    assert!(first_poll.is_pending());
    cancellation.cancel();
    let (out, err) = tokio::time::timeout(std::time::Duration::from_secs(2), output)
        .await
        .unwrap();
    assert_eq!(
        out.unwrap().unwrap_err().kind(),
        std::io::ErrorKind::Interrupted
    );
    assert_eq!(
        err.unwrap().unwrap_err().kind(),
        std::io::ErrorKind::Interrupted
    );
    assert!(stdout.next().await.is_none());
    assert!(stderr.next().await.is_none());
    assert_eq!(
        process.wait().await.unwrap_err().kind(),
        std::io::ErrorKind::Interrupted
    );
    process.kill().await.unwrap();
    process.kill().await.unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
