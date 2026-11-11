//! 数据安全与连接接缝（#2054 设置②）：备份管理全量面（列表 / 清理 / 自动
//! 备份状态 / 恢复）、加密状态与开启、多端同步（状态 / 立即同步 / 通道
//! 配置 / 测试连接 / 检查点 / 引导）与账本注册表（清单 / 新建 / 切换 /
//! 改名 / 移除）直调域与基础设施入口——同步写语义不经本壳面（同步域
//! 编排照旧），本壳只做触发与呈现。

use std::path::{Path, PathBuf};

use ledger_infra::db::DbState;
use ledger_infra::error::{AppError, Result};

use super::CODE_SESSION_MISSING;
use super::boot_record;
use super::conn_lock;

// ---------------------------------------------------------------------------
// 数据安全与连接接缝（#2054 设置②）：备份管理全量面（列表 / 清理 / 自动备份
// 状态 / 恢复）、加密状态与开启、多端同步（状态 / 立即同步 / 通道配置 /
// 测试连接 / 检查点 / 引导）与账本注册表（清单 / 新建 / 切换 / 改名 / 移除）
// 直调域与基础设施入口——与旧壳 create_backup、restore_backup、list_backups、
// prune_backups、get_auto_backup_state、set_auto_backup_enabled、get_backup_meta、
// get_encryption_status、enable_encryption、get_sync_status、sync_now、
// get_parked_ops、get/set_sync_channel_config、test_sync_channel_connection、
// get/publish_sync_checkpoint、bootstrap_sync_from_channel、list/create/switch/
// rename/remove_book 命令消费的同一域函数，不经 IPC。写连接是成对建连的写
// 侧，读连接是只读侧（ADR-0117）。同步写语义不经本壳面（同步域编排照旧），
// 本壳只做触发与呈现（issue #2054 实施决策）。
// ---------------------------------------------------------------------------

use ledger_backup::{BackupFileInfo, BackupMetaSummary, PruneResult, RestoreResult};
use ledger_sync_engine::{SyncChannelConfig, SyncRoundReport};

/// 当前生效账本库文件路径（旧壳 `active_db_path` 的本壳对应面）：引导记录
/// 登记的生效目录（设置卡数据位置的同一读源）为单一来源；未登记（极端时序）
/// 回退启动参数目录（active_db_path 的 default_data_dir 兜底同形）。
fn db_path_for_data_safety() -> Result<PathBuf> {
    let dir = match boot_record() {
        Some(record) => record.db_dir,
        None => crate::app::book_dir()
            .ok_or_else(|| AppError::coded(CODE_SESSION_MISSING, "内部错误：账本库会话缺失"))?,
    };
    Ok(dir.join(ledger_infra::db::data_location::DB_FILE_NAME))
}

/// 当前活动账本标识（旧壳 `active_book_id` 的本壳对应面）：引导快照注册表的
/// 活动指针；注册表不可用（损坏回退）回 `None`——钥匙串条目分域与多端同步
/// 门禁的同一兜底口径。
fn active_book_id() -> Option<String> {
    boot_record()
        .as_ref()
        .and_then(|record| record.registry.as_ref())
        .map(|registry| registry.active_id.clone())
}

// —— 备份管理（备份历史、滚动清理、自动备份状态与恢复） ——

/// 受管备份行集（旧壳 `list_backups` 命令同一执行协议）：备份目录内当前账本
/// （指纹 × 设备）可归属的产物，按新→旧排序。作用域按 **账本指纹** 分域
/// （ADR-0144 决策 3，[`ledger_backup::BackupScope::of_conn`]）；指纹未固化
/// （空库）= 空列表（本账尚无任何可归属产物）。读连接是只读侧。
pub fn backup_rows(db: &DbState, dir: &Path) -> Result<Vec<BackupFileInfo>> {
    let conn = conn_lock(&db.read_conn)?;
    match ledger_backup::BackupScope::of_conn(&conn)? {
        Some(scope) => ledger_backup::list_managed_backups(dir, Some(&scope)),
        None => Ok(Vec::new()),
    }
}

