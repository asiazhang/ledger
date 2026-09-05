use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use chrono::NaiveDateTime;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::db;
use crate::db::encryption::DbFileKind;
use crate::error::{AppError, Result};
use crate::fs_util::{cleanup, replace_file, temp_sibling};

/// zip 包内数据库条目名。
const ZIP_DB_ENTRY: &str = "ledger.db";
/// zip 包内元数据条目名。
const ZIP_META_ENTRY: &str = "backup.json";

/// 受管备份命名规则（与前端 `defaultBackupFileName` 保持一致）：
/// 手动 `ledger-backup-YYYYMMDD-HHMMSS.db.zip` + 自动
/// `ledger-auto-YYYYMMDD-HHMMSS.db.zip`（ADR-0016，两类同等参与清理与首次兜底判定）。
const MANAGED_BACKUP_PREFIXES: &[&str] = &["ledger-backup-", super::auto::AUTO_BACKUP_PREFIX];
const MANAGED_BACKUP_SUFFIX: &str = ".db.zip";

/// 备份来源标记（issue #127）：写入 zip 包内 `backup.json` 的 `kind` 字段，
/// 自动与手动产物除文件名前缀外再以元数据显式区分。
/// 旧版本备份缺该字段（serde `#[serde(default)]`）或值非法时均回落为
/// [`BackupKind::Manual`]，列表与恢复按手动处理（向后兼容）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
pub enum BackupKind {
    /// 自动备份引擎产物（调度入口统一写入）。
    #[serde(rename = "auto")]
    Auto,
    /// 手动触发（一键备份 / 另存为）产物，也是缺省语义。
    #[default]
    #[serde(rename = "manual")]
    Manual,
}

impl std::fmt::Display for BackupKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auto => f.write_str("auto"),
            Self::Manual => f.write_str("manual"),
        }
    }
}

// 反序列化手写而非用 `rename_all` 派生：旧备份来源兼容优先于严格校验——
// 序列化只写 auto/manual 两态，读侧未知/非法值一律按 [`BackupKind::Manual`]
// 落地而不是让整个元数据解析失败。
impl<'de> Deserialize<'de> for BackupKind {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        if s == "auto" {
            Ok(Self::Auto)
        } else {
            Ok(Self::Manual)
        }
    }
}

/// 备份文件信息（用于备份文件列表展示）。
#[derive(Debug, Serialize)]
pub struct BackupFileInfo {
    pub file_name: String,
    pub path: String,
    pub size_bytes: u64,
    pub created_at: String,
    /// 备份触发来源（issue #129，列表展示用）：元数据 `kind` 为权威来源，
    /// 读取失败（非 zip / 元数据损坏的残缺包）按文件名前缀回落。
    pub kind: BackupKind,
    /// 加密标记（issue #572 / ADR-0075 决策 7）：密文备份列表显示锁形标记；
    /// 元数据缺标记（旧备份）或读取失败的残缺包按明文回落。
    pub encrypted: bool,
}

/// 备份包元数据摘要（备份列表与恢复确认弹窗的消费形状，issue #572）。
#[derive(Debug, Clone, Copy, Serialize)]
pub struct BackupMetaSummary {
    pub kind: BackupKind,
    /// 加密标记：备份时探测写入；旧备份缺标记视为明文（向后兼容）。
    pub encrypted: bool,
}

/// 备份滚动清理结果。
#[derive(Debug, Serialize)]
pub struct PruneResult {
    pub kept: usize,
    pub deleted: Vec<String>,
    pub failed: Vec<String>,
}

/// 匹配文件名命中的受管前缀；非受管返回 None。
fn matched_managed_prefix(name: &str) -> Option<&'static str> {
    MANAGED_BACKUP_PREFIXES
        .iter()
        .copied()
        .find(|prefix| name.starts_with(prefix))
}

/// 判断文件名是否为受管备份（自动命名 `<前缀>YYYYMMDD-HHMMSS.db.zip`）。
fn is_managed_backup_file_name(name: &str) -> bool {
    matched_managed_prefix(name).is_some() && name.ends_with(MANAGED_BACKUP_SUFFIX)
}

/// 打开 zip 备份包（非 zip 输入报 `backup.invalid-archive`）。
fn open_backup_archive(backup_path: &Path) -> Result<zip::ZipArchive<File>> {
    let file = File::open(backup_path)?;
    zip::ZipArchive::new(file).map_err(|e| {
        AppError::codedp(
            "backup.invalid-archive",
            format!("不是有效的开源记账备份包: {e}"),
            &[&e.to_string()],
        )
    })
}

