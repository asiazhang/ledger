//! 备份引擎（ADR-0007 / ADR-0016）：zip 打包（`VACUUM INTO` 双文件成对一致性
//! 快照——业务件 + 同步元数据件（ADR-0139 决策 6，逐库 VACUUM 收口住基础设施）+
//! `backup.json` 元数据，自 ADR-0144 / #1984 起另带账本指纹与来源设备标识两个
//! 明文水印）、恢复与恢复前安全备份（恢复按备份形态分支：成对产
//! 物双库归位，旧形态单文件行为不变）、schema 版本校验（旧→新迁移、新→旧
//! 拒绝）、受管备份列表与滚动清理（列表/清理按**账本指纹**分域，ADR-0144
//! 决策 3 / issue #1985：跨端产物合并可见，本地句柄不再参与归属；清理以设备
//! 水印为第二道门——非本机设备名下与来源未知产物保守不删、不占配额）。

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use chrono::NaiveDateTime;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use ledger_infra::db;
use ledger_infra::db::encryption::DbFileKind;
use ledger_infra::error::{AppError, Result};
use ledger_infra::fs_util::{cleanup, replace_file, temp_sibling};

use crate::identity;

/// 备份作用域（ADR-0144 决策 3，issue #1985）：受管备份的列出与滚动清理按
/// **账本指纹**分域——指纹是跨端稳定身份，同一本账在各端各目录下的产物
/// 合并可见，本地句柄与路径不再参与归属判定；设备标识用于清理第二道门
///（本机设备水印比对，跨端互不代删；(指纹 × 设备) 配额分桶归 #1987）。
/// `None` 为引擎级兼容口径（不过滤，测试与直接调用形态）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupScope {
    /// 当前账本指纹（跨端稳定身份；构造见 [`BackupScope::of_conn`]）。
    pub fingerprint: String,
    /// 本机设备标识（多端同步域 DeviceId；不可得 = `None`，清理保守不删）。
    pub device_id: Option<String>,
}

impl BackupScope {
    /// 由连接构造当前账本的作用域（读侧唯一构造点，壳层与调度侧同源）：指纹
    /// 已固化恒读固化值；未固化回内容锚（与首用固化算法同源，固化前两者恒
    /// 等——归属判定据此即时成立，固化仍由备份写出路径首用完成）；空库且未
    /// 固化 = `None`（该现场本账尚无任何可归属产物，调用方按空列表/不清理处
    /// 理，不退化为全量）。设备标识不可得时 `device_id = None`，清理保守不删。
    pub fn of_conn(conn: &Connection) -> Result<Option<Self>> {
        Ok(
            identity::book_fingerprint_readonly(conn)?.map(|fingerprint| Self {
                fingerprint,
                device_id: identity::source_device_id(conn),
            }),
        )
    }

    /// 产物名携带的账本标识段（命名侧消费；无作用域 = 旧命名）。
    pub(crate) fn book_tag(&self) -> &str {
        &self.fingerprint
    }
}

/// zip 包内数据库条目名。
const ZIP_DB_ENTRY: &str = "ledger.db";
/// zip 包内同步元数据件条目名（双文件成对产物，ADR-0139 决策 6；旧形态单
/// 文件备份无此条目）。
const ZIP_SYNC_ENTRY: &str = "sync.db";
/// zip 包内元数据条目名。
const ZIP_META_ENTRY: &str = "backup.json";

/// 受管备份命名规则（与前端 `defaultBackupFileName` 保持一致，ADR-0016，
/// issue #127；账本标识见 [`managed_backup_file_name`]，issue #836）：
/// 手动 `ledger-backup-…` + 自动 `ledger-auto-…`（两类同等参与清理与首次
/// 兜底判定）。
const MANAGED_BACKUP_PREFIXES: &[&str] = &["ledger-backup-", super::auto::AUTO_BACKUP_PREFIX];
const MANAGED_BACKUP_SUFFIX: &str = ".db.zip";

/// 文件名内时间戳段的定宽（`YYYYMMDD-HHMMSS`）。
const TIMESTAMP_SEGMENT_LEN: usize = 15;

/// 受管备份产物名（命名规则唯一构造点，前后端共享同一格式约定，issue #836）：
/// `<前缀><YYYYMMDD-HHMMSS>[-<账本标识>].db.zip`。账本标识：携带当前活动账本
/// 标识，两本账的产物在共享备份目录内可区分、互不覆盖；`book` 为 `None` 时
/// 退化为无标识的旧命名（兼容口径，测试/无作用域现场）。
pub(crate) fn managed_backup_file_name(
    prefix: &str,
    timestamp: &str,
    book: Option<&str>,
) -> String {
    match book {
        Some(id) => format!("{prefix}{timestamp}-{id}{MANAGED_BACKUP_SUFFIX}"),
        None => format!("{prefix}{timestamp}{MANAGED_BACKUP_SUFFIX}"),
    }
}