/// 滚动清理（旧壳 `prune_backups` 命令同一执行协议）：把（本指纹 × 本设备）
/// 桶内超额产物修剪到 `keep`。保留上限非法（0）报旧壳同码
/// `backup.keep-invalid`（UI 输入钳 1–100，码化拒绝是口径兜底）。文件删除是
/// 阻塞 IO，调用方在 blocking 任务内消费；删除结果由调用方触发列表重读（旧
/// 壳 `ledger:backups-changed` 信号的壳内对应物）。
pub fn prune_backups(db: &DbState, dir: &Path, keep: usize) -> Result<PruneResult> {
    if keep == 0 {
        return Err(AppError::coded(
            "backup.keep-invalid",
            format!("备份保留上限非法: {keep}"),
        ));
    }
    let conn = conn_lock(&db.read_conn)?;
    match ledger_backup::BackupScope::of_conn(&conn)? {
        Some(scope) => ledger_backup::prune_managed_backups(dir, keep, Some(&scope)),
        None => Ok(PruneResult {
            kept: 0,
            deleted: Vec::new(),
            failed: Vec::new(),
        }),
    }
}

/// 本机设备标识（旧壳 `get_device_id` 命令同一执行协议）：清理确认弹窗待删
/// 数量「本指纹 × 本设备」桶判定的比对基准；不可得（attached 侧异常）=
/// `None`，保守不误报（旧壳同口径）。
pub fn backup_device_id(db: &DbState) -> Option<String> {
    let conn = conn_lock(&db.read_conn).ok()?;
    ledger_backup::source_device_id(&conn)
}

/// 自动备份状态（旧壳 `get_auto_backup_state` 命令同一执行协议）：开关 +
/// 上次自动备份时刻 + 连续失败次数与提示态（阈值判定归域单点
/// [`ledger_backup::failure_alerting`]，本壳不复刻）。备份目录是本壳平台
/// 存储偏好（旧壳 localStorage 同位），不经库。
pub fn auto_backup_state(db: &DbState) -> Result<ledger_backup::AutoBackupState> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_backup::get_state(&conn)
}

/// 自动备份开关连续失败提示态（与 [`auto_backup_state`] 同源的阈值投影单点）。
pub fn auto_backup_failure_alerting(consecutive_failures: u32) -> bool {
    ledger_backup::failure_alerting(consecutive_failures)
}

/// 设置自动备份开关（旧壳 `set_auto_backup_enabled` 命令同一执行协议）：写
/// `app_settings` 经基建设置接缝（设置 KV 置脏豁免，ADR-0032 随接缝自带）。
/// 开关由自动备份调度消费（宿主已随 #2078 落地：调度线程每轮读库内状态，
/// 关闭即静默跳过，三触发入口统一尊重），写入是跨壳共享的库内状态。
pub fn set_auto_backup_enabled(db: &DbState, enabled: bool) -> Result<()> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    ledger_infra::settings::set(
        &conn,
        ledger_infra::settings::SettingKey::AutoBackupEnabled,
        &enabled,
    )
}

/// 备份包元数据摘要（旧壳 `get_backup_meta` 命令同一执行协议）：恢复确认
/// 弹窗的加密标记与来源标记（跨模式警告与口令输入判定）。纯文件探测。
pub fn backup_meta(path: &Path) -> Result<BackupMetaSummary> {
    ledger_backup::probe_backup_meta(path)
}

