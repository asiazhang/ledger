//! 多端同步命令壳（issue #862 / #863 / #864 / ADR-0091）：同步状态查询、手动同步
//! 轮次、通道配置（WebDAV 凭据）与检查点发布/预检/引导（新端加入向导的命令面）。
//!
//! 只做参数解包、凭据/信封模式解析与轮次编排一行调用；通道布局、轮次协议、
//! 幂等重放、挂起语义与触发编排全在 [`crate::sync_engine`]（域不依赖壳，
//! ADR-0056），本文件不含业务语义。
//!
//! - `sync_now` 写路径经统一写入口 [`crate::write_entry::write_entry`]（ADR-0073）：
//!   重放是行为编排之外的第 N 写入入口（ADR-0091，接缝契约与批量导入同待遇），
//!   外来 op 实际应用即账本数据变化——经 [`WriteOp::SyncRound`] 条件发参考失效
//!   信号（证据 [`WriteEvidence::LedgerApplied`]），置脏照常在提交点发生。
//!   轮次期间持主连接锁（与 `sync_instrument_info` 网络同步同形）：通道轮次的
//!   发布/拉取/重放本就要求单连接互斥（checkpoint 成对约束同源），失败上抛
//!   不产生部分本地状态（已上传段内容确定等同，重试轮次幂等续作）。
//!   轮次编排本身归域（`sync_engine::trigger`，issue #863）：本壳只解包口令、
//!   解析信封模式并把轮次报告原样交出。
//! - `set_sync_channel_config` 写 `app_settings` 经 [`crate::settings`] 单点收口
//!   （置脏豁免，ADR-0032）：WebDAV 凭据是本机设备配置（不同步，同步边界见
//!   多端同步域 SyncBoundary），写操作身份 `SetSyncChannelConfig` 以例外白名单
//!   登记（见 `signals_cross_check`）；校验借域侧 `build_channel` 单点
//!   （凭据构库 + 空间闭集，不发网络请求），错误码复用域的 `sync-channel.*`。
//!   同步世界身份（通道目录 `book-<space>`）由同步空间字段显式约定——账本登记
//!   标识按 ADR-0089 是本机事实，跨设备不成立；两端配置同一空间即对齐同一世界
//!   （v1 默认 `default`，多账本各自独立同步 = 各配一个空间）。
//! - 信封模式按本库加密形态决定（复用备份域加密模式，ADR-0091 决策 8）：密文
//!   库凭主口令封包——显式参数优先，其次本机已记住口令（钥匙串），均不可得报
//!   `sync-channel.passphrase-required`；明文库明文直通，轮次报告携带
//!   `plaintext_mode` 供界面显著提示。取得的口令同时记入本机会话口令
//!   （issue #863）：解锁与一次成功的手动同步之后，打开应用即同步与低频轮询
//!   无需再触钥匙串即可封包。主口令/凭据不落日志与 trace（ADR-0075，lib.rs
//!   载荷脱敏单点遮蔽 `passphrase` 与 `password` 字段）。

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime};

use crate::commands::encryption::{active_book_id, active_db_path};
use crate::db::encryption::{
    DbFileKind, enable_encryption_for_file, probe_file_kind, verify_source_passphrase,
};
use crate::db::passphrase_cache::{self, CacheLoad};
use crate::db::{DbState, run_db};
use crate::error::{AppError, Result};
use crate::settings::{self, SettingKey};
use crate::signals::{WriteEvidence, WriteOp};
use crate::sync_engine::trigger::{
    DEFAULT_SPACE_ID, book_unavailable_error, build_channel, configured_channel,
    not_configured_error, run_round_once,
};
use crate::sync_engine::{
    EnvelopeMode, SessionEnvelope, SyncChannelConfig, SyncRoundReport, bootstrap_from_checkpoint,
    bootstrap_preflight, device_id, parked_ops,
};
use crate::write_entry::{Outcome, write_entry};

