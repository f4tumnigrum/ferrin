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