/// 读取备份包内元数据 `backup.json` 并解析（单次开包，读侧共用）。
fn read_meta_from_archive(archive: &mut zip::ZipArchive<File>) -> Result<BackupMeta> {
    let mut entry = archive.by_name(ZIP_META_ENTRY).map_err(|_| {
        AppError::coded(
            "backup.meta-missing",
            format!("备份包内未找到 {} 元数据", ZIP_META_ENTRY),
        )
    })?;
    let mut buf = Vec::new();
    entry.read_to_end(&mut buf)?;
    serde_json::from_slice::<BackupMeta>(&buf).map_err(|e| {
        AppError::codedp(
            "backup.meta-parse-failed",
            format!("备份元数据解析失败: {e}"),
            &[&e.to_string()],
        )
    })
}

/// 读取备份包元数据摘要（来源标记 + 加密标记，issue #572）。
pub fn read_backup_meta(backup_path: &Path) -> Result<BackupMetaSummary> {
    let mut archive = open_backup_archive(backup_path)?;
    let meta = read_meta_from_archive(&mut archive)?;
    Ok(BackupMetaSummary {
        kind: meta.kind,
        encrypted: meta.encrypted,
    })
}

/// 探测单个备份文件的展示元数据（恢复确认弹窗消费，issue #572）。
///
/// zip 包读包内元数据（缺加密标记视为明文，向后兼容）；非 zip 输入按裸库
/// 文件处理（与恢复通道的提取口径一致）：文件头探测判定密文（文件即真相），
/// 来源按手动回落；探测失败原样上抛。「探测」语义与 [`db::encryption::probe_file_kind`]
/// 对齐：输入可以是任何备份文件，不预设 zip 形态。
pub fn probe_backup_meta(backup_path: &Path) -> Result<BackupMetaSummary> {
    match open_backup_archive(backup_path) {
        Ok(mut archive) => {
            let meta = read_meta_from_archive(&mut archive)?;
            Ok(BackupMetaSummary {
                kind: meta.kind,
                encrypted: meta.encrypted,
            })
        }
        Err(_) => {
            let encrypted = matches!(
                db::encryption::probe_file_kind(backup_path)?,
                DbFileKind::Encrypted
            );
            Ok(BackupMetaSummary {
                kind: BackupKind::Manual,
                encrypted,
            })
        }
    }
}

/// 读取备份包元数据中的来源标记（issue #127）。委托 [`read_backup_meta`]。
pub fn read_backup_kind(backup_path: &Path) -> Result<BackupKind> {
    read_backup_meta(backup_path).map(|s| s.kind)
}

/// 从受管备份文件名解析备份时间（`YYYYMMDD-HHMMSS`）；解析失败返回 None。
fn parse_backup_timestamp(file_name: &str) -> Option<NaiveDateTime> {
    let prefix = matched_managed_prefix(file_name)?;
    let stem = file_name
        .strip_prefix(prefix)?
        .strip_suffix(MANAGED_BACKUP_SUFFIX)?;
    NaiveDateTime::parse_from_str(stem, "%Y%m%d-%H%M%S").ok()
}

/// 列出目录中的受管备份文件，按新→旧排序。
///
/// 备份时间优先取文件名时间戳（与命名规则强一致），解析失败回退文件修改时间；
/// 两者皆失败按最旧处理（排序最靠后，清理时最先被删）。目录不存在时返回空列表
/// （界面展示空态而非报错）。
pub fn list_managed_backups(dir: &Path) -> Result<Vec<BackupFileInfo>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut files: Vec<(NaiveDateTime, PathBuf)> = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_file = entry.file_type().map(|t| t.is_file()).unwrap_or(false);
        if !is_managed_backup_file_name(&name) || !is_file {
            continue;
        }
        let ts = parse_backup_timestamp(&name).or_else(|| {
            entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .map(chrono::DateTime::<chrono::Utc>::from)
                .map(|t| t.naive_utc())
        });
        files.push((ts.unwrap_or(NaiveDateTime::MIN), entry.path()));
    }
    files.sort_by_key(|a| std::cmp::Reverse(a.0));
    files
        .into_iter()
        .map(|(ts, path)| {
            let size_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let file_name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            // 来源与加密标记优先读产物元数据；残缺包按前缀/明文回落
            // （自动前缀即 auto，否则 manual；加密标记回落明文，不误报锁标）。
            let (kind, encrypted) = read_backup_meta(&path)
                .map(|s| (s.kind, s.encrypted))
                .unwrap_or_else(|_| {
                    if file_name.starts_with(super::auto::AUTO_BACKUP_PREFIX) {
                        (BackupKind::Auto, false)
                    } else {
                        (BackupKind::Manual, false)
                    }
                });
            Ok(BackupFileInfo {
                file_name,
                path: path.to_string_lossy().into_owned(),
                size_bytes,
                // 文件名时间戳为本地时间（ADR-0016 修订：原 UTC），mtime 回退也是
                // 非权威口径：均非 UTC 时刻，不带字面 Z 假标记，避免未来被按 UTC 解析双重换算。
                created_at: ts.format("%Y-%m-%dT%H:%M:%S").to_string(),
                kind,
                encrypted,
            })
        })
        .collect()
}