/// 从备份文件恢复（旧壳 `restore_backup` 命令同一执行协议）：安全备份 →
/// 整库替换 → 读连接占位化。目标库路径 = 引导生效目录（DataLocation 解析的
/// 单点，与旧壳 effective_db_dir 同语义），恢复安全备份落应用数据根
/// （RestoreSafetyBackup 既有语义；本壳数据根与旧壳同一 XDG 拼接根）。恢复
/// 期间持写连接锁（阻塞并发写，旧壳同口径）；成功后立刻占位化读连接（旧
/// inode 读句柄只给陈旧数据，issue #1280 / ADR-0117 决策 3），本壳无原位重
/// 引导机制，重启语义由状态条提示承担（偏差见交付报告）。任务无连接外格
/// 外持锁面：全部步骤在同一 blocking 任务内。
pub fn restore_backup(
    db: &DbState,
    backup_path: &Path,
    passphrase: Option<&str>,
) -> Result<RestoreResult> {
    let db_path = db_path_for_data_safety()?;
    let safety_dir = crate::cross_book::default_data_dir()
        .ok_or_else(|| AppError::Io("应用数据根不可解析，无法保存恢复安全备份".to_owned()))?;
    // 恢复期间持有写连接锁（阻塞本地 HTTP/IPC 并发写的同位语义：本壳的并发
    // 写面 = 域直调写路径 + 同步轮次，同吃这把锁）。
    let _guard = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let expected = ledger_backup::expected_schema_version()?;
    let result =
        ledger_backup::restore_db_from(backup_path, &db_path, &safety_dir, expected, passphrase);
    if result.is_ok() {
        // 文件替换成功：读连接立即占位化（同旧壳 detach_read_conn；失败仅记
        // 日志，不改变恢复结果，旧读连接占位化失败的行为同口径）。
        if let Err(e) = db.placeholderize_read_conn() {
            tracing::error!(error = %e, "恢复后读连接占位化失败，重启前读命令将持续报错");
        }
    }
    result
}

// —— 加密（状态与整库三形态写面：开启 / 修改主口令 / 关闭，issue #2088） ——

/// 加密状态（旧壳 `get_encryption_status` 命令同一执行协议）：库文件头探测
/// （文件即真相，ADR-0075 决策 4）。锁定态在本壳结构性不可达——设置卡只在
/// 业务就绪后可达（会话已就位、门已开），锁定现场的呈现面是解锁面板
/// （[`crate::unlock`]），不经本函数。
pub fn encryption_status() -> Result<bool> {
    let file_encrypted = ledger_infra::db::encryption::probe_file_kind(&db_path_for_data_safety()?)?
        == ledger_infra::db::encryption::DbFileKind::Encrypted;
    Ok(file_encrypted)
}

/// 整库转换后的原位换连（旧壳转换三件的 `detach_read_conn` + 前端重启的
/// 本壳替代形态，issue #2088 裁决：本壳交付换连编排，转换成功后按新形态
/// 开新连接对原位换入，会话零中断不重启——避免「提示重启」形态下写连接
/// 指旧 inode、重启后丢写的窗口）：凭 `passphrase`（密文形态；`None` =
/// 明文形态）开新连接对，会话信封按新形态记入（位次纪律首行——换连前记
/// 入，窗口内写后触发不致携旧形态封包，ADR-0098）→ [`DbState::swap_pair`]
/// 成对换入（ADR-0117 决策 3 单点，禁手据第二副本）。门保持开启（转换发
/// 生在已解锁会话内，口令在场）。
///
/// 新对打开失败（极端异常：试开验证已成功过）→ 读连接占位化（旧 inode
/// 防线）+ 错误上抛，调用方按失败呈现（重启恢复）。
fn swap_converted_session(db: &DbState, db_path: &Path, passphrase: Option<&str>) -> Result<()> {
    let opened = match passphrase {
        Some(pass) => {
            let conn = ledger_infra::db::open_connection_with_passphrase(db_path, pass);
            let read_conn = ledger_infra::db::open_connection_readonly_with_passphrase(db_path, pass);
            conn.and_then(|conn| {
                read_conn.map(|read_conn| {
                    (
                        conn,
                        read_conn,
                        ledger_sync_engine::SessionEnvelope::Encrypted(pass.to_owned()),
                    )
                })
            })
        }
        None => {
            let conn = ledger_infra::db::open_connection(db_path);
            let read_conn = ledger_infra::db::open_connection_readonly(db_path);
            conn.and_then(|conn| {
                read_conn.map(|read_conn| {
                    (
                        conn,
                        read_conn,
                        ledger_sync_engine::SessionEnvelope::Plaintext,
                    )
                })
            })
        }
    };
    match opened {
        Ok((conn, read_conn, envelope)) => {
            ledger_sync_engine::SessionEnvelope::remember(envelope);
            db.swap_pair(conn, read_conn)
        }
        Err(e) => {
            if let Err(pe) = db.placeholderize_read_conn() {
                tracing::error!(error = %pe, "转换后新连接对打开失败且读连接占位化也失败，重启前读命令将持续报错");
            }
            Err(e)
        }
    }
}