/// 通道配置回显（设置页通道配置表单，issue #862）：未配置时各字段为空串、
/// `configured = false`，表单按空表单起填（空间字段由前端填默认值提示）。
#[derive(Debug, Serialize)]
pub struct SyncChannelConfigState {
    /// 同步根目录 URL。
    pub base_url: String,
    /// WebDAV 账号。
    pub username: String,
    /// WebDAV 密码 / 应用密码（本机配置回显；响应体不经日志与 trace）。
    pub password: String,
    /// 同步空间（跨端共识的世界身份）。
    pub space_id: String,
    /// 是否已配置过（区分「空表单」与「保存过空值」的表单初态依据）。
    pub configured: bool,
}

/// 通道配置写入参数（表单提交形态）。
#[derive(Debug, Deserialize)]
pub struct SyncChannelConfigInput {
    /// 同步根目录 URL。
    pub base_url: String,
    /// WebDAV 账号。
    pub username: String,
    /// WebDAV 密码 / 应用密码。
    pub password: String,
    /// 同步空间（缺省回 `default`；两端填同一值即同步同一世界）。
    pub space_id: Option<String>,
}

/// 同步状态（设置页同步卡片回显，issue #862）。
#[derive(Debug, Serialize)]
pub struct SyncStatusState {
    /// 本机设备标识（首用生成并持久化，参与全序 tiebreak）。
    pub device_id: String,
    /// 通道是否已配置（WebDAV 凭据已保存）。
    pub channel_configured: bool,
    /// 上次成功同步时刻（UTC ISO；从未同步为 `None`）。
    pub last_sync_at: Option<String>,
    /// 挂起 op 数量（不可重放、待用户裁决的操作）。
    pub parked_count: usize,
    /// 本库是否为密文形态（决定信封加密与明文提示：密文库凭主口令封包，
    /// 明文库明文上通道——界面须显著提示）。
    pub library_encrypted: bool,
}

/// 读取同步状态（issue #862）：设备标识、通道配置在位、上次成功同步时刻、
/// 挂起数量与库加密形态。读路径零副作用；缺 key / 缺表回默认（读路径不因
/// 缺 key 上抛，与设置域读命令同口径）。
#[tauri::command]
pub async fn get_sync_status<R: Runtime>(app: AppHandle<R>) -> Result<SyncStatusState> {
    let conn = app.state::<DbState>().conn.clone();
    let db_path = active_db_path(&app)?;
    run_db("get_sync_status", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        Ok(SyncStatusState {
            device_id: device_id(&conn)?,
            channel_configured: configured_channel(&conn)?.is_some(),
            last_sync_at: settings::get(&conn, SettingKey::SyncLastSyncAt, None)?,
            parked_count: parked_ops(&conn)?.len(),
            library_encrypted: probe_file_kind(&db_path)? == DbFileKind::Encrypted,
        })
    })
    .await
}

