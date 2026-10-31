//! 产物归属判定（ADR-0144 决策 3，issue #1985 / #1986）：受管产物到账本作用
//! 域的归属按顺位短路——①文件名标识段 = 本指纹（零开包）；②产物内层元数据
//! 明文指纹（密文产物也由它判定，归属不依赖主口令）；③元数据无指纹时解包读
//! 库内内容锚（库内最早一笔交易，与指纹首用同源排序口径）。以上皆不可得（密
//! 文无指纹、残缺包、解包失败）不猜测、不丢弃：落**「未知来源」桶**——照常
//! 可见、来源标注未知，永不参与自动清理（清理是破坏性动作，宁可不删也不代另
//! 一端做决定）；别本账产物不进列表（ADR-0089 分域语义）。
//!
//! 判定结果缓存（ADR-0144 决策 3，issue #1986）：按「文件名 + 大小 + 修改时
//! 间」缓存到备份目录内的独立本地文件（[`CACHE_FILE_NAME`]）。缓存的是产物
//! 的**固有身份**（元数据指纹与内容锚），与作用域无关——多本账共享同一份缓
//! 存互不干扰；命中缓存不再解包；大小或修改时间变化即失效重算；缓存可从产物
//! 重建、可整份丢弃、损坏不阻断列表。缓存不进库、不进账本注册表、不参与备份
//! （点前缀非受管命名：不进受管清单、不被清理、不在备份产物内）。

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

/// 归属缓存文件名（备份目录内的独立本地文件；点前缀隐藏文件，非受管命名 →
/// 不进受管清单、不被清理、不参与备份；被外部删除无害，判定可从产物重建）。
pub(crate) const CACHE_FILE_NAME: &str = ".backup-attribution-cache.json";

/// 缓存格式版本：结构不兼容时整体按空重建（不尝试迁移——缓存可从产物重建）。
const CACHE_VERSION: u32 = 1;

/// 产物内层元数据的读出字段（归属判定与展示共用同一次开包，issue #1985）。
pub(crate) struct ProductMeta {
    pub fingerprint: Option<String>,
    pub device_id: Option<String>,
    pub kind: BackupKind,
    pub encrypted: bool,
}

/// 判定结论（engine 列表路径消费）。
pub(crate) enum Attribution {
    /// 归属本账本（顺位一/二/三任一命中）；`Some` = 判定时顺带读出的元数据
    /// （展示段复用，免二次开包）。
    Owned(Option<ProductMeta>),
    /// 「未知来源」桶：归属判不出（密文无指纹 / 残缺包 / 解包失败）——照常
    /// 可见、来源标注未知、永不参与自动清理；元数据可读时同样顺带复用。
    Unknown(Option<ProductMeta>),
    /// 别本账产物：不进列表（ADR-0089 分域语义保持）。
    Foreign,
}

impl Attribution {
    /// 把判定时顺带读出的产物元数据并入结论（展示段免二次开包）；结论不变。
    fn with_meta(self, meta: Option<ProductMeta>) -> Self {
        match self {
            Self::Owned(None) => Self::Owned(meta),
            Self::Unknown(None) => Self::Unknown(meta),
            other => other,
        }
    }
}

/// 产物固有身份（缓存值，与作用域无关）：任何账本对同一产物据此独立得出同
/// 一归属结论——多本账共享同一份缓存互不干扰。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct ProductIdentity {
    /// 内层元数据明文指纹（顺位二判据）；`None` = 升级前产物或元数据读不出。
    meta_fingerprint: Option<String>,
    /// 库内内容锚（顺位三判据）；`None` = 元数据指纹在场（顺位二可判定，无
    /// 需解包内库）、产物加密（不尝试解包）或解包失败。
    content_anchor: Option<String>,
}

/// 缓存条目：失效键（大小 + 修改时间）+ 产物固有身份。
#[derive(Debug, Serialize, Deserialize)]
struct CacheEntry {
    size_bytes: u64,
    mtime_secs: u64,
    #[serde(flatten)]
    identity: ProductIdentity,
}

/// 缓存文件形状（`entries` 用 BTreeMap：序列化顺序稳定，diff 友好）。
#[derive(Debug, Serialize, Deserialize)]
struct CacheFile {
    version: u32,
    entries: std::collections::BTreeMap<String, CacheEntry>,
}

/// 归属判定缓存（备份目录内独立本地文件；生命周期 = 一次列表，收尾持久化）。
pub(crate) struct AttributionCache {
    path: PathBuf,
    entries: std::collections::BTreeMap<String, CacheEntry>,
    dirty: bool,
}

impl AttributionCache {
    /// 加载缓存文件：不存在视为空；版本不认识或损坏（解析失败）记 debug 日志
    /// 并按空重建——缓存损坏不阻断列表。
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

    /// 按「文件名 + 大小 + 修改时间」查缓存：命中返回产物固有身份（调用方不
    /// 再解包）；大小或修改时间变化即失效丢弃（返回 `None`，现场重算）。
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

    /// 记录一次现场判定的产物固有身份（落内存，列表收尾统一持久化）；产物
    /// 大小或修改时间取不到（平台异常）时不缓存——每次列表现场重算，保守。
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