/// 开启加密（旧壳 `enable_encryption` 命令同一执行协议）：当前明文库整库
/// 一次性转换为密文库（转换与原子性语义全在基础设施
/// [`encryption::enable_encryption_for_file`] 单正文，失败原库原样保留），
/// 成功后原位换连继续会话（[`swap_converted_session`]，本会话零中断）。
/// 转换期间持写槽锁（阻塞并发写写进转换中的源文件，恢复通道同口径）。
pub fn enable_encryption(db: &DbState, passphrase: &str) -> Result<()> {
    let db_path = db_path_for_data_safety()?;
    {
        // 写槽锁只护转换段（阻塞并发写写进转换中的源文件）；换连段不得持
        // 锁——swap_pair 换写槽要取同一互斥体（持锁换连 = 自锁死）。
        let _guard = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        ledger_infra::db::encryption::enable_encryption_for_file(&db_path, passphrase)?;
    }
    swap_converted_session(db, &db_path, Some(passphrase))
}

/// 修改主口令（旧壳 `change_encryption_passphrase` 命令同一执行协议，
/// issue #571）：旧口令验证通过后整库转入新口令的新库（失败原库原样
/// 保留），成功后凭新口令原位换连继续会话（[`swap_converted_session`]）。
/// 转换期间持写槽锁（同开启加密）。
pub fn change_encryption_passphrase(
    db: &DbState,
    current_passphrase: &str,
    new_passphrase: &str,
) -> Result<()> {
    let db_path = db_path_for_data_safety()?;
    {
        // 写槽锁只护转换段（同开启加密；换连段不得持锁，见上）。
        let _guard = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        ledger_infra::db::encryption::change_passphrase_for_file(
            &db_path,
            current_passphrase,
            new_passphrase,
        )?;
    }
    swap_converted_session(db, &db_path, Some(new_passphrase))
}

/// 关闭加密（旧壳 `disable_encryption` 命令同一执行协议，issue #571）：凭
/// 当前主口令整库转回明文库（失败原库原样保留），成功后原位换连到明文
/// 新世界（[`swap_converted_session`]）。转换期间持写槽锁（同开启加密）。
pub fn disable_encryption(db: &DbState, current_passphrase: &str) -> Result<()> {
    let db_path = db_path_for_data_safety()?;
    {
        // 写槽锁只护转换段（同开启加密；换连段不得持锁，见上）。
        let _guard = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        ledger_infra::db::encryption::disable_encryption_for_file(&db_path, current_passphrase)?;
    }
    swap_converted_session(db, &db_path, None)
}

/// 主口令验证（信封模式解析的消费面，与旧壳 `resolve_passphrase` 同序）。
pub fn verify_source_passphrase_for_sync(db_path: &Path, passphrase: &str) -> Result<()> {
    ledger_infra::db::encryption::verify_source_passphrase(db_path, passphrase)
}

/// 本机已记住主口令的读取（信封模式解析的钥匙串回退面；发布构建的生物门
/// 在 v1 非 macOS 平台不支持，本壳按 infra 既有语义只投 Found/NotFound 路径）。
pub fn remembered_sync_passphrase(book: Option<&str>) -> Result<Option<String>> {
    match ledger_infra::db::passphrase_cache::load(book)? {
        ledger_infra::db::passphrase_cache::CacheLoad::Found(pass) => Ok(Some(pass)),
        _ => Ok(None),
    }
}

// —— 多端同步（状态 / 立即同步 / 通道配置 / 测试连接 / 检查点 / 引导） ——

