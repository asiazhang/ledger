//! 多端同步命令壳（issue #862 / ADR-0091）：同步状态查询、手动同步轮次与
//! 通道配置（WebDAV 凭据）。
//!
//! 只做参数解包、凭据/信封模式解析与轮次编排一行调用；通道布局、轮次协议、
//! 幂等重放与挂起语义全在 [`crate::sync_engine`]（域不依赖壳，ADR-0056），
//! 本文件不含业务语义。
//!
//! - `sync_now` 写路径经统一写入口 [`crate::write_entry::write_entry`]（ADR-0073）：
//!   重放是行为编排之外的第 N 写入入口（ADR-0091，接缝契约与批量导入同待遇），
//!   外来 op 实际应用即账本数据变化——经 [`WriteOp::SyncRound`] 条件发参考失效
//!   信号（证据 [`WriteEvidence::LedgerApplied`]），置脏照常在提交点发生。
//!   轮次期间持主连接锁（与 `sync_instrument_info` 网络同步同形）：通道轮次的
//!   发布/拉取/重放本就要求单连接互斥（checkpoint 成对约束同源），失败上抛
//!   不产生部分本地状态（已上传段内容确定等同，重试轮次幂等续作）。
//! - `set_sync_channel_config` 写 `app_settings` 经 [`crate::settings`] 单点收口
//!   （置脏豁免，ADR-0032）：WebDAV 凭据是本机设备配置（不同步，同步边界见
//!   多端同步域 SyncBoundary），写操作身份 `SetSyncChannelConfig` 以例外白名单
//!   登记（见 `signals_cross_check`）；校验借 [`WebDavTransport::new`] 构库与
//!   [`ChannelLayout::new`] 空间闭集单点（不发网络请求），错误码复用域的
//!   `sync-channel.*`。同步世界身份（通道目录 `book-<space>`）由同步空间字段
//!   显式约定——账本登记标识按 ADR-0089 是本机事实，跨设备不成立；两端配置
//!   同一空间即对齐同一世界（v1 默认 `default`，多账本各自独立同步 = 各配
//!   一个空间）。
//! - 信封模式按本库加密形态决定（复用备份域加密模式，ADR-0091 决策 8）：密文
//!   库凭主口令封包——显式参数优先，其次本机已记住口令（钥匙串），均不可得报
//!   `sync-channel.passphrase-required`；明文库明文直通，轮次报告携带
//!   `plaintext_mode` 供界面显著提示。主口令/凭据不落日志与 trace（ADR-0075，
//!   lib.rs 载荷脱敏单点遮蔽 `passphrase` 与 `password` 字段）。

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime};

use crate::commands::encryption::{active_book_id, active_db_path};
use crate::db::encryption::{DbFileKind, probe_file_kind, verify_source_passphrase};
use crate::db::passphrase_cache::{self, CacheLoad};
use crate::db::{DbState, now_iso, run_db};
use crate::error::{AppError, Result};
use crate::settings::{self, SettingKey};
use crate::signals::{WriteEvidence, WriteOp};
use crate::sync_engine::{
    ChannelLayout, EnvelopeMode, SyncRoundReport, WebDavConfig, WebDavTransport, device_id,
    parked_ops, run_round,
};
use crate::write_entry::{Outcome, write_entry};

/// 通道空间默认值（同步世界身份的 v1 共识形态，见 [`SyncChannelConfig::space_id`]）。
pub(crate) const DEFAULT_SPACE_ID: &str = "default";

/// 通道配置持久化形态（`app_settings` 的 `sync.channel.config`，JSON 对象）。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SyncChannelConfig {
    /// 同步根目录 URL。
    pub base_url: String,
    /// WebDAV 账号。
    pub username: String,
    /// WebDAV 密码 / 应用密码。
    pub password: String,
    /// 同步空间（通道世界身份，`book-<space>` 目录）：**跨端共识值**——两端
    /// 配置同一同步空间即对齐同一世界。账本登记的标识按 ADR-0089 是本机
    /// 事实（目录哈希 / 本机生成 UUID），跨设备不成立，故世界身份由用户在
    /// 通道配置里显式约定（v1 默认 `default`）；多账本各自独立同步 = 各配
    /// 一个空间。反序化缺省回默认（旧配置升级）。
    #[serde(default = "default_space_id")]
    pub space_id: String,
}

