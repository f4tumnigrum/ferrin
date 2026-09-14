//! Sandbox abstraction (feature `sandbox`): a session that reads and writes
//! files and runs commands on behalf of tools.
//!
//! Only the trait and [`LocalProcessSandbox`] (which offers **no isolation**
//! and exists for tests and examples) live here; real sandboxes are provided
//! by applications or separate crates.

mod local;

use std::collections::BTreeMap;
use std::io;

use bytes::Bytes;
use ferrin_spec::BoxFuture;
use ferrin_spec::BoxStream;
pub use local::LocalProcessSandbox;
use tokio_util::sync::CancellationToken;

/// Byte stream of file or process output.
pub type ByteStream = BoxStream<'static, io::Result<Bytes>>;

/// Options for reading a file.
#[derive(Debug, Clone)]
pub struct ReadFileOptions {
    /// Path inside the sandbox.
    pub path: String,
    /// Cancels the read.
    pub cancellation: CancellationToken,
}

impl ReadFileOptions {
    /// Reads `path`.
    #[must_use]
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            cancellation: CancellationToken::new(),
        }
    }
}

/// Options for reading a text file.
#[derive(Debug, Clone)]
pub struct ReadTextFileOptions {
    /// Path inside the sandbox.
    pub path: String,
    /// Text encoding (`utf-8` when absent).
    pub encoding: Option<String>,
    /// 1-based inclusive first line.
    pub start_line: Option<usize>,
    /// 1-based inclusive last line; past the end reads through EOF.
    pub end_line: Option<usize>,
    /// Cancels the read.
    pub cancellation: CancellationToken,
}

impl ReadTextFileOptions {
    /// Reads all of `path`.
    #[must_use]
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            encoding: None,
            start_line: None,
            end_line: None,
            cancellation: CancellationToken::new(),
        }
    }

    /// Restricts to a line range.
    #[must_use]
    pub fn lines(mut self, start_line: usize, end_line: usize) -> Self {
        self.start_line = Some(start_line);
        self.end_line = Some(end_line);
        self
    }
}

/// Options for writing a file; `C` is the payload type.
#[derive(Debug)]
pub struct WriteFileOptions<C> {
    /// Path inside the sandbox.
    pub path: String,
    /// Content to write.
    pub content: C,
    /// Cancels the write.
    pub cancellation: CancellationToken,
}

impl<C> WriteFileOptions<C> {
    /// Writes `content` to `path`.
    #[must_use]
    pub fn new(path: impl Into<String>, content: C) -> Self {
        Self {
            path: path.into(),
            content,
            cancellation: CancellationToken::new(),
        }
    }
}

/// Options for running a command.
#[derive(Debug, Clone)]
pub struct ProcessOptions {
    /// Shell command line.
    pub command: String,
    /// Working directory inside the sandbox.
    pub working_directory: Option<String>,
    /// Extra environment variables (override the sandbox defaults).
    pub env: BTreeMap<String, String>,
    /// Kills the process when triggered.
    pub cancellation: CancellationToken,
}

impl ProcessOptions {
    /// Runs `command`.
    #[must_use]
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            working_directory: None,
            env: BTreeMap::new(),
            cancellation: CancellationToken::new(),
        }
    }

    /// Sets the working directory.
    #[must_use]
    pub fn in_directory(mut self, directory: impl Into<String>) -> Self {
        self.working_directory = Some(directory.into());
        self
    }

    /// Adds an environment variable.
    #[must_use]
    pub fn env(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(name.into(), value.into());
        self
    }
}

/// Result of a completed command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessResult {
    /// Exit code (`-1` when the process was killed by a signal).
    pub exit_code: i32,
    /// Captured standard output.
    pub stdout: String,
    /// Captured standard error.
    pub stderr: String,
}

/// A running process started with [`Sandbox::spawn`].
///
/// Implementations must make `kill` idempotent and let `wait` be called
/// after the output streams were taken.
pub trait SandboxProcess: Send {
    /// Process id, when known.
    fn pid(&self) -> Option<u32>;
    /// Takes the standard output stream (once).
    fn take_stdout(&mut self) -> Option<ByteStream>;
    /// Takes the standard error stream (once).
    fn take_stderr(&mut self) -> Option<ByteStream>;
    /// Waits for exit, returning the exit code.
    fn wait(&mut self) -> BoxFuture<'_, io::Result<i32>>;
    /// Terminates the process.
    fn kill(&mut self) -> BoxFuture<'_, io::Result<()>>;
}

/// A sandbox session.
///
/// Implement this to run tools inside a container, VM or remote worker.
/// Reads return `Ok(None)` for missing files; writes create parent
/// directories and overwrite; `run` is `spawn` plus collecting both output
/// streams.
pub trait Sandbox: Send + Sync {
    /// Human-readable description added to agent instructions.
    fn description(&self) -> &str;
    /// Streams a file's bytes.
    fn read_file(&self, options: ReadFileOptions) -> BoxFuture<'_, io::Result<Option<ByteStream>>>;
    /// Reads a file into memory.
    fn read_binary_file(
        &self,
        options: ReadFileOptions,
    ) -> BoxFuture<'_, io::Result<Option<Bytes>>>;
    /// Reads a text file, optionally a line range.
    fn read_text_file(
        &self,
        options: ReadTextFileOptions,
    ) -> BoxFuture<'_, io::Result<Option<String>>>;
    /// Writes a file from a byte stream.
    fn write_file(&self, options: WriteFileOptions<ByteStream>) -> BoxFuture<'_, io::Result<()>>;
    /// Writes a file from bytes.
    fn write_binary_file(&self, options: WriteFileOptions<Bytes>) -> BoxFuture<'_, io::Result<()>>;
    /// Writes a text file.
    fn write_text_file(&self, options: WriteFileOptions<String>) -> BoxFuture<'_, io::Result<()>>;
    /// Starts a process.
    fn spawn(&self, options: ProcessOptions) -> BoxFuture<'_, io::Result<Box<dyn SandboxProcess>>>;
    /// Runs a command to completion.
    fn run(&self, options: ProcessOptions) -> BoxFuture<'_, io::Result<ProcessResult>>;
}
