//! Cross-platform socket transport.
//!
//! Each call opens a fresh connection, sends one request, reads one response,
//! then closes. This matches the per-call spawn_blocking pattern in the GUI.

#[cfg(unix)]
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::warn;

use crate::error::IpcError;
use crate::proto::{IpcRequest, IpcResponse};

// ── Socket path ───────────────────────────────────────────────────────────────

#[cfg(unix)]
fn socket_path() -> PathBuf {
    let dir = std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let uid = unsafe { libc::getuid() };
            PathBuf::from(format!("/run/user/{uid}"))
        });
    dir.join("backuppilot-daemon.sock")
}

#[cfg(windows)]
const PIPE_NAME: &str = r"\\.\pipe\backuppilot-daemon";

// ── IpcStream: type-erased read/write stream ──────────────────────────────────

#[cfg(unix)]
pub type IpcStream = tokio::net::UnixStream;

#[cfg(windows)]
pub type IpcStream = tokio::net::windows::named_pipe::NamedPipeClient;

// ── IpcClient ─────────────────────────────────────────────────────────────────

pub struct IpcClient;

impl IpcClient {
    /// Call a daemon method. Opens a connection, sends one request, returns the result.
    pub async fn call(method: &str, params: serde_json::Value) -> crate::Result<String> {
        let stream = Self::connect().await?;
        let req = IpcRequest {
            method: method.to_string(),
            params,
        };
        let mut line = serde_json::to_string(&req)
            .map_err(|e| IpcError::Protocol(e.to_string()))?;
        line.push('\n');

        #[cfg(unix)]
        let (reader, mut writer) = stream.into_split();
        #[cfg(windows)]
        let (reader, mut writer) = tokio::io::split(stream);

        writer.write_all(line.as_bytes()).await?;
        writer.flush().await?;
        drop(writer);

        let mut buf_reader = BufReader::new(reader);
        let mut response_line = String::new();
        buf_reader.read_line(&mut response_line).await?;

        let response: IpcResponse = serde_json::from_str(response_line.trim())
            .map_err(|e| IpcError::Protocol(format!("malformed response: {e}")))?;

        match response {
            IpcResponse::Ok { ok } => {
                let s = match ok {
                    serde_json::Value::String(s) => s,
                    other => other.to_string(),
                };
                Ok(s)
            }
            IpcResponse::Err { err } => Err(IpcError::Failure(err)),
        }
    }

    #[cfg(unix)]
    async fn connect() -> crate::Result<IpcStream> {
        let path = socket_path();
        tokio::net::UnixStream::connect(&path)
            .await
            .map_err(|e| IpcError::Connect(format!("{}: {e}", path.display())))
    }

    #[cfg(windows)]
    async fn connect() -> crate::Result<IpcStream> {
        use tokio::net::windows::named_pipe::ClientOptions;
        ClientOptions::new()
            .open(PIPE_NAME)
            .map_err(|e| IpcError::Connect(format!("{PIPE_NAME}: {e}")))
    }
}

// ── IpcServer ─────────────────────────────────────────────────────────────────

pub struct IpcServer {
    #[cfg(unix)]
    listener: tokio::net::UnixListener,
    #[cfg(windows)]
    pipe_name: String,
    #[cfg(windows)]
    first_instance: Option<tokio::net::windows::named_pipe::NamedPipeServer>,
}

impl IpcServer {
    /// Bind to the platform socket. Removes a stale Unix socket file if needed.
    pub fn bind() -> crate::Result<Self> {
        #[cfg(unix)]
        {
            let path = socket_path();
            if path.exists() {
                let _ = std::fs::remove_file(&path);
            }
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let listener = tokio::net::UnixListener::bind(&path)
                .map_err(|e| IpcError::Connect(format!("bind {}: {e}", path.display())))?;
            return Ok(Self { listener });
        }
        #[cfg(windows)]
        {
            use tokio::net::windows::named_pipe::ServerOptions;
            // first_pipe_instance(true) fails if another server already owns the pipe.
            let first = ServerOptions::new()
                .first_pipe_instance(true)
                .create(PIPE_NAME)
                .map_err(|e| IpcError::Connect(format!("pipe already in use: {e}")))?;
            return Ok(Self { pipe_name: PIPE_NAME.to_string(), first_instance: Some(first) });
        }
    }

