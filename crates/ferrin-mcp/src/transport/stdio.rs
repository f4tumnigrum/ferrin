//! Stdio transport: a child process exchanging newline-delimited JSON-RPC
//! messages on its standard streams.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::Mutex;

use ferrin_provider_util::settings::env_var;
use futures_util::future::BoxFuture;
use futures_util::stream::BoxStream;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::process::Child;
use tokio::process::ChildStdin;
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use super::CloseOptions;
use super::EventChannel;
use super::McpTransport;
use super::SendOptions;
use super::TransportCapabilities;
use super::TransportEvent;
use super::lock;
use super::with_cancellation;
use crate::error::McpError;
use crate::protocol::JsonRpcMessage;

/// Environment variables passed to the child by default.
#[cfg(windows)]
pub const DEFAULT_INHERITED_ENV_VARS: &[&str] = &[
    "APPDATA",
    "HOMEDRIVE",
    "HOMEPATH",
    "LOCALAPPDATA",
    "PATH",
    "PROCESSOR_ARCHITECTURE",
    "SYSTEMDRIVE",
    "SYSTEMROOT",
    "TEMP",
    "USERNAME",
    "USERPROFILE",
];

/// Environment variables passed to the child by default.
#[cfg(not(windows))]
pub const DEFAULT_INHERITED_ENV_VARS: &[&str] =
    &["HOME", "LOGNAME", "PATH", "SHELL", "TERM", "USER"];

/// Where the child's standard error goes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum StdioStderr {
    /// Inherit the parent's standard error (default).
    #[default]
    Inherit,
    /// Discard.
    Null,
}

/// Configuration of [`StdioTransport`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct StdioConfig {
    /// Executable to run.
    pub command: String,
    /// Arguments.
    pub args: Vec<String>,
    /// Extra environment variables.
    pub env: BTreeMap<String, String>,
    /// Working directory.
    pub cwd: Option<PathBuf>,
    /// Standard error handling.
    pub stderr: StdioStderr,
    /// Whether [`DEFAULT_INHERITED_ENV_VARS`] are copied from the parent
    /// (default `true`).
    pub inherit_default_env: bool,
}

impl StdioConfig {
    /// Configuration running `command` without arguments.
    #[must_use]
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            args: Vec::new(),
            env: BTreeMap::new(),
            cwd: None,
            stderr: StdioStderr::Inherit,
            inherit_default_env: true,
        }
    }

    /// Sets the arguments.
    #[must_use]
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }

    /// Adds an environment variable.
    #[must_use]
    pub fn env(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(name.into(), value.into());
        self
    }

    /// Sets the working directory.
    #[must_use]
    pub fn cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Sets the standard error handling.
    #[must_use]
    pub fn stderr(mut self, stderr: StdioStderr) -> Self {
        self.stderr = stderr;
        self
    }

    /// Controls whether the default environment variables are inherited.
    #[must_use]
    pub fn inherit_default_env(mut self, inherit: bool) -> Self {
        self.inherit_default_env = inherit;
        self
    }

    fn command(&self) -> Result<Command, McpError> {
        if cfg!(windows)
            && std::iter::once(&self.command)
                .chain(&self.args)
                .any(|value| value.contains(['\r', '\n']))
        {
            return Err(McpError::invalid_argument(
                "line breaks are not allowed in the command or its arguments on Windows",
            ));
        }
        let mut command = Command::new(&self.command);
        command.args(&self.args);
        command.env_clear();
        if self.inherit_default_env {
            for name in DEFAULT_INHERITED_ENV_VARS {
                // Values starting with `()` are shell functions exported by
                // some shells and are never meaningful for a child.
                if let Some(value) = env_var(name)
                    && !value.starts_with("()")
                {
                    command.env(name, value);
                }
            }
        }
        command.envs(&self.env);
        if let Some(cwd) = &self.cwd {
            command.current_dir(cwd);
        }
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(match self.stderr {
                StdioStderr::Inherit => Stdio::inherit(),
                StdioStderr::Null => Stdio::null(),
                #[allow(unreachable_patterns, reason = "StdioStderr is non-exhaustive")]
                _ => Stdio::inherit(),
            })
            .kill_on_drop(true);
        #[cfg(windows)]
        {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        Ok(command)
    }
}

#[derive(Debug, Default)]
struct State {
    started: bool,
    closed: bool,
    protocol_version: Option<String>,
}

/// One outgoing frame and the channel reporting its outcome.
type Frame = (String, oneshot::Sender<std::io::Result<()>>);

struct Inner {
    config: StdioConfig,
    state: Mutex<State>,
    /// Frames are written by a single task so concurrent messages never
    /// interleave.
    frames: Mutex<Option<mpsc::UnboundedSender<Frame>>>,
    child: Mutex<Option<Child>>,
    events: EventChannel,
    cancellation: CancellationToken,
    tasks: Mutex<JoinSet<()>>,
}

impl std::fmt::Debug for Inner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StdioTransport")
            .field("config", &self.config)
            .field("state", &lock(&self.state))
            .finish_non_exhaustive()
    }
}