/// 备份来源标记（issue #127）：写入 zip 包内 `backup.json` 的 `kind` 字段，
/// 自动与手动产物除文件名前缀外再以元数据显式区分；更新前备份（PreUpdateBackup，
/// issue #1430 / ADR-0124 决策 6）是第三种来源。
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
    /// 更新前备份（PreUpdateBackup）产物：应用自动更新安装前由系统自动创建
    /// （见 [`PRE_UPDATE_BACKUP_FILE_NAME`] 与 [`create_pre_update_backup`]）。
    /// 与 RestoreSafetyBackup 同族——不经调度器、不占保留上限、独立目录存放；
    /// 旧版本读侧不认识该值，按 [`BackupKind::Manual`] 回落（向后兼容）。
    #[serde(rename = "pre-update")]
    PreUpdate,
}

impl std::fmt::Display for BackupKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auto => f.write_str("auto"),
            Self::Manual => f.write_str("manual"),
            Self::PreUpdate => f.write_str("pre-update"),
        }
    }
}

// 反序列化手写而非用 `rename_all` 派生：旧备份来源兼容优先于严格校验——
// 序列化只写 auto/manual/pre-update 三态，读侧未知/非法值一律按
// [`BackupKind::Manual`] 落地而不是让整个元数据解析失败（旧版本应用读到
// 更新前备份的 `pre-update` 标记同样回落手动，列表与恢复不报错）。
impl<'de> Deserialize<'de> for BackupKind {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        if s == "auto" {
            Ok(Self::Auto)
        } else if s == "pre-update" {
            Ok(Self::PreUpdate)
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
    /// 来源设备标识（ADR-0144 决策 5，issue #1985）：产物内层元数据的
    /// `device_id` 明文水印；缺水印（升级前旧产物/残缺包/产出轮次设备不可得）
    /// = `None`，前端徽标呈「未知来源」。
    pub source_device_id: Option<String>,
    /// 来源是否未知（issue #1985）：元数据读不出设备水印 = true（与
    /// `source_device_id` 互为表里；归属判定与来源未知是两个维度——指纹
    /// 可归属的产物也可以来源未知）。
    pub source_unknown: bool,
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
pub(crate) fn is_managed_backup_file_name(name: &str) -> bool {
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

/// 从受管备份文件名拆出（备份时间，账本标识）：时间戳在头部，为定宽
/// `YYYYMMDD-HHMMSS`；账本标识跟在其后、可缺省（历史产物）。解析定位在头部
/// 定宽段，对标识内容（UUID / 目录派生哈希，均可能含 `-`）不敏感。
pub(crate) fn split_managed_name(file_name: &str) -> Option<(NaiveDateTime, Option<&str>)> {
    let prefix = matched_managed_prefix(file_name)?;
    let stem = file_name
        .strip_prefix(prefix)?
        .strip_suffix(MANAGED_BACKUP_SUFFIX)?;
    if stem.len() == TIMESTAMP_SEGMENT_LEN {
        // 旧命名：整段即时间戳。
        let ts = NaiveDateTime::parse_from_str(stem, "%Y%m%d-%H%M%S").ok()?;
        return Some((ts, None));
    }
    // 新命名：头部为定宽时间戳段，其后是连接 `-` 与账本标识。
    if stem.len() > TIMESTAMP_SEGMENT_LEN + 1 && stem.as_bytes()[TIMESTAMP_SEGMENT_LEN] == b'-' {
        let ts =
            NaiveDateTime::parse_from_str(&stem[..TIMESTAMP_SEGMENT_LEN], "%Y%m%d-%H%M%S").ok()?;
        let book = &stem[TIMESTAMP_SEGMENT_LEN + 1..];
        if !book.is_empty() {
            return Some((ts, Some(book)));
        }
    }
    None
}

/// 从受管备份文件名解析备份时间；解析失败返回 None（列表回退文件修改时间）。
fn parse_backup_timestamp(file_name: &str) -> Option<NaiveDateTime> {
    split_managed_name(file_name).map(|(ts, _)| ts)
}

/// 产物内层元数据的读出字段（归属判定与展示共用同一次开包，issue #1985）。
struct ProductMeta {
    fingerprint: Option<String>,
    device_id: Option<String>,
    kind: BackupKind,
    encrypted: bool,
}

/// 单次开包读出 [`ProductMeta`]；元数据读不出（非 zip / 元数据损坏）返回
/// `None`（残缺包归属判不出，展示字段按前缀/明文回落）。
fn read_product_meta(backup_path: &Path) -> Option<ProductMeta> {
    let mut archive = open_backup_archive(backup_path).ok()?;
    let meta = read_meta_from_archive(&mut archive).ok()?;
    Some(ProductMeta {
        fingerprint: meta.fingerprint,
        device_id: meta.device_id,
        kind: meta.kind,
        encrypted: meta.encrypted,
    })
}

/// 产物归属判定（ADR-0144 决策 3，issue #1985）：按顺位短路——①文件名标识
/// 段等于本指纹（新产物命名直书指纹，零开包）；②产物内层元数据明文指纹等
/// 于本指纹（指向别端句柄或无标识命名的产物经水印归并）。命中返回
/// `Some(false)`（`true` = 归属读已顺带完成，列表可复用）；未命中返回
/// `None`——判不出归属的产物本期不进列表（内容锚判定与「未知来源」桶的
/// 可见性归 #1986/T4）。命中与未命中各记一条 debug 日志（含文件名与结论）。
fn attribute_to_scope(
    file_name: &str,
    path: &Path,
    file_book: Option<&str>,
    scope: &BackupScope,
) -> Option<Option<ProductMeta>> {
    if file_book == Some(scope.fingerprint.as_str()) {
        tracing::debug!(file = %file_name, "产物归属本账本（文件名标识段命中指纹）");
        return Some(None);
    }
    match read_product_meta(path) {
        Some(meta) if meta.fingerprint.as_deref() == Some(scope.fingerprint.as_str()) => {
            tracing::debug!(file = %file_name, "产物归属本账本（内层元数据指纹命中）");
            Some(Some(meta))
        }
        _ => {
            tracing::debug!(file = %file_name, "产物不归属本账本（指纹不匹配），不进列表");
            None
        }
    }
}

/// 把单个目录内作用域内的受管产物收进 `files`（列表扫描单元，双路径共用）；
/// 归属判定按指纹顺位短路走 [`attribute_to_scope`]（ADR-0089 分域语义保持）。
fn collect_scoped_in_dir(
    root: &Path,
    scope: Option<&BackupScope>,
    // 第三元：归属判定顺位二命中时顺带读出的产物元数据（复用同一开包，免二次解压）。
    files: &mut Vec<(NaiveDateTime, PathBuf, Option<ProductMeta>)>,
) -> Result<()> {
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_file = entry.file_type().map(|t| t.is_file()).unwrap_or(false);
        if !is_managed_backup_file_name(&name) || !is_file {
            continue;
        }
        let file_book = split_managed_name(&name).and_then(|(_, book)| book);
        // 归属判定（有作用域时）：命中顺位一 → 元数据留待展示段按既有口径读；
        // 命中顺位二 → 顺带持有的元数据直接复用；未命中 → 不进列表。
        let attributed = match scope {
            None => None,
            Some(scope) => match attribute_to_scope(&name, &entry.path(), file_book, scope) {
                Some(attributed) => attributed,
                None => continue,
            },
        };
        let ts = parse_backup_timestamp(&name).or_else(|| {
            entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .map(chrono::DateTime::<chrono::Utc>::from)
                .map(|t| t.naive_utc())
        });
        files.push((ts.unwrap_or(NaiveDateTime::MIN), entry.path(), attributed));
    }
    Ok(())
}

/// 受管备份写侧落点（ADR-0144 决策 6，#1988）：`<备份目录>/<设备标识>/<产物名>`
/// ——新产物按设备多分一层，容灾镜像与跨端清理都以设备目录为界；设备标识经
/// 连接现场读取（[`identity::source_device_id`]，与产物元数据水印同源，首用
/// 生成）。设备标识取不到时回落扁平旧路径——该轮产物本就无设备水印（来源
/// 未知，永不参与自动清理），备份不因身份缺失阻断。设备子目录按需创建；
/// 备份目录本体缺失不在此补建，交由 [`backup_db_to`] 的目标目录门禁以既有
/// 码化错误收场（自动备份「目录无效即失败」语义不变）。
pub fn managed_write_target(dir: &Path, file_name: &str, conn: &Connection) -> Result<PathBuf> {
    let target = match identity::source_device_id(conn) {
        Some(device) => {
            let device_dir = dir.join(&device);
            if dir.is_dir() {
                std::fs::create_dir_all(&device_dir)?;
            }
            device_dir.join(file_name)
        }
        None => dir.join(file_name),
    };
    Ok(target)
}

/// 手动备份命令的目标解析（#1988）：目标落在当前备份目录（进程内偏好镜像，
/// 前端启动/变更时经 `set_auto_backup_dir` 推送）内且文件名为受管命名时，
/// 改道设备子目录落点（[`managed_write_target`]）——与自动备份同布局；其余
/// 现场原样返回：另存为到备份目录之外是用户显式选择，不代为改道；镜像未
/// 配置（极端时序）退回既有扁平行为。
pub fn managed_command_target(target: &Path, conn: &Connection) -> Result<PathBuf> {
    let Some(name) = target.file_name().and_then(|n| n.to_str()) else {
        return Ok(target.to_path_buf());
    };
    if !is_managed_backup_file_name(name) {
        return Ok(target.to_path_buf());
    }
    let parent = match target.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => return Ok(target.to_path_buf()),
    };
    let Some(mirror) = super::auto::shared_prefs().snapshot_dir() else {
        return Ok(target.to_path_buf());
    };
    if !same_dir_path(&mirror, &parent.to_string_lossy()) {
        return Ok(target.to_path_buf());
    }
    managed_write_target(parent, name, conn)
}

/// 目录同位判定（尾部分隔符不敏感）：镜像来自前端 `normalizeBackupDir`（去尾
/// 斜杠）推送，目标父目录来自用户路径拼接，两侧可能差一个尾分隔符。
fn same_dir_path(a: &str, b: &str) -> bool {
    fn norm(s: &str) -> &str {
        s.trim_end_matches(['/', '\\'])
    }
    let (a, b) = (norm(a), norm(b));
    !a.is_empty() && a == b
}

/// 列出目录中作用域内的受管备份文件，按新→旧排序。
///
/// 扫描两处（ADR-0144 决策 6，#1988）：目录本体 = 旧扁平路径（升级存量，
/// 只读）+ 一级子目录 = 设备子目录（新产物落点）。引擎不认识「设备」这一
/// 形状：一级子目录一律下探，受管判定仍只认文件名与作用域，目录多一层不
/// 引起失配。单个子目录读失败（并发清理移除等瞬态）跳过并留痕，不放大成
/// 整表失败；目录本体读失败照旧上抛。
///
/// `scope`：账本作用域（`None` = 不过滤的引擎级兼容口径）；归属判定按指纹
/// 顺位短路（见 [`attribute_to_scope`]）：其他账本/判不出归属的产物不可见、
/// 不参与清理（ADR-0089 分域语义保持，ADR-0144 跨端合并）。
/// 备份时间优先取文件名时间戳（与命名规则强一致），解析失败回退文件修改时间；
/// 两者皆失败按最旧处理（排序最靠后，清理时最先被删）。目录不存在时返回空列表
/// （界面展示空态而非报错）。
pub fn list_managed_backups(
    dir: &Path,
    scope: Option<&BackupScope>,
) -> Result<Vec<BackupFileInfo>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut files: Vec<(NaiveDateTime, PathBuf, Option<ProductMeta>)> = Vec::new();
    collect_scoped_in_dir(dir, scope, &mut files)?;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let sub = entry.path();
        if let Err(e) = collect_scoped_in_dir(&sub, scope, &mut files) {
            tracing::debug!(dir = %sub.display(), error = %e, "子目录扫描失败，跳过");
        }
    }
    files.sort_by_key(|a| std::cmp::Reverse(a.0));
    files
        .into_iter()
        .map(|(ts, path, attributed)| {
            let size_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let file_name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            // 来源与加密标记优先读产物元数据；残缺包按前缀/明文回落
            // （自动前缀即 auto，否则 manual；加密标记回落明文，不误报锁标）。
            let (kind, encrypted, source_device_id) = match attributed {
                Some(meta) => (meta.kind, meta.encrypted, meta.device_id),
                None => match read_product_meta(&path) {
                    Some(meta) => (meta.kind, meta.encrypted, meta.device_id),
                    None => {
                        if file_name.starts_with(super::auto::AUTO_BACKUP_PREFIX) {
                            (BackupKind::Auto, false, None)
                        } else {
                            (BackupKind::Manual, false, None)
                        }
                    }
                },
            };
            let source_unknown = source_device_id.is_none();
            Ok(BackupFileInfo {
                file_name,
                path: path.to_string_lossy().into_owned(),
                size_bytes,
                // 文件名时间戳为本地时间（ADR-0016 修订：原 UTC），mtime 回退也是
                // 非权威口径：均非 UTC 时刻，不带字面 Z 假标记，避免未来被按 UTC 解析双重换算。
                created_at: ts.format("%Y-%m-%dT%H:%M:%S").to_string(),
                kind,
                encrypted,
                source_device_id,
                source_unknown,
            })
        })
        .collect()
}

