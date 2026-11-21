use std::future::Future;

use rusqlite::Connection;

use ledger_infra::db::DbWriteHandle;
use ledger_infra::error::Result;

pub trait ScopedSession {
    fn with_connection<R, F>(&self, use_connection: F) -> impl Future<Output = Result<R>> + Send
    where
        F: FnOnce(&Connection) -> Result<R> + Send + 'static,
        R: Send + 'static;
}

pub struct FacadeWriteSession {
    handle: DbWriteHandle,
    span: &'static str,
}

impl FacadeWriteSession {
    pub fn new(handle: DbWriteHandle, span: &'static str) -> Self {
        Self { handle, span }
    }
}

impl ScopedSession for FacadeWriteSession {
    fn with_connection<R, F>(&self, use_connection: F) -> impl Future<Output = Result<R>> + Send
    where
        F: FnOnce(&Connection) -> Result<R> + Send + 'static,
        R: Send + 'static,
    {
        self.handle.run_raw(self.span, use_connection)
    }
}