impl Inner {
    async fn read_lines(self: Arc<Self>, stdout: tokio::process::ChildStdout) {
        let mut lines = BufReader::new(stdout).lines();
        loop {
            let next = tokio::select! {
                () = self.cancellation.cancelled() => return,
                next = lines.next_line() => next,
            };
            match next {
                Ok(Some(line)) => {
                    if line.trim().is_empty() {
                        continue;
                    }
                    match JsonRpcMessage::parse(&line) {
                        Ok(message) => self.events.emit(TransportEvent::Message(message)),
                        Err(error) => self.events.emit(TransportEvent::Error(error)),
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    self.events.emit(TransportEvent::Error(McpError::Io(error)));
                    break;
                }
            }
        }
        lock(&self.state).closed = true;
        lock(&self.frames).take();
        self.events.emit(TransportEvent::Closed);
    }

    async fn write_frames(mut stdin: ChildStdin, mut frames: mpsc::UnboundedReceiver<Frame>) {
        while let Some((text, done)) = frames.recv().await {
            let result = async {
                stdin.write_all(text.as_bytes()).await?;
                stdin.flush().await
            }
            .await;
            let _ = done.send(result);
        }
        let _ = stdin.shutdown().await;
    }

    async fn write(&self, message: JsonRpcMessage) -> Result<(), McpError> {
        let mut text = message.to_json_string();
        text.push('\n');
        let (done, outcome) = oneshot::channel();
        {
            let frames = lock(&self.frames);
            let Some(sender) = frames.as_ref() else {
                return Err(McpError::Closed);
            };
            if sender.send((text, done)).is_err() {
                return Err(McpError::Closed);
            }
        }
        match outcome.await {
            Ok(result) => result.map_err(McpError::Io),
            Err(_) => Err(McpError::Closed),
        }
    }
}

/// Stdio transport.
#[derive(Debug)]
pub struct StdioTransport {
    inner: Arc<Inner>,
}

impl StdioTransport {
    /// Creates the transport; the process is spawned by `start`.
    #[must_use]
    pub fn new(config: StdioConfig) -> Self {
        Self {
            inner: Arc::new(Inner {
                config,
                state: Mutex::new(State::default()),
                frames: Mutex::new(None),
                child: Mutex::new(None),
                events: EventChannel::new(),
                cancellation: CancellationToken::new(),
                tasks: Mutex::new(JoinSet::new()),
            }),
        }
    }

    /// Process id of the running child.
    #[must_use]
    pub fn pid(&self) -> Option<u32> {
        lock(&self.inner.child).as_ref().and_then(Child::id)
    }
}

impl McpTransport for StdioTransport {
    fn start(&self) -> BoxFuture<'_, Result<(), McpError>> {
        Box::pin(async move {
            let inner = &self.inner;
            if lock(&inner.state).started {
                return Err(McpError::transport("transport is already started"));
            }
            let mut child = inner.config.command()?.spawn()?;
            let stdin = child
                .stdin
                .take()
                .ok_or_else(|| McpError::transport("child process has no stdin"))?;
            let stdout = child
                .stdout
                .take()
                .ok_or_else(|| McpError::transport("child process has no stdout"))?;
            let (sender, receiver) = mpsc::unbounded_channel();
            *lock(&inner.frames) = Some(sender);
            *lock(&inner.child) = Some(child);
            lock(&inner.state).started = true;
            let mut tasks = lock(&inner.tasks);
            tasks.spawn(Inner::write_frames(stdin, receiver));
            tasks.spawn(Arc::clone(inner).read_lines(stdout));
            Ok(())
        })
    }

    fn send(
        &self,
        message: JsonRpcMessage,
        options: SendOptions,
    ) -> BoxFuture<'_, Result<(), McpError>> {
        Box::pin(async move {
            if !lock(&self.inner.state).started {
                return Err(McpError::transport("transport has not been started"));
            }
            with_cancellation(options.cancellation.as_ref(), self.inner.write(message)).await
        })
    }

    fn incoming(&self) -> BoxStream<'static, TransportEvent> {
        self.inner.events.take()
    }

    fn close(&self, options: CloseOptions) -> BoxFuture<'_, Result<(), McpError>> {
        Box::pin(async move {
            let inner = &self.inner;
            {
                let mut state = lock(&inner.state);
                if state.closed {
                    return Ok(());
                }
                state.closed = true;
            }
            inner.cancellation.cancel();
            lock(&inner.frames).take();
            lock(&inner.tasks).abort_all();
            let child = lock(&inner.child).take();
            if let Some(mut child) = child {
                // `InvalidInput` means the child already exited.
                let _ = child.start_kill();
                let _ = with_cancellation(options.cancellation.as_ref(), async {
                    child.wait().await.map_err(McpError::Io)
                })
                .await;
            }
            inner.events.emit(TransportEvent::Closed);
            Ok(())
        })
    }

    fn protocol_version(&self) -> Option<String> {
        lock(&self.inner.state).protocol_version.clone()
    }

    fn set_protocol_version(&self, version: Option<&str>) {
        lock(&self.inner.state).protocol_version = version.map(str::to_owned);
    }

    fn capabilities(&self) -> TransportCapabilities {
        TransportCapabilities {
            supports_protocol_version_discovery: true,
            supports_tool_parameter_headers: false,
        }
    }
}
