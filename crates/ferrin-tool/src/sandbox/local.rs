//! [`LocalProcessSandbox`]: runs on the host without isolation.

use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use bytes::Bytes;
use ferrin_spec::BoxFuture;
use futures_util::StreamExt;
use futures_util::future::Either;
use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::process::Child;
use tokio::process::Command;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use super::ByteStream;
use super::ProcessOptions;
use super::ProcessResult;
use super::ReadFileOptions;
use super::ReadTextFileOptions;
use super::Sandbox;
use super::SandboxProcess;
use super::WriteFileOptions;

/// A [`Sandbox`] backed by the local file system and shell.
///
/// **Provides no isolation.** Paths resolve relative to `root`, but absolute
/// paths and `..` are not blocked. Use only in tests and examples.
#[derive(Debug, Clone)]
pub struct LocalProcessSandbox {
    root: PathBuf,
    shell: Vec<String>,
    description: String,
}

impl LocalProcessSandbox {
    /// Uses `root` as the working directory and the platform shell
    /// (`/bin/sh -c` or `cmd /C`).
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let shell = if cfg!(windows) {
            vec!["cmd".to_owned(), "/C".to_owned()]
        } else {
            vec!["/bin/sh".to_owned(), "-c".to_owned()]
        };
        Self {
            description: format!(
                "Local process sandbox (no isolation). Root directory: {}",
                root.display()
            ),
            root,
            shell,
        }
    }

    /// Overrides the shell used to run commands (program followed by the
    /// arguments that precede the command line).
    #[must_use]
    pub fn with_shell(
        mut self,
        program: impl Into<String>,
        args: impl IntoIterator<Item = String>,
    ) -> Self {
        self.shell = std::iter::once(program.into()).chain(args).collect();
        self
    }

    /// Overrides the description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// The root directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn resolve(&self, path: &str) -> PathBuf {
        self.root.join(path)
    }

    fn command(&self, options: &ProcessOptions) -> Command {
        let mut command = Command::new(&self.shell[0]);
        command.args(&self.shell[1..]).arg(&options.command);
        let directory = options
            .working_directory
            .as_deref()
            .map_or_else(|| self.root.clone(), |dir| self.resolve(dir));
        command
            .current_dir(directory)
            .envs(&options.env)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        command
    }
}

fn reader_stream(
    reader: impl AsyncRead + Send + Unpin + 'static,
    cancellation: CancellationToken,
) -> ByteStream {
    Box::pin(futures_util::stream::unfold(
        Some((reader, cancellation)),
        |state| async move {
            let (mut reader, cancellation) = state?;
            let mut buffer = vec![0u8; 8 * 1024];
            match with_cancellation(&cancellation, reader.read(&mut buffer)).await {
                Ok(0) => None,
                Ok(read) => {
                    buffer.truncate(read);
                    Some((Ok(Bytes::from(buffer)), Some((reader, cancellation))))
                }
                Err(error) => Some((Err(error), None)),
            }
        },
    ))
}

fn cancelled() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "cancelled")
}

async fn with_cancellation<T>(
    cancellation: &CancellationToken,
    future: impl Future<Output = io::Result<T>>,
) -> io::Result<T> {
    match futures_util::future::select(Box::pin(cancellation.cancelled()), Box::pin(future)).await {
        Either::Left(((), _)) => Err(cancelled()),
        Either::Right((result, _)) => result,
    }
}

fn decode_text(bytes: &[u8], encoding: Option<&str>) -> io::Result<String> {
    match encoding.map(str::to_ascii_lowercase).as_deref() {
        None | Some("utf-8" | "utf8") => Ok(String::from_utf8_lossy(bytes).into_owned()),
        Some(other) => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("unsupported text encoding \"{other}\""),
        )),
    }
}

