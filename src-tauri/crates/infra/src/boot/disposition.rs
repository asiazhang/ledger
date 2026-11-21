use std::path::Path;
use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::Result;

use super::encryption::{self, DbFileKind};

pub const BOOT_DB_UNREADABLE: &str = "boot.db-unreadable";

pub const BOOT_DB_NORMALIZE_FAILED: &str = "boot.db-normalize-failed";

pub fn gate_rejection_error() -> crate::error::AppError {
    crate::error::AppError::coded(
        BOOT_DB_UNREADABLE,
        "启动失败，数据库不可用，请先在失败恢复屏重置或恢复",
    )
}

#[derive(Clone)]
pub struct BootFailureGate {
    failed: Arc<AtomicBool>,
    code: Arc<RwLock<Option<String>>>,
}

impl BootFailureGate {
    pub fn new() -> Self {
        Self {
            failed: Arc::new(AtomicBool::new(false)),
            code: Arc::new(RwLock::new(None)),
        }
    }

    pub fn is_failed(&self) -> bool {
        self.failed.load(Ordering::SeqCst)
    }

    pub fn set_failed(&self, code: Option<&str>) {
        self.failed.store(true, Ordering::SeqCst);
        *self.code.write().unwrap_or_else(|e| e.into_inner()) = code.map(str::to_string);
    }

    pub fn failure_code(&self) -> String {
        self.code
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .unwrap_or_else(|| BOOT_DB_UNREADABLE.to_string())
    }

    pub fn clear(&self) {
        self.failed.store(false, Ordering::SeqCst);
        *self.code.write().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

impl Default for BootFailureGate {
    fn default() -> Self {
        Self::new()
    }
}

pub fn classify_for_boot(path: &Path) -> Result<BootDisposition> {
    let probe = encryption::probe_file(path)?;
    match probe.kind {
        DbFileKind::Plaintext => Ok(match probe.reserved_bytes {
            Some(0) | None => BootDisposition::OpenPlaintext,
            Some(_) => BootDisposition::NormalizePlaintext,
        }),
        DbFileKind::Empty => Ok(BootDisposition::OpenPlaintext),
        DbFileKind::Encrypted => {
            if encryption::has_encrypted_file_layout(path) {
                Ok(BootDisposition::AwaitUnlock)
            } else {
                Ok(BootDisposition::Unreadable)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootDisposition {
    OpenPlaintext,
    NormalizePlaintext,
    AwaitUnlock,
    Unreadable,
}

pub struct BootPlan {
    pub boot: super::data_location::Boot,
    pub disposition: Result<BootDisposition>,
}

pub fn plan_boot(default_dir: &Path) -> BootPlan {
    let boot = super::data_location::boot(default_dir);
    let disposition = classify_for_boot(&boot.db_dir.join(super::data_location::DB_FILE_NAME));
    BootPlan { boot, disposition }
}