/// 将目录中作用域内的受管备份修剪到最多 `keep` 个：按旧→新删除超出部分
/// （issue #836：各账本在共享 BackupDirectory 内分别滚动清理，保留上限为
/// 设备本地偏好、按本独立计算）。
///
/// 清理判定两道门（ADR-0144 决策 3/5；第一道门自 issue #1985 起按指纹分域）：
/// 指纹归属过滤（第一道，决定候选）之后，逐候选比对内层元数据的设备水印与
/// 本机设备——带**本机**设备水印的产物才是可清理产物；他端设备名下的同账
/// 产物互不代删（ADR-0144 决策 4），缺设备水印（升级前存量、设备标识不可得
/// 的轮次、残缺包）= 来源未知，同样保守不删、不占保留配额（(指纹 × 设备)
/// 配额分桶归 #1987）。本机设备不可得（作用域无 device_id）时一律不删。
/// 无作用域兼容口径沿用既有行为：带设备水印即可清理。非本机/未知来源产物
/// 不计入配额分母；kept 报告的仍是作用域内受管产物总数。单个文件删除失败
/// （占用/无权限）时跳过并记入 `failed`，不中断其余清理。
pub fn prune_managed_backups(
    dir: &Path,
    keep: usize,
    scope: Option<&BackupScope>,
) -> Result<PruneResult> {
    let files = list_managed_backups(dir, scope)?;
    let total = files.len();
    let own_device = scope.and_then(|s| s.device_id.as_deref());
    let mut delible: Vec<BackupFileInfo> = Vec::new();
    for file in files {
        let deletable = match own_device {
            // 有作用域：设备水印逐值比对（指纹归属 ≠ 本机产物，可来自任一端）。
            Some(own) => file.source_device_id.as_deref() == Some(own),
            // 无作用域兼容口径：带设备水印即可清理（既有行为，#1984）。
            None => file.source_device_id.is_some(),
        };
        if deletable {
            delible.push(file);
        } else {
            tracing::debug!(
                file = %file.file_name,
                "非本机设备名下/来源未知的受管产物跳过清理（永不自动清理，ADR-0144）"
            );
        }
    }
    if delible.len() <= keep {
        return Ok(PruneResult {
            kept: total,
            deleted: Vec::new(),
            failed: Vec::new(),
        });
    }
    let mut deleted = Vec::new();
    let mut failed = Vec::new();
    // delible 与 list 同序（新→旧），reverse 后为旧→新，取前超额个删除。
    let excess = delible.len() - keep;
    for file in delible.into_iter().rev().take(excess) {
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

/// 备份元数据，写入 zip 包内 `backup.json`。`kind`、`encrypted` 与 `paired` 均为
/// 旧版本备份可能缺失的字段（serde 默认回落，向后兼容）。
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
    /// 双库成对标记（ADR-0139 决策 6，票 04）：true 表示包内 `sync.db` 条目是
    /// 同步元数据件（恢复按双库成对归位）。产出方拆库后恒写 true（contract，
    /// #1872：无 main 回退源）；缺省 false = 旧形态单文件备份，sync 条目
    ///（expand 期回退形态）或缺失一律忽略，恢复行为与旧版一致。
    #[serde(default)]
    paired: bool,
    /// 账本指纹（ADR-0144 决策 5，#1984）：产物归属账本的跨端身份，明文
    ///（只含标识不含密钥，归属判定不依赖主口令）。旧版本产物缺该字段
    ///（缺省 None，向后兼容）；旧版本应用读新产物对未知字段宽容（serde
    /// 默认行为，未开 deny_unknown_fields）。
    #[serde(default)]
    fingerprint: Option<String>,
    /// 生成端设备标识（DeviceId，ADR-0144 决策 5）：明文，多端同步域
    /// 安装实例级标识。设备标识不可得的轮次不写该字段（skip 序列化）——
    /// 该产物视为来源未知（与旧产物同形态，读侧统一按 None 处理）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    device_id: Option<String>,
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

/// 源库形态门禁（issue #1454）：快照（`VACUUM INTO`）发起前检查源库文件头，
/// 命中**外来形态明文库**（每页保留字节 ≠ 0，外部工具写入，issue #1453）时
/// 返回码化错误，不再发起注定失败的语句——SQLCipher 给 `ATTACH` 加的补丁
/// 在「主库请求的保留字节 > 0」时推断主库是加密库、让附加库继承同一密钥，
/// 明文库没有密钥，`VACUUM INTO` 内部那条不带 KEY 的 ATTACH 必然失败（报
/// SQLite 原文）。启动归一化（#1453）覆盖启动时已存在的偏差；本门禁覆盖
/// 「运行期间库文件被外部工具改写」的窗口，文案指向重启（重启即经启动
/// 归一化修复）。
///
/// 路径取自连接（`main` 库文件名）；内存库（测试工厂）无路径可探测、密文库
/// 的保留字节无意义（`VACUUM INTO` 继承真实密钥，既有设计依赖，ADR-0075
/// 决策 7）——一律放行，只拦「明文库 + 保留字节 ≠ 0」。
fn ensure_source_snapshotable(conn: &Connection) -> Result<()> {
    let path = match conn.path() {
        Some(p) if !p.is_empty() => p,
        _ => return Ok(()),
    };
    let probe = db::encryption::probe_file(Path::new(path))?;
    if probe.kind == DbFileKind::Plaintext && probe.reserved_bytes.is_some_and(|v| v != 0) {
        return Err(AppError::coded(
            "backup.foreign-form",
            "库文件被外部工具改写，无法创建备份，重启应用可自动修复",
        ));
    }
    Ok(())
}

/// 将当前数据库备份为 zip 包（`ledger.db` + `sync.db` + `backup.json`）写入
/// `target`。
///
/// `kind` 标记产物来源（自动 / 手动），随元数据落盘供后续识别。
/// 通过 `VACUUM INTO` 生成一致的库文件快照（双文件成对，ADR-0139 决策 6：
/// 业务件 + 同步元数据件，单连接互斥锁内逐库快照），不影响正在进行的写入；
/// 打包完成后原子替换目标文件。同步件对 attached `sync` 侧快照（真同步元数据，
/// `paired = true`；拆库后 attached 恒有表，无 main 回退源——contract，#1872）。
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

    // 源库形态门禁：外来形态明文库的快照必然失败，先拦下给可读错误
    //（issue #1454），不创建任何临时文件。
    ensure_source_snapshotable(conn)?;

    let tmp_db = temp_sibling(target, "db");
    let tmp_sync = temp_sibling(target, "sync");
    let tmp_zip = temp_sibling(target, "zip");

    // 全部步骤收进一个闭包：任一步失败（`VACUUM INTO`、打包、替换启用）都在
    // 收尾统一清理临时文件——失败早退绕过 cleanup 曾每次在备份目录留下
    // 临时残留（0 字节文件累积，既有缺陷的范围外修复，issue #1454）。
    let result = (|| -> Result<BackupResult> {
        // 0. 账本身份水印（ADR-0144 / #1984）：先算指纹与设备标识再快照——
        //    首用固化（AppSettings 写入与 DeviceId 首用生成）落在本轮快照内，
        //    产物（库件 + 内层元数据）自洽携带身份；设备标识取不到只降级
        //    水印（来源未知），不阻断备份。
        let fingerprint = identity::book_fingerprint(conn)?;
        let device_id = identity::source_device_id(conn);
        // 1. VACUUM INTO 生成一致的临时库文件（要求目标不存在，故用唯一临时名）。
        conn.execute(
            "VACUUM INTO ?1",
            rusqlite::params![tmp_db.to_string_lossy()],
        )?;
        // 同步元数据件：attached `sync` 侧的同刻快照（逐库 VACUUM 收口基础设施
        // 单点；与业务件同一互斥锁内先后定格，同刻成对）。生产连接恒挂载，
        // 产物恒为真成对形态。
        let paired = true;
        db::snapshot_sync_tables_into(conn, &tmp_sync)?;

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
            paired,
            fingerprint: Some(fingerprint),
            device_id,
        };
        let file = File::create(&tmp_zip)?;
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file(ZIP_DB_ENTRY, options)?;
        let mut db_file = File::open(&tmp_db)?;
        std::io::copy(&mut db_file, &mut zip)?;
        zip.start_file(ZIP_SYNC_ENTRY, options)?;
        let mut sync_file = File::open(&tmp_sync)?;
        std::io::copy(&mut sync_file, &mut zip)?;
        zip.start_file(ZIP_META_ENTRY, options)?;
        zip.write_all(serde_json::to_string_pretty(&meta)?.as_bytes())?;
        zip.finish()?;

        // 3. 原子替换目标（成功时 tmp_zip 已 rename 走，收尾 cleanup 容忍不存在）。
        replace_file(&tmp_zip, target)?;

        let size_bytes = std::fs::metadata(target)?.len();
        tracing::info!(target = %target.display(), size = %size_bytes, schema = %meta.schema_version, paired, "备份完成");
        Ok(BackupResult {
            path: target.to_string_lossy().into_owned(),
            size_bytes,
            schema_version: meta.schema_version,
            created_at: meta.created_at,
        })
    })();
    cleanup(&tmp_db);
    cleanup(&tmp_sync);
    cleanup(&tmp_zip);
    result
}