/// 同步状态投影（旧壳 `get_sync_status` 命令响应体的本壳消费面；设备标识
/// 不在本壳呈现面，投影不携带——偏差见交付报告）。
#[derive(Clone, Debug, PartialEq)]
pub struct SyncStatusSeam {
    /// 通道是否已配置。
    pub channel_configured: bool,
    /// 上次成功同步时刻（UTC ISO；从未同步 = None）。
    pub last_sync_at: Option<String>,
    /// 挂起 op 数量。
    pub parked_count: usize,
    /// 本库是否密文形态（信封模式显性提示依据）。
    pub library_encrypted: bool,
}

/// 读取同步状态（旧壳 `get_sync_status` 命令同一执行协议）：通道配置在位、
/// 上次成功同步时刻、挂起数量与库加密形态（文件头探测）。读路径零副作用；
/// 缺 key / 缺表回默认。
pub fn sync_status(db: &DbState) -> Result<SyncStatusSeam> {
    let conn = conn_lock(&db.read_conn)?;
    let channel_configured = ledger_sync_engine::configured_channel(&conn)?.is_some();
    let last_sync_at = ledger_infra::settings::get(
        &conn,
        ledger_infra::settings::SettingKey::SyncLastSyncAt,
        None,
    )?;
    let parked_count = ledger_sync_engine::parked_ops(&conn)?.len();
    drop(conn);
    let library_encrypted = encryption_status()?;
    Ok(SyncStatusSeam {
        channel_configured,
        last_sync_at,
        parked_count,
        library_encrypted,
    })
}

/// 挂起操作行集（旧壳 `get_parked_ops` 命令同一执行协议）：本轮挂起的明细
/// 挂接面；呈现用已渲染中文详情（[`ledger_sync_engine::ParkedOp::message`]）。
pub fn parked_rows(db: &DbState) -> Result<Vec<ledger_sync_engine::ParkedOp>> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_sync_engine::parked_ops(&conn)
}

/// 通道配置读取（旧壳 `get_sync_channel_config` 命令同一执行协议）：未配置
/// 回 `None`；缺 key / 缺表按设置域读口径回默认。
pub fn sync_channel_config(db: &DbState) -> Result<Option<SyncChannelConfig>> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_sync_engine::configured_channel(&conn)
}

/// 保存通道配置（旧壳 `set_sync_channel_config` 命令同一执行协议）：端点非
/// https 拒绝——用户输入门与旧壳命令层同位；同步空间空白归默认空间）→
/// 凭据构库校验（构库单点不发网络请求）→ 经设置接缝落 `app_settings`。
/// 两步校验通过才写入；失败零写入（不产生半配置态）。写连接是写侧。
pub fn save_sync_channel_config(db: &DbState, mut config: SyncChannelConfig) -> Result<()> {
    let endpoint = config.endpoint.trim().to_owned();
    if !endpoint.is_empty() && !endpoint.to_ascii_lowercase().starts_with("https://") {
        return Err(AppError::coded(
            "sync-channel.endpoint-insecure",
            "同步通道端点必须使用 https 地址",
        ));
    }
    config.endpoint = endpoint;
    if config.space_id.trim().is_empty() {
        config.space_id = ledger_sync_engine::trigger::DEFAULT_SPACE_ID.to_owned();
    }
    ledger_sync_engine::build_channel(&config)?;
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    ledger_infra::settings::set(
        &conn,
        ledger_infra::settings::SettingKey::SyncChannelConfig,
        &config,
    )
}

/// 保存前「测试连接」（旧壳 `test_sync_channel_connection` 命令同一执行协
/// 议）：当前表单尚未落库的配置做一次只读探针，零持久化副作用。用户输入门
/// （非 https 拒绝）与构库校验同保存路径同一单点。
pub fn test_sync_connection(config: &SyncChannelConfig) -> Result<()> {
    let endpoint = config.endpoint.trim().to_owned();
    if !endpoint.is_empty() && !endpoint.to_ascii_lowercase().starts_with("https://") {
        return Err(AppError::coded(
            "sync-channel.endpoint-insecure",
            "同步通道端点必须使用 https 地址",
        ));
    }
    let mut probe = config.clone();
    probe.endpoint = endpoint;
    ledger_sync_engine::probe_channel(&probe)
}