    /// Accept connections in a loop, dispatching each to `handler`.
    /// `handler(method, params)` returns `Ok(json_string)` or `Err(message)`.
    pub async fn serve<H, Fut>(self, handler: H)
    where
        H: Fn(String, serde_json::Value) -> Fut + Send + Sync + 'static + Clone,
        Fut: std::future::Future<Output = Result<String, String>> + Send + 'static,
    {
        #[cfg(unix)]
        self.serve_unix(handler).await;
        #[cfg(windows)]
        self.serve_windows(handler).await;
    }

    #[cfg(unix)]
    async fn serve_unix<H, Fut>(self, handler: H)
    where
        H: Fn(String, serde_json::Value) -> Fut + Send + Sync + 'static + Clone,
        Fut: std::future::Future<Output = Result<String, String>> + Send + 'static,
    {
        loop {
            match self.listener.accept().await {
                Ok((stream, _)) => {
                    let h = handler.clone();
                    tokio::spawn(handle_unix(stream, h));
                }
                Err(e) => {
                    warn!("IPC accept error: {e}");
                }
            }
        }
    }

    #[cfg(windows)]
    async fn serve_windows<H, Fut>(self, handler: H)
    where
        H: Fn(String, serde_json::Value) -> Fut + Send + Sync + 'static + Clone,
        Fut: std::future::Future<Output = Result<String, String>> + Send + 'static,
    {
        use tokio::net::windows::named_pipe::ServerOptions;

        // Accept a connection on an already-created pipe instance, then immediately
        // create the next one so we're always ready for the next client.
        let mut current = self.first_instance.expect("first_instance must be set");
        loop {
            if let Err(e) = current.connect().await {
                warn!("IPC named pipe connect error: {e}");
                break;
            }
            // Create the next instance before spawning the handler so we never
            // miss a connection.
            let next = match ServerOptions::new().create(&self.pipe_name) {
                Ok(s) => s,
                Err(e) => {
                    warn!("IPC named pipe create error: {e}");
                    break;
                }
            };
            let h = handler.clone();
            tokio::spawn(handle_windows(current, h));
            current = next;
        }
    }
}

#[cfg(unix)]
async fn handle_unix<H, Fut>(stream: tokio::net::UnixStream, handler: H)
where
    H: Fn(String, serde_json::Value) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<String, String>> + Send + 'static,
{
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    if let Ok(Some(line)) = lines.next_line().await {
        let reply = dispatch_line(&line, &handler).await;
        let _ = writer.write_all(reply.as_bytes()).await;
        let _ = writer.flush().await;
    }
}

#[cfg(windows)]
async fn handle_windows<H, Fut>(
    pipe: tokio::net::windows::named_pipe::NamedPipeServer,
    handler: H,
) where
    H: Fn(String, serde_json::Value) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<String, String>> + Send + 'static,
{
    let (reader, mut writer) = tokio::io::split(pipe);
    let mut lines = BufReader::new(reader).lines();
    if let Ok(Some(line)) = lines.next_line().await {
        let reply = dispatch_line(&line, &handler).await;
        let _ = writer.write_all(reply.as_bytes()).await;
        let _ = writer.flush().await;
    }
}

async fn dispatch_line<H, Fut>(line: &str, handler: &H) -> String
where
    H: Fn(String, serde_json::Value) -> Fut,
    Fut: std::future::Future<Output = Result<String, String>>,
{
    let req: IpcRequest = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(e) => {
            let resp = IpcResponse::Err { err: format!("parse error: {e}") };
            let mut s = serde_json::to_string(&resp).unwrap_or_default();
            s.push('\n');
            return s;
        }
    };

    let result = handler(req.method, req.params).await;
    let resp = match result {
        Ok(json_str) => {
            let val: serde_json::Value = serde_json::from_str(&json_str)
                .unwrap_or(serde_json::Value::String(json_str));
            IpcResponse::Ok { ok: val }
        }
        Err(msg) => IpcResponse::Err { err: msg },
    };
    let mut s = serde_json::to_string(&resp).unwrap_or_default();
    s.push('\n');
    s
}
