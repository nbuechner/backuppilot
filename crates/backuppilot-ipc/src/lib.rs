//! Cross-platform IPC between the BackupPilot daemon and GUI/CLI clients.
//!
//! Protocol: one request per connection, newline-delimited JSON.
//!
//! Transport:
//!   Linux/macOS – Unix domain socket (`$XDG_RUNTIME_DIR/backuppilot-daemon.sock`)
//!   Windows     – Named pipe (`\\.\pipe\backuppilot-daemon`)
//!
//! Request:  `{"method":"list_profiles","params":null}\n`
//! Response: `{"ok":"[...]"}\n`  or  `{"err":"error message"}\n`

mod error;
mod proto;
mod transport;

pub use error::IpcError;
pub use transport::{IpcClient, IpcServer, IpcStream};

pub type Result<T> = std::result::Result<T, IpcError>;

pub use proto::{IpcRequest, IpcResponse};