/// 手动同步（issue #862「立即同步」/ #863 轮次编排）：执行一次同步轮次——发布
/// 自己流新 op + 拉取他人流增量并经同步引擎幂等重放。返回轮次报告（上传/应用/
/// 挂起计数，前端据此轻量提示）；`plaintext_mode` 为真表示本轮明文上通道
/// （界面显著提示）。挂起明细经 [`get_parked_ops`] 查询（同步卡片回显面）。
///
/// 信封模式解析见模块文档；`passphrase` 为密文库下的显式主口令（不落日志，
/// 留空则回退本机已记住口令，均不可得报 `sync-channel.passphrase-required`）。
/// 通道未配置报 `sync-channel.not-configured`；账本注册表不可用报
/// `sync-channel.book-unavailable`。成功后随轮次事务更新「上次成功同步时刻」，
/// 并把口令记入本机会话（后续自动轮询无需再触钥匙串）。
#[tauri::command]
pub async fn sync_now<R: Runtime>(
    app: AppHandle<R>,
    passphrase: Option<String>,
) -> Result<SyncRoundReport> {
    let conn = app.state::<DbState>().conn.clone();
    let db_path = active_db_path(&app)?;
    let book = active_book_id(&app);
    write_entry(
        "sync_now",
        conn,
        Some(&app),
        WriteOp::SyncRound,
        move |conn| {
            // 通道在位性前置：未配置即早退，不触网。
            let config = configured_channel(conn)?.ok_or_else(not_configured_error)?;
            // 注册表在位性门禁（同步以活动账本为范围；世界身份走同步空间，
            // 见 [`SyncChannelConfig::space_id]）：损坏回退现场拒绝同步。
            if book.is_none() {
                return Err(book_unavailable_error());
            }
            let channel = build_channel(&config)?;
            // 口令持有串活在轮次作用域，信封模式借出形态对齐（无泄漏）。
            let passphrase_holder = resolve_passphrase(&db_path, book.as_deref(), passphrase)?;
            let mode = match passphrase_holder {
                Some(ref passphrase) => EnvelopeMode::Encrypted { passphrase },
                None => EnvelopeMode::Plaintext,
            };
            let report = run_round_once(conn, &channel, &mode)?;
            // 成功轮次记入本会话形态（打开即同步与低频轮询不再触钥匙串）；
            // 明文库记「明文形态」——同一单点同时承载两态（issue #863）。
            match &passphrase_holder {
                Some(passphrase) => {
                    SessionEnvelope::remember(SessionEnvelope::Encrypted(passphrase.clone()))
                }
                None => SessionEnvelope::remember(SessionEnvelope::Plaintext),
            }
            Ok(Outcome::Evidenced(
                report,
                WriteEvidence::LedgerApplied(report.applied > 0),
            ))
        },
    )
    .await
}

/// 读取挂起操作清单（issue #863 挂起通知数据面）：不可重放 op 的身份与码化
/// 原因，按全序返回。挂起通知可见是 #863 验收项；数量经 `get_sync_status`
/// 的 `parked_count` 回显，明细经本命令按需拉取。
#[tauri::command]
pub async fn get_parked_ops<R: Runtime>(app: AppHandle<R>) -> Result<Vec<ParkedOpState>> {
    let conn = app.state::<DbState>().conn.clone();
    run_db("get_parked_ops", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        Ok(parked_ops(&conn)?
            .into_iter()
            .map(ParkedOpState::from)
            .collect())
    })
    .await
}

/// 挂起操作回显形态（挂起通知与裁决界面的 wire 面）。
#[derive(Debug, Serialize)]
pub struct ParkedOpState {
    /// op 标识（信封不可读时为合成 id）。
    pub op_id: String,
    /// 来源设备标识（信封不可读时为空串）。
    pub device_id: String,
    /// 实体判别键（载荷不可解时为空串）。
    pub entity: String,
    /// 实体 id（不可知时为空串）。
    pub entity_id: String,
    /// 码化挂起原因（前端按码本地化）。
    pub code: String,
    /// 码化挂起原因的插值参数（按消息中动态值出现顺序；ADR-0050）：前端按
    /// `errors.<code>` 模板插值用（issue #957）。
    pub params: Vec<String>,
    /// 挂起原因详情（**已渲染**的中文完整句，码未命中模板或 params 不足时降级透传）。
    pub message: String,
    /// 挂起时刻（本机簿记事实）。
    pub parked_at: String,
}

impl From<crate::sync_engine::ParkedOp> for ParkedOpState {
    fn from(op: crate::sync_engine::ParkedOp) -> Self {
        Self {
            op_id: op.op_id,
            device_id: op.device_id,
            entity: op.entity,
            entity_id: op.entity_id,
            code: op.code,
            params: op.params,
            message: op.message,
            parked_at: op.parked_at,
        }
    }
}

/// 读取通道配置（issue #862 表单回显）：未配置回空表单形态（`configured = false`）。
#[tauri::command]
pub async fn get_sync_channel_config<R: Runtime>(
    app: AppHandle<R>,
) -> Result<SyncChannelConfigState> {
    let conn = app.state::<DbState>().conn.clone();
    run_db("get_sync_channel_config", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        Ok(match configured_channel(&conn)? {
            Some(config) => SyncChannelConfigState {
                base_url: config.base_url,
                username: config.username,
                password: config.password,
                space_id: config.space_id,
                configured: true,
            },
            None => SyncChannelConfigState {
                base_url: String::new(),
                username: String::new(),
                password: String::new(),
                space_id: String::new(),
                configured: false,
            },
        })
    })
    .await
}