/// 轮次连接源（ADR-0120 决策 4 的本壳形态）：把 DbState 写连接槽包成轮次
/// 分段接缝——每个数据库段短暂取一次连接、用完即还，网络段不消费连接
/// （与旧壳 MainConnSegments 同形状）；持锁时长探针与 #1276 守门口径一致。
struct ShellRoundConn {
    conn: std::sync::Arc<std::sync::Mutex<rusqlite::Connection>>,
    key: u64,
}

impl ledger_sync_engine::RoundConn for ShellRoundConn {
    fn with_connection<R, F>(
        &self,
        _segment: ledger_sync_engine::ConnSegment,
        use_connection: F,
    ) -> Result<R>
    where
        F: FnOnce(&rusqlite::Connection) -> Result<R>,
    {
        let hold_started = std::time::Instant::now();
        let guard = self.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        let result = use_connection(&guard);
        ledger_infra::db::probe_lock_hold(hold_started.elapsed());
        result
    }

    fn round_key(&self) -> u64 {
        self.key
    }
}

/// 手动同步一轮（旧壳 `sync_now` 命令同一执行协议）：通道读取（短取锁）→
/// 构库 → 主口令解析（明文库明文直通；密文库凭显式口令或本机缓存，先验证
/// 后封包）→ 轮次编排归域（在途互斥 / 幂等重放 / 挂起语义在域内单点）。
/// 成功后记会话信封（旧壳 SessionEnvelope::remember 同语义保留）。「上次
/// 成功同步时刻」由域内整体裁决点落库
/// （旧壳同点）；报告 `applied > 0` 的失效信号由视图层广播（本函数零信号
/// 副作用）。
pub fn sync_now(db: &DbState, passphrase: Option<&str>) -> Result<SyncRoundReport> {
    let db_path = db_path_for_data_safety()?;
    let book = active_book_id();
    // 通道读取 + 未配置 / 账本門禁（短取读连接）。
    let config = {
        let conn = conn_lock(&db.read_conn)?;
        ledger_sync_engine::configured_channel(&conn)?
            .ok_or_else(ledger_sync_engine::trigger::not_configured_error)?
    };
    if book.is_none() {
        return Err(ledger_sync_engine::trigger::book_unavailable_error());
    }
    let channel = ledger_sync_engine::build_channel(&config)?;
    let holder = passphrase_holder(db_path.as_path(), book.as_deref(), passphrase)?;
    let mode = match &holder {
        Some(holder) => ledger_sync_engine::EnvelopeMode::Encrypted { passphrase: holder },
        None => ledger_sync_engine::EnvelopeMode::Plaintext,
    };
    let locks = ShellRoundConn {
        conn: std::sync::Arc::clone(&db.conn),
        key: ledger_sync_engine::connection_round_key(&db.conn),
    };
    let report = ledger_sync_engine::run_round_once(&locks, &channel, &mode)?;
    // 成功轮次记入本会话形态（域语义保留：手动同步后不再需要重新触口令）。
    match &holder {
        Some(holder) => ledger_sync_engine::SessionEnvelope::remember(
            ledger_sync_engine::SessionEnvelope::Encrypted(holder.clone()),
        ),
        None => ledger_sync_engine::SessionEnvelope::remember(
            ledger_sync_engine::SessionEnvelope::Plaintext,
        ),
    }
    Ok(report)
}

/// 主口令解析（旧壳 `resolve_passphrase` 同序单点）：明文库回 `None`（明文
/// 直通）；密文库回 `Some`（显式口令优先，其次本机缓存，均不可得报
/// `sync-channel.passphrase-required`），先验证后封包（错误口令封出的段对端
/// 无法解封，必须在上传前拦下——`encryption.passphrase-incorrect` 同码）。
fn passphrase_holder(
    db_path: &Path,
    book: Option<&str>,
    passphrase: Option<&str>,
) -> Result<Option<String>> {
    if ledger_infra::db::encryption::probe_file_kind(db_path)?
        != ledger_infra::db::encryption::DbFileKind::Encrypted
    {
        return Ok(None);
    }
    let passphrase = match passphrase {
        Some(passphrase) => passphrase.to_owned(),
        None => remembered_sync_passphrase(book)?.ok_or_else(|| {
            AppError::coded(
                "sync-channel.passphrase-required",
                "同步通道上有加密数据，需要主口令才能读取",
            )
        })?,
    };
    ledger_infra::db::encryption::verify_source_passphrase(db_path, &passphrase)?;
    Ok(Some(passphrase))
}