    /// 持久化：临时件 + 原子替换（中断不留半份缓存）；写失败只记日志——缓存
    /// 可从产物重建，不阻断列表。无变更时不写盘（列表高频刷新零写放大）。
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

/// 产物文件修改时间的秒级表示（失效键的修改时间分量）；取不到（时钟早于
/// epoch 或平台异常）返回 `None`，该产物不进缓存。
fn mtime_secs(metadata: &std::fs::Metadata) -> Option<u64> {
    metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

/// 产物归属判定（顺位短路，ADR-0144 决策 3，issue #1985 / #1986）：顺位一
/// 文件名标识段命中零开包；顺位二/三先查缓存（命中不再解包），未命中/失效
/// 现场解包判定并把产物固有身份落缓存。`cache_key` 是产物在备份目录内的相
/// 对键（扁平名或 `设备子目录/文件名`，#1988 双路径扫描），缓存键与判定日
/// 志共用——同名产物跨目录互不串扰。命中、未命中、未知来源落桶与缓存失效
/// 重建各记一条 debug 日志（含产物键与判定结论）。
pub(crate) fn attribute_to_scope(
    cache_key: &str,
    path: &Path,
    file_book: Option<&str>,
    scope: &BackupScope,
    cache: &mut AttributionCache,
) -> Attribution {
    // 顺位一：文件名标识段 = 本指纹（新产物命名直书指纹，零开包）。
    if file_book == Some(scope.fingerprint.as_str()) {
        tracing::debug!(file = %cache_key, "产物归属本账本（文件名标识段命中指纹）");
        return Attribution::Owned(None);
    }
    // 顺位二/三：命中缓存则按缓存结论归属，不再解包。
    if let Some(identity) = cache.lookup(cache_key, path) {
        tracing::debug!(file = %cache_key, "归属判定命中缓存，不再解包");
        return attribute_by_identity(cache_key, &identity, scope).with_meta(None);
    }
    // 缓存未命中/失效：现场判定（解包），产物固有身份落缓存。
    let (identity, meta) = observe_product_identity(path);
    tracing::debug!(file = %cache_key, "归属缓存未命中，解包现场判定");
    cache.store(cache_key, path, &identity);
    attribute_by_identity(cache_key, &identity, scope).with_meta(meta)
}

/// 按产物固有身份对作用域归属（顺位二/三 + 未知来源桶，纯函数，缓存与现场
/// 共用同一套结论）。
fn attribute_by_identity(
    cache_key: &str,
    identity: &ProductIdentity,
    scope: &BackupScope,
) -> Attribution {
    // 顺位二：内层元数据明文指纹（密文产物也由它判定，用户故事 10）。
    if identity.meta_fingerprint.as_deref() == Some(scope.fingerprint.as_str()) {
        tracing::debug!(file = %cache_key, "产物归属本账本（内层元数据指纹命中）");
        return Attribution::Owned(None);
    }
    if identity.meta_fingerprint.is_some() {
        tracing::debug!(file = %cache_key, "产物不归属本账本（元数据指纹不匹配），不进列表");
        return Attribution::Foreign;
    }
    // 顺位三：库内内容锚（升级前存量产物的归并判据，issue #1986）。
    if identity.content_anchor.as_deref() == Some(scope.fingerprint.as_str()) {
        tracing::debug!(file = %cache_key, "产物归属本账本（内容锚命中）");
        return Attribution::Owned(None);
    }
    if identity.content_anchor.is_some() {
        tracing::debug!(file = %cache_key, "产物不归属本账本（内容锚不匹配），不进列表");
        return Attribution::Foreign;
    }
    // 皆不可得：未知来源桶——可见、标注，永不参与自动清理（ADR-0144 决策 3）。
    tracing::debug!(
        file = %cache_key,
        "产物判不出归属，落入未知来源桶（可见、永不自动清理）"
    );
    Attribution::Unknown(None)
}

/// 单次解包读出产物固有身份（缓存值）与展示元数据：残缺包（元数据读不出）
/// 身份全空；元数据指纹在场（顺位二可判定）或产物加密（不尝试解包，ADR-0144
/// 决策 3）时内容锚恒不可得；其余明文产物解包读库内内容锚。
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

/// 单次开包读出 [`ProductMeta`]；元数据读不出（非 zip / 元数据损坏）返回
/// `None`（残缺包归属判不出，展示字段按前缀/明文回落）。
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

/// 解包产物内主库，读库内最早一笔交易的全局唯一标识（内容锚，与指纹首用同
/// 款排序口径）。只读动作：解出到临时件、只读连接、用毕清理；任何一步失败
///（非 zip、缺主库条目、密文库、旧 schema 缺列）返回 `None`——判不出归
/// 「未知来源」桶，列表不报错。
fn content_anchor_from_archive(backup_path: &Path) -> Option<String> {
    let file = File::open(backup_path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    let tmp = temp_sibling(backup_path, "anchor");
    let anchor = (|| -> Option<String> {
        let mut entry = archive.by_name(ZIP_DB_ENTRY).ok()?;
        let mut out = File::create(&tmp).ok()?;
        std::io::copy(&mut entry, &mut out).ok()?;
        drop(out);
        // 密文库（文件即真相）在列表路径读不出内容锚：没有主口令，不尝试。
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