/// 更新前备份产物名（issue #1430 / ADR-0124 决策 6）：**固定名**——每次更新
/// 前备份覆盖上一份（覆盖式保留最近一份，不堆积），与恢复安全备份的
/// 「永不删除」族内差异点。落点独立于用户备份目录（壳层解析为应用数据目录），
/// 不匹配受管命名前缀，永不进入用户备份清单、不占备份保留上限（词汇表见
/// 备份与数据文件域 PreUpdateBackup 词条）。
pub const PRE_UPDATE_BACKUP_FILE_NAME: &str = "pre-update.db.zip";

/// 创建更新前备份（PreUpdateBackup，issue #1430 / ADR-0124 决策 6）：应用自动
/// 更新安装前由系统自动为当前活动账本创建的备份，用于新版迁移出错时回滚。
///
/// 与恢复安全备份同族语义（系统自动创建与命名、不经调度器、不标脏、不占
/// 保留上限）：不经调度器与不标脏由调用形态保证——调用方以**裸作业**投递本
/// 函数（不经统一写入口，无提交点置脏，先例同 `create_backup` 命令），调度
/// 状态零接触；产物落 [`PRE_UPDATE_BACKUP_FILE_NAME`] 固定名，覆盖上一份。
/// 备份失败（`VACUUM INTO` / 打包 / 落盘任一步）原样上抛，由调用方中止更新
/// ——不带病升级（ADR-0124 决策 6）。
pub fn create_pre_update_backup(
    conn: &Connection,
    dir: &Path,
    app_version: &str,
) -> Result<BackupResult> {
    backup_db_to(
        conn,
        &dir.join(PRE_UPDATE_BACKUP_FILE_NAME),
        app_version,
        BackupKind::PreUpdate,
    )
}