fn select_lines(text: &str, start_line: Option<usize>, end_line: Option<usize>) -> String {
    if start_line.is_none() && end_line.is_none() {
        return text.to_owned();
    }
    let start = start_line.unwrap_or(1).max(1);
    let lines: Vec<&str> = text.split('\n').collect();
    let end = end_line.unwrap_or(lines.len()).min(lines.len());
    if start > end {
        return String::new();
    }
    lines[start - 1..end].join("\n")
}

async fn open_optional(path: &Path) -> io::Result<Option<tokio::fs::File>> {
    match tokio::fs::File::open(path).await {
        Ok(file) => Ok(Some(file)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

async fn create_with_parents(path: &Path) -> io::Result<tokio::fs::File> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::File::create(path).await
}

struct LocalProcess {
    pid: Option<u32>,
    tasks: JoinSet<io::Result<i32>>,
    completion: Option<Result<i32, Arc<io::Error>>>,
    stdout: Option<ByteStream>,
    stderr: Option<ByteStream>,
    kill: CancellationToken,
}

async fn supervise_process(
    mut child: Child,
    cancellation: CancellationToken,
    kill: CancellationToken,
) -> io::Result<i32> {
    let waited = with_cancellation(&cancellation, with_cancellation(&kill, child.wait())).await;
    match waited {
        Ok(status) => Ok(status.code().unwrap_or(-1)),
        Err(error) if error.kind() == io::ErrorKind::Interrupted => {
            match child.kill().await {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::InvalidInput => {}
                Err(error) => return Err(error),
            }
            if cancellation.is_cancelled() {
                Err(error)
            } else {
                child.wait().await.map(|status| status.code().unwrap_or(-1))
            }
        }
        Err(error) => Err(error),
    }
}

fn process_result(result: &Result<i32, Arc<io::Error>>) -> io::Result<i32> {
    result
        .clone()
        .map_err(|error| io::Error::new(error.kind(), error))
}

impl SandboxProcess for LocalProcess {
    fn pid(&self) -> Option<u32> {
        self.pid
    }

    fn take_stdout(&mut self) -> Option<ByteStream> {
        self.stdout.take()
    }

    fn take_stderr(&mut self) -> Option<ByteStream> {
        self.stderr.take()
    }

    fn wait(&mut self) -> BoxFuture<'_, io::Result<i32>> {
        Box::pin(async move {
            if let Some(completion) = &self.completion {
                return process_result(completion);
            }
            let result = match self.tasks.join_next().await {
                Some(Ok(result)) => result,
                Some(Err(error)) => Err(io::Error::other(error)),
                None => Err(io::Error::other(
                    "process supervisor ended without a result",
                )),
            };
            self.pid = None;
            let completion = result.map_err(Arc::new);
            let result = process_result(&completion);
            self.completion = Some(completion);
            result
        })
    }

    fn kill(&mut self) -> BoxFuture<'_, io::Result<()>> {
        Box::pin(async move {
            self.kill.cancel();
            match self.wait().await {
                Ok(_) => Ok(()),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => Ok(()),
                Err(error) => Err(error),
            }
        })
    }
}

impl Sandbox for LocalProcessSandbox {
    fn description(&self) -> &str {
        &self.description
    }