/// 保存通道配置（issue #862）：WebDAV 凭据构库校验（`sync-channel.*` 码化错误，
/// 不发网络请求）→ 经 settings 单点落 `app_settings`。置脏豁免路径（设备本机
/// 配置，ADR-0032），零信号——设置页自读回显。
#[tauri::command]
pub async fn set_sync_channel_config<R: Runtime>(
    app: AppHandle<R>,
    config: SyncChannelConfigInput,
) -> Result<()> {
    let conn = app.state::<DbState>().conn.clone();
    run_db("set_sync_channel_config", move || {
        let space_id = config
            .space_id
            .unwrap_or_else(|| DEFAULT_SPACE_ID.to_string());
        let config = SyncChannelConfig {
            base_url: config.base_url,
            username: config.username,
            password: config.password,
            space_id,
        };
        // 校验单点（域侧 `build_channel`）：不通过不落库——错误码为域的
        // sync-channel.*。
        build_channel(&config)?;
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        settings::set(&conn, SettingKey::SyncChannelConfig, &config)
    })
    .await
}

/// 口令解析与验证（ADR-0091 决策 8）：密文库回 `Some(主口令)`——显式参数优先
/// （不经钥匙串、不弹生物认证），其次本机已记住口令（发布构建读取前过生物认证
/// 门，取消/不可用同「无缓存」回退报 `sync-channel.passphrase-required`）；明文
/// 库回 `None`（明文直通，`plaintext_mode` 随轮次报告供界面显著提示）。
/// 取得的口令先验证再封包（[`verify_source_passphrase`]，复用备份域「先验证后
/// 转换」同款读语句与合并口径 `encryption.passphrase-incorrect`）：错误口令封出
/// 的段对端无法解封，且段名幂等跳过会令重传永不发生，必须在上传前拦下。
fn resolve_passphrase(
    db_path: &std::path::Path,
    book: Option<&str>,
    passphrase: Option<String>,
) -> Result<Option<String>> {
    if probe_file_kind(db_path)? != DbFileKind::Encrypted {
        return Ok(None);
    }
    let passphrase = match passphrase {
        Some(passphrase) => passphrase,
        None => match passphrase_cache::load(book)? {
            CacheLoad::Found(passphrase) => passphrase,
            CacheLoad::NotFound | CacheLoad::Cancelled => {
                return Err(AppError::coded(
                    "sync-channel.passphrase-required",
                    "同步通道上有加密数据，需要主口令才能读取",
                ));
            }
        },
    };
    verify_source_passphrase(db_path, &passphrase)?;
    Ok(Some(passphrase))
}

// ---------------------------------------------------------------------------
// 检查点发布 / 预检 / 引导（issue #864 新端加入向导的命令面；ADR-0091 决策 9、
// ADR-0098 决策 5——引导是整库换入的重动作，只经用户显式向导，不挂自动轮次）
// ---------------------------------------------------------------------------

/// 通道上的检查点指针回显（预检形态；不含快照体）。
#[derive(Debug, Serialize)]
pub struct SyncCheckpointInfoState {
    /// 检查点代数（每次发布单调递增）。
    pub generation: i64,
    /// 密文字节数（快照体大小，供向导展示）。
    pub size: u64,
    /// 产出时刻（产出端本地事实，供展示）。
    pub created_at: String,
}

/// 预检通道上的当前检查点（issue #864 引导向导首步）：只读 manifest 不下载
/// 快照体，向导据此区分「通道上还没有检查点（先去旧设备发布）」与「发现检查
/// 点，可引导」。通道未配置报 `sync-channel.not-configured`。
#[tauri::command]
pub async fn get_sync_channel_checkpoint<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Option<SyncCheckpointInfoState>> {
    let conn = app.state::<DbState>().conn.clone();
    run_db("get_sync_channel_checkpoint", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        let config = configured_channel(&conn)?.ok_or_else(not_configured_error)?;
        let channel = build_channel(&config)?;
        Ok(channel
            .checkpoint_pointer()?
            .map(|p| SyncCheckpointInfoState {
                generation: p.generation,
                size: p.size,
                created_at: p.created_at,
            }))
    })
    .await
}