/// 从备份恢复：提取数据库 → 完整性 + 版本校验（必要时迁移升级）→ 安全备份当前库 → 替换。
///
/// `backup_path` 支持 zip 包（标准格式）与裸 `.db` 文件两种输入。
/// `safety_dir` 用于存放恢复前自动创建的 RestoreSafetyBackup。
/// `passphrase` 用于密文备份（issue #572）：提取出的备份库探测为密文时，
/// 校验与迁移都需凭备份所在库的主口令打开（缺口令拒绝并报码化错误，
/// 错误口令报 `encryption.passphrase-incorrect` 可重试）；明文备份不消费口令。
///
/// 恢复按备份形态分支（双库成对，ADR-0139 决策 6）：元数据 `paired = true`
/// 的双库产物把 `sync.db` 一并归位（设备身份、位点、日志随库一致回滚，替换
/// 先 sync 后 main，任一步失败已归位/已替换的 sync.db 尽力回位，「回滚无痕」
/// 语义不变）；旧形态（paired 缺省 false）只恢复主库，原位遗留 sync.db 照旧
/// 移入安全目录保留、由挂载接线补建。
pub fn restore_db_from(
    backup_path: &Path,
    db_path: &Path,
    safety_dir: &Path,
    expected_schema: i64,
    passphrase: Option<&str>,
) -> Result<RestoreResult> {
    let tmp_db = temp_sibling(db_path, "restore");
    let tmp_sync = temp_sibling(db_path, "restore-sync");

    // 1. 提取数据库文件（zip 或裸 db；双库产物同包提取 sync 条目）。
    let sync_entry_present = match extract_db_files(backup_path, &tmp_db, &tmp_sync) {
        Ok(found) => found,
        Err(e) => {
            cleanup(&tmp_db);
            cleanup(&tmp_sync);
            return Err(e);
        }
    };
    // 恢复形态分支判据（自描述元数据，缺省旧形态；元数据缺失/损坏的包按旧
    // 形态处理——恢复不因元数据而新增失败面，与既有行为一致）。
    let meta_paired = backup_paired_flag(backup_path);
    let paired = meta_paired && sync_entry_present;
    if meta_paired && !sync_entry_present {
        // 成对产物缺 sync 条目：包不完整（产出方恒成对写条目，只有手工拼包
        // 会命中），显式报错而不是静默丢同步身份。
        cleanup(&tmp_db);
        cleanup(&tmp_sync);
        return Err(AppError::coded(
            "backup.sync-entry-missing",
            format!("备份包内未找到 {ZIP_SYNC_ENTRY}，成对备份不完整"),
        ));
    }

    // 2. 探测备份库密文（文件即真相，issue #572），供口令消费与恢复后重置连接选择。
    let backup_encrypted = match db::encryption::probe_file_kind(&tmp_db) {
        Ok(kind) => kind == DbFileKind::Encrypted,
        Err(e) => {
            cleanup(&tmp_db);
            cleanup(&tmp_sync);
            return Err(e);
        }
    };

    // 3. 完整性 + 版本校验；密文备份在此凭主口令打开（缺口令报
    //    backup.passphrase-required，错误口令报 encryption.passphrase-incorrect），
    //    拒绝发生在安全备份与替换之前；备份旧于当前则迁移升级。成对产物的
    //    同步件同关口（完整性校验，失败同样发生在任何替换之前）。
    let backup_schema = match validate_backup(&tmp_db, expected_schema, passphrase) {
        Ok(v) => v,
        Err(e) => {
            cleanup(&tmp_db);
            cleanup(&tmp_sync);
            return Err(e);
        }
    };
    if paired && let Err(e) = validate_backup_sync_component(&tmp_sync, passphrase) {
        cleanup(&tmp_db);
        cleanup(&tmp_sync);
        return Err(e);
    }

    // 4. 安全备份当前库（恢复出错时可回滚）。文件级拷贝：当前库为密文时
    //    安全备份自然继承密文，凭同一主口令可回滚（issue #572 钉住）。
    //    同步元数据库移位去向记入 sync_moved（原位路径, 安全副本路径），
    //    替换失败时归位（见步骤 5）。
    let mut sync_moved: Option<(std::path::PathBuf, std::path::PathBuf)> = None;
    if db_path.exists() {
        std::fs::create_dir_all(safety_dir)?;
        let stamp = db::now_iso().replace([':', 'T'], "-");
        let safety = safety_dir.join(format!("restore-safety-{stamp}.db"));
        std::fs::copy(db_path, &safety)?;
        // 同步元数据库一并移位（ADR-0139 决策 3/6 配对纪律，issue #1869）：
        // 原位遗留的 sync.db 与恢复出的世界形态无涉——留在原地，重开挂载可能
        // 形态错配（如密文世界恢复明文备份）。按恢复安全备份命名语义移入安全
        // 目录保留（永不删除）；旧形态恢复的新世界 sync.db 由挂载接线按恢复后
        // 主库形态补建，成对恢复则由 sync 条目就位。
        let sync_path = db::sync_db_path(db_path);
        if sync_path.exists() {
            let sync_safety = safety_dir.join(format!("restore-safety-sync-{stamp}.db"));
            std::fs::rename(&sync_path, &sync_safety)?;
            tracing::info!(safety = %sync_safety.display(), "恢复前已移位当前同步元数据库");
            sync_moved = Some((sync_path, sync_safety));
        }
        tracing::info!(safety = %safety.display(), "恢复前已自动备份当前数据库");
    }

    // 5. 替换：成对恢复先换 sync.db（此刻主库未动，失败仅归位 sync.db 即回滚
    //    无痕），再换主库（既有失败语义：已移位/已替换的 sync.db 尽力归位）。
    let sync_path = db::sync_db_path(db_path);
    if paired && let Err(error) = replace_file(&tmp_sync, &sync_path) {
        if let Some((sync_path, sync_safety)) = &sync_moved
            && let Err(move_back) = std::fs::rename(sync_safety, sync_path)
        {
            tracing::error!(
                error = %move_back,
                "恢复 sync.db 替换失败后归位失败，保持移位现场"
            );
        }
        cleanup(&tmp_db);
        cleanup(&tmp_sync);
        return Err(error);
    }
    cleanup(&tmp_sync);
    let replace_result = replace_file(&tmp_db, db_path);
    cleanup(&tmp_db);
    if let Err(error) = replace_result {
        // 替换失败主库原样保留（ADR-0117 决策 3 恢复失败语义「回滚无痕」）：
        // 已移位/已替换的 sync.db 尽力归位，现场可重试；归位失败保持移位现场
        // 并报错（主库未动，原世界仍以「主库 + 安全副本 sync.db」自洽存在）。
        if let Some((sync_path, sync_safety)) = &sync_moved
            && let Err(move_back) = std::fs::rename(sync_safety, sync_path)
        {
            tracing::error!(
                error = %move_back,
                "恢复替换失败后同步元数据库归位失败，保持移位现场"
            );
        }
        return Err(error);
    }

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
    tracing::info!(schema = %backup_schema, paired, "恢复完成");
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
    let conn = open_backup_db_conn(tmp_db, passphrase)?;
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
        // 旧备份不做就地迁移升级（V036 拆库起，迁移链跨 attached 侧，ATTACH
        // 又不能在事务内执行——本连接是临时副本的无挂载连接，在它上面跑链
        // 会把四表搬进随连接消亡的库）。收敛点在恢复后的原位重引导：建连收尾
        // 先按世界版本挂载 sync.db（拆库前世界自动补建），init_db 重放待跑
        // 迁移（含 V036 搬迁）；恢复与重开之间中断同样由下次启动收敛。
        tracing::info!(
            backup_schema,
            expected_schema,
            "备份 schema 较旧，恢复后由原位重引导的建连迁移升级"
        );
    }
    Ok(backup_schema)
}