    fn read_file(&self, options: ReadFileOptions) -> BoxFuture<'_, io::Result<Option<ByteStream>>> {
        Box::pin(async move {
            let path = self.resolve(&options.path);
            let file = with_cancellation(&options.cancellation, open_optional(&path)).await?;
            Ok(file.map(|file| reader_stream(file, options.cancellation)))
        })
    }

    fn read_binary_file(
        &self,
        options: ReadFileOptions,
    ) -> BoxFuture<'_, io::Result<Option<Bytes>>> {
        Box::pin(async move {
            let path = self.resolve(&options.path);
            with_cancellation(&options.cancellation, async {
                match tokio::fs::read(&path).await {
                    Ok(bytes) => Ok(Some(Bytes::from(bytes))),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
                    Err(error) => Err(error),
                }
            })
            .await
        })
    }

    fn read_text_file(
        &self,
        options: ReadTextFileOptions,
    ) -> BoxFuture<'_, io::Result<Option<String>>> {
        Box::pin(async move {
            let bytes = self
                .read_binary_file(ReadFileOptions {
                    path: options.path,
                    cancellation: options.cancellation,
                })
                .await?;
            let Some(bytes) = bytes else {
                return Ok(None);
            };
            let text = decode_text(&bytes, options.encoding.as_deref())?;
            Ok(Some(select_lines(
                &text,
                options.start_line,
                options.end_line,
            )))
        })
    }

    fn write_file(&self, options: WriteFileOptions<ByteStream>) -> BoxFuture<'_, io::Result<()>> {
        Box::pin(async move {
            let path = self.resolve(&options.path);
            let mut content = options.content;
            with_cancellation(&options.cancellation, async {
                let mut file = create_with_parents(&path).await?;
                while let Some(chunk) = content.next().await {
                    file.write_all(&chunk?).await?;
                }
                file.flush().await
            })
            .await
        })
    }

    fn write_binary_file(&self, options: WriteFileOptions<Bytes>) -> BoxFuture<'_, io::Result<()>> {
        Box::pin(async move {
            let path = self.resolve(&options.path);
            with_cancellation(&options.cancellation, async {
                let mut file = create_with_parents(&path).await?;
                file.write_all(&options.content).await?;
                file.flush().await
            })
            .await
        })
    }

    fn write_text_file(&self, options: WriteFileOptions<String>) -> BoxFuture<'_, io::Result<()>> {
        self.write_binary_file(WriteFileOptions {
            path: options.path,
            content: Bytes::from(options.content),
            cancellation: options.cancellation,
        })
    }

    fn spawn(&self, options: ProcessOptions) -> BoxFuture<'_, io::Result<Box<dyn SandboxProcess>>> {
        Box::pin(async move {
            if options.cancellation.is_cancelled() {
                return Err(cancelled());
            }
            let mut child = self.command(&options).spawn()?;
            let pid = child.id();
            let stdout = child
                .stdout
                .take()
                .map(|reader| reader_stream(reader, options.cancellation.clone()));
            let stderr = child
                .stderr
                .take()
                .map(|reader| reader_stream(reader, options.cancellation.clone()));
            let kill = CancellationToken::new();
            let mut tasks = JoinSet::new();
            tasks.spawn(supervise_process(child, options.cancellation, kill.clone()));
            Ok(Box::new(LocalProcess {
                pid,
                tasks,
                completion: None,
                stdout,
                stderr,
                kill,
            }) as Box<dyn SandboxProcess>)
        })
    }

    fn run(&self, options: ProcessOptions) -> BoxFuture<'_, io::Result<ProcessResult>> {
        Box::pin(async move {
            let cancellation = options.cancellation.clone();
            if cancellation.is_cancelled() {
                return Err(cancelled());
            }
            let mut child = self.command(&options).spawn()?;
            let output = with_cancellation(&cancellation, async {
                let stdout = child.stdout.take();
                let stderr = child.stderr.take();
                let (stdout, stderr) =
                    futures_util::future::join(read_all(stdout), read_all(stderr)).await;
                let status = child.wait().await?;
                Ok(ProcessResult {
                    exit_code: status.code().unwrap_or(-1),
                    stdout: String::from_utf8_lossy(&stdout?).into_owned(),
                    stderr: String::from_utf8_lossy(&stderr?).into_owned(),
                })
            })
            .await;
            if output
                .as_ref()
                .is_err_and(|error| error.kind() == io::ErrorKind::Interrupted)
            {
                let _ = child.kill().await;
            }
            output
        })
    }
}

async fn read_all(reader: Option<impl AsyncRead + Unpin>) -> io::Result<Vec<u8>> {
    let mut buffer = Vec::new();
    if let Some(mut reader) = reader {
        reader.read_to_end(&mut buffer).await?;
    }
    Ok(buffer)
}
