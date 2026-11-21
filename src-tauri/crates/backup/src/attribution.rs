use std::fs::File;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use ledger_infra::db;
use ledger_infra::db::encryption::DbFileKind;
use ledger_infra::fs_util::{cleanup, replace_file, temp_sibling};

use super::engine::{
    BackupKind, BackupScope, ZIP_DB_ENTRY, open_backup_archive, read_meta_from_archive,
};
use crate::identity;

pub(crate) const CACHE_FILE_NAME: &str = ".backup-attribution-cache.json";

const CACHE_VERSION: u32 = 1;

pub(crate) struct ProductMeta {
    pub fingerprint: Option<String>,
    pub device_id: Option<String>,
    pub kind: BackupKind,
    pub encrypted: bool,
}

pub(crate) enum Attribution {
    Owned(Option<ProductMeta>),
    Unknown(Option<ProductMeta>),
    Foreign,
}

impl Attribution {
    fn with_meta(self, meta: Option<ProductMeta>) -> Self {
        match self {
            Self::Owned(None) => Self::Owned(meta),
            Self::Unknown(None) => Self::Unknown(meta),
            other => other,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct ProductIdentity {
    meta_fingerprint: Option<String>,
    content_anchor: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CacheEntry {
    size_bytes: u64,
    mtime_secs: u64,
    #[serde(flatten)]
    identity: ProductIdentity,
}

#[derive(Debug, Serialize, Deserialize)]
struct CacheFile {
    version: u32,
    entries: std::collections::BTreeMap<String, CacheEntry>,
}

pub(crate) struct AttributionCache {
    path: PathBuf,
    entries: std::collections::BTreeMap<String, CacheEntry>,
    dirty: bool,
}

impl AttributionCache {
    pub(crate) fn new(dir: &Path) -> Self {
        let path = dir.join(CACHE_FILE_NAME);
        let entries = match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<CacheFile>(&bytes) {
                Ok(file) if file.version == CACHE_VERSION => file.entries,
                Ok(_) => {
                    tracing::debug!(cache = %path.display(), "归属缓存版本不认识，按空重建");
                    std::collections::BTreeMap::new()
                }
                Err(e) => {
                    tracing::debug!(
                        cache = %path.display(),
                        error = %e,
                        "归属缓存文件损坏，忽略并按产物重建"
                    );
                    std::collections::BTreeMap::new()
                }
            },
            Err(_) => std::collections::BTreeMap::new(),
        };
        Self {
            path,
            entries,
            dirty: false,
        }
    }

    pub(crate) fn lookup(&mut self, file_name: &str, path: &Path) -> Option<ProductIdentity> {
        let entry = self.entries.get(file_name)?;
        let metadata = std::fs::metadata(path).ok()?;
        let mtime = mtime_secs(&metadata)?;
        if entry.size_bytes == metadata.len() && entry.mtime_secs == mtime {
            Some(entry.identity.clone())
        } else {
            tracing::debug!(file = %file_name, "归属缓存失效（大小或修改时间变化），丢弃重算");
            self.entries.remove(file_name);
            self.dirty = true;
            None
        }
    }

    fn store(&mut self, file_name: &str, path: &Path, identity: &ProductIdentity) {
        let Some(metadata) = std::fs::metadata(path).ok() else {
            return;
        };
        let Some(mtime) = mtime_secs(&metadata) else {
            return;
        };
        self.entries.insert(
            file_name.to_string(),
            CacheEntry {
                size_bytes: metadata.len(),
                mtime_secs: mtime,
                identity: identity.clone(),
            },
        );
        self.dirty = true;
    }

    pub(crate) fn flush(&mut self) {
        if !self.dirty {
            return;
        }
        let file = CacheFile {
            version: CACHE_VERSION,
            entries: std::mem::take(&mut self.entries),
        };
        let tmp = temp_sibling(&self.path, "cache");
        let result = (|| -> ledger_infra::error::Result<()> {
            let json = serde_json::to_vec_pretty(&file)?;
            std::fs::write(&tmp, json)?;
            replace_file(&tmp, &self.path)
        })();
        self.entries = file.entries;
        match result {
            Ok(()) => self.dirty = false,
            Err(e) => {
                cleanup(&tmp);
                tracing::debug!(
                    cache = %self.path.display(),
                    error = %e,
                    "归属缓存写盘失败，下次列表重建"
                );
            }
        }
    }
}

fn mtime_secs(metadata: &std::fs::Metadata) -> Option<u64> {
    metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

pub(crate) fn attribute_to_scope(
    cache_key: &str,
    path: &Path,
    file_book: Option<&str>,
    scope: &BackupScope,
    cache: &mut AttributionCache,
) -> Attribution {
    if file_book == Some(scope.fingerprint.as_str()) {
        tracing::debug!(file = %cache_key, "产物归属本账本（文件名标识段命中指纹）");
        return Attribution::Owned(None);
    }
    if let Some(identity) = cache.lookup(cache_key, path) {
        tracing::debug!(file = %cache_key, "归属判定命中缓存，不再解包");
        return attribute_by_identity(cache_key, &identity, scope).with_meta(None);
    }
    let (identity, meta) = observe_product_identity(path);
    tracing::debug!(file = %cache_key, "归属缓存未命中，解包现场判定");
    cache.store(cache_key, path, &identity);
    attribute_by_identity(cache_key, &identity, scope).with_meta(meta)
}

fn attribute_by_identity(
    cache_key: &str,
    identity: &ProductIdentity,
    scope: &BackupScope,
) -> Attribution {
    if identity.meta_fingerprint.as_deref() == Some(scope.fingerprint.as_str()) {
        tracing::debug!(file = %cache_key, "产物归属本账本（内层元数据指纹命中）");
        return Attribution::Owned(None);
    }
    if identity.meta_fingerprint.is_some() {
        tracing::debug!(file = %cache_key, "产物不归属本账本（元数据指纹不匹配），不进列表");
        return Attribution::Foreign;
    }
    if identity.content_anchor.as_deref() == Some(scope.fingerprint.as_str()) {
        tracing::debug!(file = %cache_key, "产物归属本账本（内容锚命中）");
        return Attribution::Owned(None);
    }
    if identity.content_anchor.is_some() {
        tracing::debug!(file = %cache_key, "产物不归属本账本（内容锚不匹配），不进列表");
        return Attribution::Foreign;
    }
    tracing::debug!(
        file = %cache_key,
        "产物判不出归属，落入未知来源桶（可见、永不自动清理）"
    );
    Attribution::Unknown(None)
}

fn observe_product_identity(path: &Path) -> (ProductIdentity, Option<ProductMeta>) {
    let Some(meta) = read_product_meta(path) else {
        return (ProductIdentity::default(), None);
    };
    let content_anchor = if meta.fingerprint.is_some() || meta.encrypted {
        None
    } else {
        content_anchor_from_archive(path)
    };
    let identity = ProductIdentity {
        meta_fingerprint: meta.fingerprint.clone(),
        content_anchor,
    };
    (identity, Some(meta))
}

pub(crate) fn read_product_meta(backup_path: &Path) -> Option<ProductMeta> {
    let mut archive = open_backup_archive(backup_path).ok()?;
    let meta = read_meta_from_archive(&mut archive).ok()?;
    Some(ProductMeta {
        fingerprint: meta.fingerprint,
        device_id: meta.device_id,
        kind: meta.kind,
        encrypted: meta.encrypted,
    })
}

fn content_anchor_from_archive(backup_path: &Path) -> Option<String> {
    let file = File::open(backup_path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    let tmp = temp_sibling(backup_path, "anchor");
    let anchor = (|| -> Option<String> {
        let mut entry = archive.by_name(ZIP_DB_ENTRY).ok()?;
        let mut out = File::create(&tmp).ok()?;
        std::io::copy(&mut entry, &mut out).ok()?;
        drop(out);
        if matches!(
            db::encryption::probe_file_kind(&tmp).ok()?,
            DbFileKind::Encrypted
        ) {
            return None;
        }
        let conn = db::open_connection_readonly(&tmp).ok()?;
        identity::content_anchor(&conn).ok().flatten()
    })();
    cleanup(&tmp);
    anchor
}