/// 成对备份的同步件校验：凭主口令打开（密文形态同主库）并做完整性检查——
/// 失败发生在任何替换之前（校验阶段拒绝，缺口令 / 错误口令 / 损坏的错误
/// 形态与主库校验同款）。
fn validate_backup_sync_component(tmp_sync: &Path, passphrase: Option<&str>) -> Result<()> {
    let conn = open_backup_db_conn(tmp_sync, passphrase)?;
    db::check_integrity(&conn)
}

/// 读取备份包的双库成对标记（恢复形态分支判据）：zip 内 `backup.json` 的
/// `paired` 字段；非 zip、元数据缺失或损坏一律按旧形态单文件备份（false）——
/// 恢复不因元数据而新增失败面，与既有行为一致。
fn backup_paired_flag(backup_path: &Path) -> bool {
    File::open(backup_path)
        .ok()
        .and_then(|file| zip::ZipArchive::new(file).ok())
        .and_then(|mut archive| read_meta_from_archive(&mut archive).ok())
        .map(|meta| meta.paired)
        .unwrap_or(false)
}

/// 从备份输入提取库文件到 `(out_main, out_sync)`：zip 包解出 `ledger.db` 与
/// `sync.db`（双文件成对产物；缺 sync 条目返回 false = 旧形态单文件备份），
/// 裸 `.db` 直接拷贝（旧形态，返回 false）。
fn extract_db_files(backup_path: &Path, out_main: &Path, out_sync: &Path) -> Result<bool> {
    let file = File::open(backup_path)?;
    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(_) => {
            // 非 zip：按裸 db 处理（旧形态）。
            std::fs::copy(backup_path, out_main)?;
            return Ok(false);
        }
    };

    let mut found_main = false;
    let mut found_sync = false;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name()?.into_owned();
        if name == ZIP_DB_ENTRY {
            let mut out_file = File::create(out_main)?;
            std::io::copy(&mut entry, &mut out_file)?;
            found_main = true;
        } else if name == ZIP_SYNC_ENTRY {
            let mut out_file = File::create(out_sync)?;
            std::io::copy(&mut entry, &mut out_file)?;
            found_sync = true;
        }
    }
    if !found_main {
        return Err(AppError::coded(
            "backup.db-entry-missing",
            format!("备份包内未找到 {}，不是有效的开源记账备份", ZIP_DB_ENTRY),
        ));
    }
    Ok(found_sync)
}
