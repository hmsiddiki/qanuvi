//! Where events of a running thing (an agent session, a terminal) go: the desktop window or a remote connection.

use std::sync::Arc;

/// Nobody listens to the events any more.
#[derive(Debug)]
pub struct Closed;

/// Sends events somewhere. `send` answers false when nobody listens any more.
pub struct Sink<T>(Arc<dyn Fn(T) -> bool + Send + Sync>);

impl<T> Clone for Sink<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> Sink<T> {
    pub fn new(f: impl Fn(T) -> bool + Send + Sync + 'static) -> Self {
        Self(Arc::new(f))
    }

    pub fn send(&self, event: T) -> Result<(), Closed> {
        if (self.0)(event) { Ok(()) } else { Err(Closed) }
    }
}

#[cfg(feature = "desktop")]
impl<T: tauri::ipc::IpcResponse + Clone + Send + Sync + 'static> From<tauri::ipc::Channel<T>>
    for Sink<T>
{
    fn from(channel: tauri::ipc::Channel<T>) -> Self {
        Sink::new(move |e| channel.send(e).is_ok())
    }
}
