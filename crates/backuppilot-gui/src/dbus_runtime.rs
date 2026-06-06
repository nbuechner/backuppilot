//! Run IPC (tokio) work off the GTK main thread and deliver results on the main loop.

use std::future::Future;

use gtk::gio;
use gtk::glib;
use backuppilot_ipc::IpcError;

pub fn spawn<F, T>(future: F, on_result: impl FnOnce(Result<T, IpcError>) + 'static)
where
    F: Future<Output = Result<T, IpcError>> + Send + 'static,
    T: Send + 'static,
{
    crate::debug::log_dbus("spawn", "task");
    glib::spawn_future_local(async move {
        let join = gio::spawn_blocking(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime for IPC");
            rt.block_on(future)
        });

        let result = match join.await {
            Ok(result) => result,
            Err(err) => {
                tracing::warn!(?err, "IPC background task failed");
                Err(IpcError::failure(format!("background task failed: {err:?}")))
            }
        };

        match &result {
            Ok(_) => crate::debug::log_dbus("done", "ok"),
            Err(err) => crate::debug::log_dbus("done", err.to_string()),
        }
        on_result(result);
    });
}