/// 检查点发布结果（向导成功提示的数据面）。
#[derive(Debug, Serialize)]
pub struct SyncCheckpointPublished {
    /// 本次发布的代数。
    pub generation: i64,
    /// 密文字节数。
    pub size: u64,
    /// 明文模式标记（快照明文上通道，界面显著提示依据，ADR-0091 决策 8）。
    pub plaintext_mode: bool,
}

/// 发布检查点到通道（issue #864）：全量快照 + 各流位点成对封包上传，manifest
/// 换指针——存量数据的旧端把「新端可引导的来源」放上通道的唯一动作。
///
/// 本命令是通道操作而非账本写入：本地数据零变化（零信号），且 `VACUUM INTO`
/// 无法在事务内执行，故不经统一写入口 [`crate::write_entry::write_entry`]、
/// 直接持主连接锁调用（位点与快照同刻成对约束，`create_checkpoint`）。
/// 信封模式解析与 `sync_now` 同款（`resolve_passphrase` 单点：密文库凭显式
/// 口令或钥匙串，先验证后封包）。
#[tauri::command]
pub async fn publish_sync_checkpoint<R: Runtime>(
    app: AppHandle<R>,
    passphrase: Option<String>,
) -> Result<SyncCheckpointPublished> {
    let conn = app.state::<DbState>().conn.clone();
    let db_path = active_db_path(&app)?;
    let book = active_book_id(&app);
    run_db("publish_sync_checkpoint", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        let config = configured_channel(&conn)?.ok_or_else(not_configured_error)?;
        if book.is_none() {
            return Err(book_unavailable_error());
        }
        let channel = build_channel(&config)?;
        let passphrase_holder = resolve_passphrase(&db_path, book.as_deref(), passphrase)?;
        let mode = match &passphrase_holder {
            Some(passphrase) => EnvelopeMode::Encrypted { passphrase },
            None => EnvelopeMode::Plaintext,
        };
        let pointer = channel.publish_checkpoint(&conn, &mode)?;
        Ok(SyncCheckpointPublished {
            generation: pointer.generation,
            size: pointer.size,
            plaintext_mode: mode.is_plaintext(),
        })
    })
    .await
}

/// 引导结果（向导收尾提示与重启决策的数据面）。
#[derive(Debug, Serialize)]
pub struct SyncBootstrapOutcome {
    /// 采纳的检查点代数。
    pub generation: i64,
    /// 快照密文字节数。
    pub size: u64,
    /// 引导后本库已从明文转换为本机密文库（重启后需凭主口令解锁）。
    pub reencrypted: bool,
}