/// 将目录中的受管备份修剪到最多 `keep` 个：按旧→新删除超出部分。
///
/// 单个文件删除失败（占用/无权限）时跳过并记入 `failed`，不中断其余清理。
pub fn prune_managed_backups(dir: &Path, keep: usize) -> Result<PruneResult> {
    let files = list_managed_backups(dir)?;
    let total = files.len();
    if total <= keep {
        return Ok(PruneResult {
            kept: total,
            deleted: Vec::new(),
            failed: Vec::new(),
        });
    }
    let mut deleted = Vec::new();
    let mut failed = Vec::new();
    // list 已按新→旧排序，reverse 后为旧→新，取前 excess 个删除。
    for file in files.into_iter().rev().take(total - keep) {
        match std::fs::remove_file(&file.path) {
            Ok(()) => {
                tracing::info!(file = %file.file_name, "已清理旧备份");
                deleted.push(file.file_name);
            }
            Err(e) => {
                tracing::warn!(file = %file.file_name, error = %e, "清理旧备份失败");
                failed.push(file.file_name);
            }
        }
    }
    Ok(PruneResult {
        kept: total - deleted.len(),
        deleted,
        failed,
    })
}

/// 备份元数据，写入 zip 包内 `backup.json`。`kind` 与 `encrypted` 均为旧版本
/// 备份可能缺失的字段（serde 默认回落，向后兼容）。
#[derive(Debug, Serialize, Deserialize)]
struct BackupMeta {
    created_at: String,
    app_version: String,
    schema_version: i64,
    /// 来源标记（issue #127）；缺省回落 manual（向后兼容）。
    #[serde(default)]
    kind: BackupKind,
    /// 加密标记（issue #572 / ADR-0075 决策 7）：备份时探测产物写入；
    /// 旧版本备份缺该字段时按明文对待（缺省 false，向后兼容）。
    #[serde(default)]
    encrypted: bool,
}