/// [`SyncChannelConfig::space_id`] 的 serde 缺省值单点。
fn default_space_id() -> String {
    DEFAULT_SPACE_ID.to_string()
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

/// 读取同步状态（issue #862）：设备标识、通道配置在位、上次成功同步时刻、
/// 挂起数量与库加密形态。读路径零副作用；缺 key / 缺表回默认（读路径不因
/// 缺 key 上抛，与设置域读命令同口径）。
#[tauri::command]
pub async fn get_sync_status<R: Runtime>(app: AppHandle<R>) -> Result<SyncStatusState> {
    let conn = app.state::<DbState>().conn.clone();
    let db_path = active_db_path(&app)?;
    run_db("get_sync_status", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        let configured: Option<SyncChannelConfig> =
            settings::get(&conn, SettingKey::SyncChannelConfig, None)?;
        Ok(SyncStatusState {
            device_id: device_id(&conn)?,
            channel_configured: configured.is_some(),
            last_sync_at: settings::get(&conn, SettingKey::SyncLastSyncAt, None)?,
            parked_count: parked_ops(&conn)?.len(),
            library_encrypted: probe_file_kind(&db_path)? == DbFileKind::Encrypted,
        })
    })
    .await
}

/// 手动同步（issue #862「立即同步」）：执行一次同步轮次——发布自己流新 op +
/// 拉取他人流增量并经同步引擎幂等重放。返回轮次报告（上传/应用/挂起计数，
/// 前端据此轻量提示）；`plaintext_mode` 为真表示本轮明文上通道（界面显著提示）。
///
/// 信封模式解析见模块文档；`passphrase` 为密文库下的显式主口令（不落日志，
/// 留空则回退本机已记住口令，均不可得报 `sync-channel.passphrase-required`）。
/// 通道未配置报 `sync-channel.not-configured`；账本注册表不可用报
/// `sync-channel.book-unavailable`。成功后随轮次事务更新「上次成功同步时刻」。
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
            let config: SyncChannelConfig =
                settings::get(conn, SettingKey::SyncChannelConfig, None)?.ok_or_else(|| {
                    AppError::coded(
                        "sync-channel.not-configured",
                        "同步通道尚未配置，请先在设置中填写网盘信息",
                    )
                })?;
            // 注册表在位性门禁（同步以活动账本为范围；世界身份走同步空间，
            // 见 [`SyncChannelConfig::space_id]）：损坏回退现场拒绝同步。
            if book.is_none() {
                return Err(AppError::coded(
                    "sync-channel.book-unavailable",
                    "账本注册表不可用，无法确定同步范围",
                ));
            }
            let transport = WebDavTransport::new(WebDavConfig {
                base_url: config.base_url,
                username: config.username,
                password: config.password,
            })?;
            let layout = ChannelLayout::new(&config.space_id)?;
            // 口令持有串活在轮次作用域，信封模式借出形态对齐（无泄漏）。
            let passphrase_holder = resolve_passphrase(&db_path, book.as_deref(), passphrase)?;
            let mode = match passphrase_holder {
                Some(ref passphrase) => EnvelopeMode::Encrypted { passphrase },
                None => EnvelopeMode::Plaintext,
            };
            let report = run_round(conn, &transport, &layout, &mode)?;
            // 上次成功同步时刻随轮次事务落库（设备本地运行时状态，失败不更新）。
            settings::set(conn, SettingKey::SyncLastSyncAt, &now_iso())?;
            Ok(Outcome::Evidenced(
                report,
                WriteEvidence::LedgerApplied(report.applied > 0),
            ))
        },
    )
    .await
}

/// 读取通道配置（issue #862 表单回显）：未配置回空表单形态（`configured = false`）。
#[tauri::command]
pub async fn get_sync_channel_config<R: Runtime>(
    app: AppHandle<R>,
) -> Result<SyncChannelConfigState> {
    let conn = app.state::<DbState>().conn.clone();
    run_db("get_sync_channel_config", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        let config: Option<SyncChannelConfig> =
            settings::get(&conn, SettingKey::SyncChannelConfig, None)?;
        Ok(match config {
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
        // 校验单点：构库（连接参数定型，不发网络请求）+ 布局构造（空间字符
        // 闭集），不通过不落库——错误码为域的 sync-channel.*。
        let config = SyncChannelConfig {
            base_url: config.base_url,
            username: config.username,
            password: config.password,
            space_id,
        };
        WebDavTransport::new(WebDavConfig {
            base_url: config.base_url.clone(),
            username: config.username.clone(),
            password: config.password.clone(),
        })?;
        ChannelLayout::new(&config.space_id)?;
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
