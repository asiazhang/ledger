use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use rusqlite::Connection;

use super::facade::DbFacade;
use super::job_gate::LockOutcome;
use crate::error::Result;

#[derive(Clone)]
pub struct DbSlotPair {
    write: Arc<Mutex<Connection>>,
    read: Arc<Mutex<Connection>>,
}

impl DbSlotPair {
    pub fn new(write: Arc<Mutex<Connection>>, read: Arc<Mutex<Connection>>) -> Self {
        DbSlotPair { write, read }
    }

    pub fn write_handle(&self) -> DbWriteHandle {
        DbWriteHandle(self.clone())
    }

    pub fn read_handle(&self) -> DbReadHandle {
        DbReadHandle(self.clone())
    }

    pub fn write_slot(&self) -> &Arc<Mutex<Connection>> {
        &self.write
    }

    fn facade(&self) -> Result<Arc<DbFacade>> {
        facade_for(self)
    }

    fn registry_key(&self) -> usize {
        Arc::as_ptr(&self.write) as usize
    }
}

#[derive(Clone)]
pub struct DbWriteHandle(DbSlotPair);

#[derive(Clone)]
pub struct DbReadHandle(DbSlotPair);

impl DbWriteHandle {
    pub async fn run<T, F>(&self, command: &'static str, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
    {
        self.0.facade()?.run_write(command, f).await
    }

    pub async fn run_raw<T, F>(&self, command: &'static str, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
    {
        self.0.facade()?.run_write_raw(command, f).await
    }

    pub fn run_blocking<T, F>(&self, command: &'static str, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
    {
        self.0.facade()?.run_write_blocking(command, f)
    }

    pub fn run_raw_blocking<T, F>(&self, command: &'static str, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
    {
        self.0.facade()?.run_write_raw_blocking(command, f)
    }

    pub fn run_raw_blocking_within<T, F>(
        &self,
        command: &'static str,
        timeout: Duration,
        f: F,
    ) -> LockOutcome<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
    {
        match self.0.facade() {
            Ok(facade) => facade.run_write_raw_blocking_within(command, timeout, f),
            Err(error) => LockOutcome::Ran(Err(error)),
        }
    }

    pub fn write_slot(&self) -> &Arc<Mutex<Connection>> {
        self.0.write_slot()
    }
}

impl DbReadHandle {
    pub async fn run<T, F>(&self, command: &'static str, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
    {
        self.0.facade()?.run_read(command, f).await
    }
}

struct FacadeEntry {
    facade: Arc<DbFacade>,
    pinned: bool,
}

static FACADES: OnceLock<Mutex<HashMap<usize, FacadeEntry>>> = OnceLock::new();

fn facades() -> &'static Mutex<HashMap<usize, FacadeEntry>> {
    FACADES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn facade_for(pair: &DbSlotPair) -> Result<Arc<DbFacade>> {
    let key = pair.registry_key();
    let mut table = facades()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    table.retain(|_, entry| entry.pinned || Arc::strong_count(&entry.facade) > 1);
    if let Some(entry) = table.get(&key) {
        return Ok(Arc::clone(&entry.facade));
    }
    let facade = Arc::new(DbFacade::start_slots(&pair.write, &pair.read)?);
    table.insert(
        key,
        FacadeEntry {
            facade: Arc::clone(&facade),
            pinned: false,
        },
    );
    Ok(facade)
}

pub fn install_facade(pair: &DbSlotPair) -> Result<()> {
    let key = pair.registry_key();
    let facade = Arc::new(DbFacade::start_slots(&pair.write, &pair.read)?);
    let mut retired = Vec::new();
    {
        let mut table = facades()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let stale: Vec<usize> = table
            .iter()
            .filter(|(existing, entry)| entry.pinned && **existing != key)
            .map(|(existing, _)| *existing)
            .collect();
        for existing in stale {
            if let Some(entry) = table.remove(&existing) {
                retired.push(entry);
            }
        }
        if let Some(previous) = table.insert(
            key,
            FacadeEntry {
                facade,
                pinned: true,
            },
        ) {
            retired.push(previous);
        }
    }
    drop(retired);
    Ok(())
}

#[cfg(test)]
pub(crate) fn facade_installed(write: &Arc<Mutex<Connection>>) -> bool {
    let key = Arc::as_ptr(write) as usize;
    facades()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&key)
        .is_some_and(|entry| entry.pinned)
}