/// 通道检查点指针（旧壳 `get_sync_channel_checkpoint` 命令同一执行协议）：
/// 只读 manifest 不下载快照体，引导向导首步的预检。通道未配置报旧壳同码
/// `sync-channel.not-configured`。网络等待不消费连接（配置短取读锁）。
pub fn sync_channel_checkpoint(
    db: &DbState,
) -> Result<Option<ledger_sync_engine::CheckpointPointer>> {
    let config = {
        let conn = conn_lock(&db.read_conn)?;
        ledger_sync_engine::configured_channel(&conn)?
            .ok_or_else(ledger_sync_engine::trigger::not_configured_error)?
    };
    let channel = ledger_sync_engine::build_channel(&config)?;
    channel.checkpoint_pointer()
}

/// 发布检查点（旧壳 `publish_sync_checkpoint` 命令同一执行协议）：快照产出
/// （写连接锁只盖这一步，位点与快照同刻成对）→ 封包与三次通道往返在锁外。
/// 本地数据零变化（零信号，通道操作而非账本写入，旧壳同口径）。
pub fn publish_sync_checkpoint(
    db: &DbState,
    passphrase: Option<&str>,
) -> Result<SyncCheckpointPublished> {
    let db_path = db_path_for_data_safety()?;
    let book = active_book_id();
    let config = {
        let conn = conn_lock(&db.read_conn)?;
        ledger_sync_engine::configured_channel(&conn)?
            .ok_or_else(ledger_sync_engine::trigger::not_configured_error)?
    };
    if book.is_none() {
        return Err(ledger_sync_engine::trigger::book_unavailable_error());
    }
    let channel = ledger_sync_engine::build_channel(&config)?;
    let holder = passphrase_holder(db_path.as_path(), book.as_deref(), passphrase)?;
    let mode = match &holder {
        Some(holder) => ledger_sync_engine::EnvelopeMode::Encrypted { passphrase: holder },
        None => ledger_sync_engine::EnvelopeMode::Plaintext,
    };
    // 锁内段：快照产出（主连接互斥，随语句块立即释放）。
    let checkpoint = {
        let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        ledger_sync_engine::create_checkpoint(&conn)?
    };
    // 锁外段：封包（KDF）与通道网络往返。
    let pointer = channel.upload_checkpoint(&checkpoint, &mode)?;
    Ok(SyncCheckpointPublished {
        generation: pointer.generation,
        size: pointer.size,
        plaintext_mode: mode.is_plaintext(),
    })
}

/// 发布结果（旧壳 `SyncCheckpointPublished` 同形；视图提示的数据面）。
#[derive(Clone, Debug)]
pub struct SyncCheckpointPublished {
    /// 本次发布的代数。
    pub generation: i64,
    /// 密文字节数。
    pub size: u64,
    /// 明文模式标记（快照明文上通道，界面显著提示依据）。
    pub plaintext_mode: bool,
}

/// 引导主连接段接缝（域 [`ledger_sync_engine::BootstrapConnSegments`] 的本壳
/// 唯一实现，与旧壳 MainConnSegments 同形）：每段短取一次写连接——锁失败映
/// 射与持锁时长探针内化此处。
struct ShellBootstrapSegments {
    conn: std::sync::Arc<std::sync::Mutex<rusqlite::Connection>>,
}

impl ledger_sync_engine::BootstrapConnSegments for ShellBootstrapSegments {
    fn with_conn<T>(
        &self,
        use_conn: impl FnOnce(&mut rusqlite::Connection) -> Result<T>,
    ) -> Result<T> {
        let hold_started = std::time::Instant::now();
        let mut conn = self.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        let result = use_conn(&mut conn);
        ledger_infra::db::probe_lock_hold(hold_started.elapsed());
        result
    }
}