#[derive(Debug, Serialize)]
pub struct BackupResult {
    pub path: String,
    pub size_bytes: u64,
    pub schema_version: i64,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct RestoreResult {
    pub schema_version: i64,
    pub restored_at: String,
}

/// 当前应用期望的 schema 版本（全部迁移执行后的 `user_version`）。
pub fn expected_schema_version() -> Result<i64> {
    let mut conn = db::open_in_memory()?;
    db::init_db(&mut conn)?;
    conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
        .map_err(AppError::from)
}

/// 读取连接的 schema 版本（`user_version`）。
fn schema_version(conn: &Connection) -> Result<i64> {
    conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
        .map_err(AppError::from)
}

/// 将当前数据库备份为 zip 包（`ledger.db` + `backup.json`）写入 `target`。
///
/// `kind` 标记产物来源（自动 / 手动），随元数据落盘供后续识别。
/// 通过 `VACUUM INTO` 生成一致的库文件快照，不影响正在进行的写入；打包完成后原子替换目标文件。
pub fn backup_db_to(
    conn: &Connection,
    target: &Path,
    app_version: &str,
    kind: BackupKind,
) -> Result<BackupResult> {
    let parent = match target.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    if !parent.is_dir() {
        return Err(AppError::codedp(
            "backup.target-dir-missing",
            format!("备份目标目录不存在: {}", parent.display()),
            &[&parent.display().to_string()],
        ));
    }

    let tmp_db = temp_sibling(target, "db");
    let tmp_zip = temp_sibling(target, "zip");

    // 1. VACUUM INTO 生成一致的临时库文件（要求目标不存在，故用唯一临时名）。
    conn.execute(
        "VACUUM INTO ?1",
        rusqlite::params![tmp_db.to_string_lossy()],
    )?;

    // 2. 探测产物密文（VACUUM INTO 继承源库加密与密钥，ADR-0075 决策 7：
    //    文件即真相，探测的是实际落盘的快照而非源连接），随元数据落盘。
    let encrypted = matches!(
        db::encryption::probe_file_kind(&tmp_db)?,
        DbFileKind::Encrypted
    );
    let meta = BackupMeta {
        created_at: db::now_iso(),
        app_version: app_version.to_string(),
        schema_version: schema_version(conn)?,
        kind,
        encrypted,
    };
    let zip_result = (|| -> Result<()> {
        let file = File::create(&tmp_zip)?;
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file(ZIP_DB_ENTRY, options)?;
        let mut db_file = File::open(&tmp_db)?;
        std::io::copy(&mut db_file, &mut zip)?;
        zip.start_file(ZIP_META_ENTRY, options)?;
        zip.write_all(serde_json::to_string_pretty(&meta)?.as_bytes())?;
        zip.finish()?;
        Ok(())
    })();
    cleanup(&tmp_db);
    zip_result?;

    // 3. 原子替换目标。
    replace_file(&tmp_zip, target)?;
    cleanup(&tmp_zip);

    let size_bytes = std::fs::metadata(target)?.len();
    tracing::info!(target = %target.display(), size = %size_bytes, schema = %meta.schema_version, "备份完成");
    Ok(BackupResult {
        path: target.to_string_lossy().into_owned(),
        size_bytes,
        schema_version: meta.schema_version,
        created_at: meta.created_at,
    })
}

/// 从备份恢复：提取数据库 → 完整性 + 版本校验（必要时迁移升级）→ 安全备份当前库 → 替换。
///
/// `backup_path` 支持 zip 包（标准格式）与裸 `.db` 文件两种输入。
/// `safety_dir` 用于存放恢复前自动创建的 RestoreSafetyBackup。
/// `passphrase` 用于密文备份（issue #572）：提取出的备份库探测为密文时，
/// 校验与迁移都需凭备份所在库的主口令打开（缺口令拒绝并报码化错误，
/// 错误口令报 `encryption.passphrase-incorrect` 可重试）；明文备份不消费口令。
pub fn restore_db_from(
    backup_path: &Path,
    db_path: &Path,
    safety_dir: &Path,
    expected_schema: i64,
    passphrase: Option<&str>,
) -> Result<RestoreResult> {
    let tmp_db = temp_sibling(db_path, "restore");

    // 1. 提取数据库文件（zip 或裸 db）。
    if let Err(e) = extract_db_file(backup_path, &tmp_db) {
        cleanup(&tmp_db);
        return Err(e);
    }

    // 2. 探测备份库密文（文件即真相，issue #572），供口令消费与恢复后重置连接选择。
    let backup_encrypted = match db::encryption::probe_file_kind(&tmp_db) {
        Ok(kind) => kind == DbFileKind::Encrypted,
        Err(e) => {
            cleanup(&tmp_db);
            return Err(e);
        }
    };

    // 3. 完整性 + 版本校验；密文备份在此凭主口令打开（缺口令报
    //    backup.passphrase-required，错误口令报 encryption.passphrase-incorrect），
    //    拒绝发生在安全备份与替换之前；备份旧于当前则迁移升级。
    let backup_schema = match validate_backup(&tmp_db, expected_schema, passphrase) {
        Ok(v) => v,
        Err(e) => {
            cleanup(&tmp_db);
            return Err(e);
        }
    };

    // 4. 安全备份当前库（恢复出错时可回滚）。文件级拷贝：当前库为密文时
    //    安全备份自然继承密文，凭同一主口令可回滚（issue #572 钉住）。
    if db_path.exists() {
        std::fs::create_dir_all(safety_dir)?;
        let stamp = db::now_iso().replace([':', 'T'], "-");
        let safety = safety_dir.join(format!("restore-safety-{stamp}.db"));
        std::fs::copy(db_path, &safety)?;
        tracing::info!(safety = %safety.display(), "恢复前已自动备份当前数据库");
    }

    // 5. 替换原库。
    let replace_result = replace_file(&tmp_db, db_path);
    cleanup(&tmp_db);
    replace_result?;

    let restored_at = db::now_iso();
    // 恢复成功后重置自动备份调度状态（issue #126）：不置真、重新计时，避免
    // 恢复的旧状态触发「恢复完立即备份」；旧版本备份缺 key 时落到约定默认值。
    // 用新开连接写入：rename 替换后主连接仍指旧 inode（由前端 restart_app
    // 重新加载），必须把重置写进已就位的新库文件才能随恢复结果生效。
    // 恢复出的库可能为密文（跨模式恢复 / 密文备份往返），凭恢复时提供的
    // 主口令打开；无口令的密文库写不进去，跳过重置并记日志（issue #572）。
    let open_reset = match (backup_encrypted, passphrase) {
        (true, Some(pass)) => db::open_connection_with_passphrase(db_path, pass),
        (false, _) => db::open_connection(db_path),
        // 防御分支：密文备份缺口令在校验阶段（替换发生前）已被拒绝，
        // 控制流到不了这里；保留以维持 match 穷尽性，不 panic（ADR-0060）。
        (true, None) => {
            tracing::warn!("恢复出的密文库缺少主口令，跳过自动备份状态重置");
            tracing::info!(schema = %backup_schema, "恢复完成");
            return Ok(RestoreResult {
                schema_version: backup_schema,
                restored_at,
            });
        }
    };
    match open_reset {
        Ok(conn) => {
            if let Err(e) = super::auto::reset(&conn, &restored_at) {
                tracing::warn!(error = %e, "恢复后重置自动备份状态失败");
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "恢复后打开新库重置自动备份状态失败");
        }
    }
    tracing::info!(schema = %backup_schema, "恢复完成");
    Ok(RestoreResult {
        schema_version: backup_schema,
        restored_at,
    })
}

/// 打开备份库连接：密文库凭主口令（缺口令报 `backup.passphrase-required`，
/// 错误口令在建连后首条读语句报 `encryption.passphrase-incorrect`）；明文与
/// 不足一个文件头的残缺输入都按既有明文路径打开（残缺输入由完整性检查报错，
/// 保持既有错误形态）。
fn open_backup_db_conn(tmp_db: &Path, passphrase: Option<&str>) -> Result<Connection> {
    match db::encryption::probe_file_kind(tmp_db)? {
        DbFileKind::Encrypted => {
            let conn = match passphrase {
                Some(pass) => db::open_connection_with_passphrase(tmp_db, pass)?,
                None => {
                    return Err(AppError::coded(
                        "backup.passphrase-required",
                        "该备份为密文备份，需要备份所在库的主口令才能恢复",
                    ));
                }
            };
            // `PRAGMA key` 本身不校验口令；校验发生在首条读语句。用类型化读
            // 语句先行校验（与 unlock_db_file 同款），错误口令归一为可重试的
            // 码化错误，而不是让后续完整性检查以裸错误上抛（issue #572）。
            if let Err(e) =
                conn.query_row::<i64, _, _>("SELECT count(*) FROM sqlite_master", [], |r| r.get(0))
            {
                if db::encryption::is_not_a_database(&e) {
                    // 合并口径单一构造点（ADR-0075 决策 5 修订 / issue #603）：
                    // 备份口令错误与备份损坏同报 NOTADB，不误报损坏、可就地重输。
                    return Err(db::encryption::passphrase_incorrect_error());
                }
                return Err(e.into());
            }
            Ok(conn)
        }
        DbFileKind::Plaintext | DbFileKind::Empty => db::open_connection(tmp_db),
    }
}

/// 校验备份数据库文件：完整性检查 + schema 版本策略（旧→新允许并迁移，新→旧拒绝）。
fn validate_backup(tmp_db: &Path, expected_schema: i64, passphrase: Option<&str>) -> Result<i64> {
    let mut conn = open_backup_db_conn(tmp_db, passphrase)?;
    db::check_integrity(&conn)?;
    let backup_schema = schema_version(&conn)?;
    if backup_schema > expected_schema {
        return Err(AppError::codedp(
            "backup.schema-newer",
            format!(
                "备份来自更高版本的应用（备份 schema v{backup_schema} > 当前 v{expected_schema}），请升级应用后再恢复"
            ),
            &[&backup_schema.to_string(), &expected_schema.to_string()],
        ));
    }
    if backup_schema < expected_schema {
        tracing::info!(
            backup_schema,
            expected_schema,
            "备份 schema 较旧，恢复时自动迁移升级"
        );
        db::init_db(&mut conn)?;
    }
    Ok(backup_schema)
}

/// 从备份输入提取数据库文件到 `out`：zip 包解出 `ledger.db`，裸 `.db` 直接拷贝。
fn extract_db_file(backup_path: &Path, out: &Path) -> Result<()> {
    let file = File::open(backup_path)?;
    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(_) => {
            // 非 zip：按裸 db 处理。
            std::fs::copy(backup_path, out)?;
            return Ok(());
        }
    };

    let mut found = false;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().to_string();
        if name == ZIP_DB_ENTRY {
            let mut out_file = File::create(out)?;
            std::io::copy(&mut entry, &mut out_file)?;
            found = true;
        }
    }
    if !found {
        return Err(AppError::coded(
            "backup.db-entry-missing",
            format!("备份包内未找到 {}，不是有效的开源记账备份", ZIP_DB_ENTRY),
        ));
    }
    Ok(())
}