/// 新端从通道检查点引导（issue #864）：拉取通道当前检查点并整库换入本机——
/// 「加入即新库」的显式向导动作，成功后由前端原位重引导（`restart_app`）。
///
/// 序列（全部持主连接锁，与轮次同形；快照拉取是整库体量，期间业务写被互斥
/// 排除是正确性要求而非副作用）：
/// 1. 前置守卫（fail fast，不浪费拉取；[`crate::sync_engine::
///    bootstrap_preflight`]，两半同序优先级在域内单点）：通道在位；未参与
///    同步（`sync-engine.bootstrap-not-fresh`）；无用户业务数据（
///    `sync-channel.bootstrap-library-not-empty`——快照整库覆盖，带存量数据
///    被引导等于丢账）。
/// 2. 拉取检查点：信封自描述，密文快照凭口令开封（缺口令报域的
///    `sync-engine.checkpoint-passphrase-required` 可重试，错误口令报
///    `encryption.passphrase-incorrect`）。
/// 3. 信封形态对齐守卫：同一通道的段必须同形态可开（明文端开不了密文段、
///    密文端封的段明文对端开不了）——引导端必须对齐快照形态：
///    - 密文快照 × 密文本机：输入口令必须就是本机主口令（后续轮次以本机口令
///      封段，两把钥匙会让对端解不开），不一致报
///      `sync-channel.bootstrap-passphrase-mismatch`；
///    - 明文快照 × 密文本机：拒绝（`sync-channel.bootstrap-form-mismatch`），
///      请先关闭加密再引导；
///    - 密文快照 × 明文本机：引导后整库转换为本机密文库（复用备份域加密
///      转换，`enable_encryption_for_file` 原子替换 + `.bak` 保留）。
/// 4. 整库换入（域 [`bootstrap_from_checkpoint`]：SQL 级重建、换入本机设备
///    身份、位点以快照为准；快照 schema 偏斜双向处置）。
/// 5. 清「上次成功同步时刻」：本机簿记事实，快照携带的是来源端的值。
///
/// 本命令不经统一写入口（整库替换同 Restore 先例，零信号）：引导后前端立即
/// 原位重引导重载数据，信号无消费窗口。转换失败的可恢复路径：本机已是引导
/// 后的完整数据，经设置页「开启加密」以同一主口令转换即可重新对齐通道形态。
#[tauri::command]
pub async fn bootstrap_sync_from_channel<R: Runtime>(
    app: AppHandle<R>,
    passphrase: Option<String>,
) -> Result<SyncBootstrapOutcome> {
    let conn = app.state::<DbState>().conn.clone();
    let db_path = active_db_path(&app)?;
    run_db("bootstrap_sync_from_channel", move || {
        let mut conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        // 1. 前置守卫（fail fast）：未参与同步且无用户业务数据（「加入即新库」）。
        let config = configured_channel(&conn)?.ok_or_else(not_configured_error)?;
        bootstrap_preflight(&conn)?;
        let channel = build_channel(&config)?;
        // 2. 拉取检查点（持锁；快照体整库下载）。
        let fetched = channel.fetch_checkpoint(passphrase.as_deref())?;
        // 3. 信封形态对齐守卫（替换本机数据前判定，失败零副作用）。
        let local_encrypted = probe_file_kind(&db_path)? == DbFileKind::Encrypted;
        if fetched.sealed && local_encrypted {
            // fetch 成功 ⇒ 口令已验证可开快照（缺口令/错口令在域内归一报错）。
            let passphrase = passphrase.as_deref().unwrap_or_default();
            if verify_source_passphrase(&db_path, passphrase).is_err() {
                return Err(AppError::coded(
                    "sync-channel.bootstrap-passphrase-mismatch",
                    "输入的主口令与本机主口令不一致：密文库引导须以本机主口令进行（通道上的快照以同一主口令封包）",
                ));
            }
        } else if !fetched.sealed && local_encrypted {
            return Err(AppError::coded(
                "sync-channel.bootstrap-form-mismatch",
                "通道上的检查点为明文，本机为密文库：请先在设置中关闭加密，或让来源端开启加密后重新发布检查点",
            ));
        }
        let reencrypted = fetched.sealed && !local_encrypted;
        // 4. 整库换入（域守卫复验同步三表全空）。
        bootstrap_from_checkpoint(&mut conn, &fetched.checkpoint, passphrase.as_deref())?;
        // 5. 本机簿记事实不采纳来源端取值（在文件级转换前执行：转换的原子
        //   替换会让本连接指向被换下的旧文件，此后一切写路径不得再经它——
        //   与既有「开启加密后由前端重启」同一纪律）。
        crate::settings::clear(&conn, crate::settings::SettingKey::SyncLastSyncAt)?;
        // 密文快照 × 明文本机：引导后整库转换为本机密文库（文件级原子替换，
        // 复用备份域机制；重启后新连接凭口令打开）。
        if reencrypted {
            let passphrase = passphrase.as_deref().unwrap_or_default();
            enable_encryption_for_file(&db_path, passphrase)?;
        }
        tracing::info!(
            generation = fetched.generation,
            size = fetched.size,
            reencrypted,
            "已从通道检查点完成新端引导"
        );
        Ok(SyncBootstrapOutcome {
            generation: fetched.generation,
            size: fetched.size,
            reencrypted,
        })
    })
    .await
}