/// 从通道检查点引导（旧壳 `bootstrap_sync_from_channel` 命令同一执行协议）：
/// 拉取通道当前检查点并整库换入本机——「加入即新库」的显式向导动作，编排
/// 全在域单点 [`ledger_sync_engine::bootstrap_from_channel`]（前置守卫、拉
/// 取、复验、信封形态对齐、整库换入、簿记清理与转密文决策在域内），本壳只
/// 实现段接缝。整库换入后本壳无原位重引导，重启语义由状态条提示承担。
pub fn bootstrap_sync_from_channel(
    db: &DbState,
    passphrase: Option<&str>,
) -> Result<ledger_sync_engine::BootstrapOutcome> {
    let db_path = db_path_for_data_safety()?;
    let config = {
        let conn = conn_lock(&db.read_conn)?;
        ledger_sync_engine::configured_channel(&conn)?
            .ok_or_else(ledger_sync_engine::trigger::not_configured_error)?
    };
    let channel = ledger_sync_engine::build_channel(&config)?;
    let segments = ShellBootstrapSegments {
        conn: std::sync::Arc::clone(&db.conn),
    };
    ledger_sync_engine::bootstrap_from_channel(&segments, &db_path, &channel, passphrase)
}

// —— 账本注册表（清单 / 新建 / 切换 / 改名 / 移除；账本切换卡消费面） ——

/// 账本清单聚合（旧壳 `list_books` 命令同一执行协议）：登记清单、活动指针、
/// 登记变更可用性与回退警示。清单读注册表**最新落盘态**（登记变更落盘后立
/// 即可见）；本壳默认数据目录 = 启动参数账本目录（#2053 身份即目录的同一
/// 契约），聚合与数据位置卡同归口。
pub fn book_list(launch_dir: &Path) -> ledger_infra::db::book_registry::BookListInfo {
    ledger_infra::db::data_location::gather_book_list(launch_dir, boot_record().as_ref())
}

/// 新建账本（旧壳 `create_book` 命令同一执行协议）：登记变更预检 → 引导内核
/// 登记（自动建子目录、查重拒绝、首次登记落新格式全在域内单正文）。返回
/// 登记后的账本条目。
pub fn create_book(launch_dir: &Path, name: &str) -> Result<ledger_infra::db::book_registry::Book> {
    ledger_infra::db::data_location::mutable_registry(boot_record().as_ref())?;
    ledger_infra::db::book_registry::create_book_entry(launch_dir, name)
}

/// 切换活动账本（旧壳 `switch_book` 命令同一执行协议）：登记变更预检 → 写
/// 活动指针落盘。重引导（进目标账本）本壳无原位机制——重启语义由状态条
/// 提示承担（旧壳 restart_app 的偏差见交付报告）。
pub fn switch_book(launch_dir: &Path, id: &str) -> Result<ledger_infra::db::book_registry::Book> {
    ledger_infra::db::data_location::mutable_registry(boot_record().as_ref())?;
    ledger_infra::db::book_registry::switch_active_book(launch_dir, id)
}

/// 改账本展示名（旧壳 `rename_book` 命令同一执行协议）：只动注册表元数据，
/// 目录与库文件零变化。
pub fn rename_book(
    launch_dir: &Path,
    id: &str,
    name: &str,
) -> Result<ledger_infra::db::book_registry::Book> {
    ledger_infra::db::data_location::mutable_registry(boot_record().as_ref())?;
    ledger_infra::db::book_registry::rename_book_entry(launch_dir, id, name)
}

/// 移除账本（旧壳 `remove_book` 命令同一执行协议）：仅摘登记，目录与库文件
/// 原样保留（可重新登记找回）；活动账本不可移除在域内核拒绝。
pub fn remove_book(launch_dir: &Path, id: &str) -> Result<()> {
    ledger_infra::db::data_location::mutable_registry(boot_record().as_ref())?;
    ledger_infra::db::book_registry::remove_book_entry(launch_dir, id)
}
