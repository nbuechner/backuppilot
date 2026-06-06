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
            // security_descriptor with "D:(A;;GA;;;WD)" allows Everyone (World) full access so
            // the pipe is reachable from any logon session (interactive, SSH, Task Scheduler…).
            let mut opts = ServerOptions::new();
            opts.first_pipe_instance(true);
            // Create the pipe with a permissive DACL so it's reachable from any logon
            // session (interactive, SSH, Task Scheduler each have separate sessions).
            #[allow(unsafe_code)]
            let first = unsafe {
                use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
                let sd = pipe_security_descriptor_allow_all()
                    .unwrap_or(core::ptr::null_mut());
                let mut sa = SECURITY_ATTRIBUTES {
                    nLength: core::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                    lpSecurityDescriptor: sd,
                    bInheritHandle: 0,
                };
                let sa_ptr = if sd.is_null() {
                    core::ptr::null_mut()
                } else {
                    (&mut sa as *mut SECURITY_ATTRIBUTES).cast::<core::ffi::c_void>()
                };
                opts.create_with_security_attributes_raw(PIPE_NAME, sa_ptr)
                    .map_err(|e| IpcError::Connect(format!("pipe already in use: {e}")))?
            };
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

        // Pre-create a pool of waiting pipe instances so concurrent callers
        // (e.g. listProfiles + listStatuses fired simultaneously) never see
        // "all pipe instances busy" (os error 231).
        const POOL: usize = 8;
        let first = self.first_instance.expect("first_instance must be set");
        let mut pool = vec![first];
        for _ in 1..POOL {
            match ServerOptions::new().create(&self.pipe_name) {
                Ok(s) => pool.push(s),
                Err(e) => { warn!("IPC pool init error: {e}"); break; }
            }
        }

        // Each pool slot gets its own independent accept loop.
        let mut handles = Vec::new();
        for server in pool {
            let h = handler.clone();
            let name = self.pipe_name.clone();
            handles.push(tokio::spawn(async move {
                let mut current = server;
                loop {
                    if let Err(e) = current.connect().await {
                        warn!("IPC named pipe connect error: {e}");
                        break;
                    }
                    let next = match ServerOptions::new().create(&name) {
                        Ok(s) => s,
                        Err(e) => { warn!("IPC named pipe create error: {e}"); break; }
                    };
                    tokio::spawn(handle_windows(current, h.clone()));
                    current = next;
                }
            }));
        }
        for h in handles { let _ = h.await; }
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

/// On Windows: creates a SECURITY_DESCRIPTOR that grants Everyone (World) full access.
///
/// This ensures the named pipe is reachable from any logon session — interactive desktop,
/// SSH (session 0), and Task Scheduler services all use separate logon sessions, but all
/// need to connect to the same pipe.
///
/// Returns a heap-allocated security descriptor pointer (via `LocalAlloc` internally).
/// The caller is responsible for freeing it with `LocalFree`. Returns `None` on failure.
#[cfg(windows)]
#[allow(unsafe_code)]
unsafe fn pipe_security_descriptor_allow_all() -> Option<*mut core::ffi::c_void> {
    use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorA;
    // D:(A;;GA;;;WD)  — DACL: Allow GENERIC_ALL to World (Everyone)
    let sddl = b"D:(A;;GA;;;WD)\0";
    let mut sd = core::ptr::null_mut::<core::ffi::c_void>();
    let ok = ConvertStringSecurityDescriptorToSecurityDescriptorA(
        sddl.as_ptr(),
        1, // SDDL_REVISION_1
        &mut sd,
        core::ptr::null_mut(),
    );
    if ok != 0 { Some(sd) } else { None }
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
