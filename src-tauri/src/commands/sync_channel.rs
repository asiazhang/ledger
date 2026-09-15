//! 多端同步命令壳（issue #862 / #863 / #864 / #1217 / #1219 / #1221 / ADR-0091）：
//! 同步状态查询、手动同步轮次、通道配置（S3 兼容对象存储凭据）、保存前的
//! 「测试连接」探测与检查点发布/预检/引导（新端加入向导的命令面）。
//!
//! 只做参数解包、凭据/信封模式解析与轮次编排一行调用；通道布局、轮次协议、
//! 幂等重放、挂起语义与触发编排全在 [`crate::sync_engine`]（域不依赖壳，
//! ADR-0056），本文件不含业务语义。唯一例外是保存通道配置时的**用户输入门**
//! （端点必须 https，见 [`ensure_secure_endpoint`]，保存与
//! [`test_sync_channel_connection`] 共用）：它约束人提交的表单形态，不是通道语义
//! ——域侧 [`build_channel`] 对测试直置的明文 http S3 桩配置保持可用，照常跑
//! 自动轮次；该门住命令层是 #1217 的实现决定（理由见函数注释）。
//!
//! - `sync_now` 写路径经分段取锁、整体裁决形态的统一写入口
//!   [`crate::shell_support::write_entry::write_entry_segmented`]（#1276 形态，
//!   ADR-0120 决策 4）：仍是一个写操作身份、一次提交点置脏、一次信号发射；重放
//!   是行为编排之外的第 N 写入入口（ADR-0091，接缝契约与批量导入同待遇），外来
//!   op 实际应用即账本数据变化——经 [`WriteOp::SyncRound`] 条件发参考失效信号
//!   （证据 [`WriteEvidence::LedgerApplied`]），置脏照常在收尾裁决点发生。
//!   连接锁只盖轮次的数据库步骤（读段/重放段/落库段，经 [`SegmentRoundConn`]
//!   每段短取）；manifest 读/写、段上传/下载、封包与解封（含 KDF）是网络段，
//!   在锁外完成——同步在途时本地记账、导入与读命令不再被整轮同步挡住（ADR-0120）。
//!   同端轮次顺序性由域内轮次在途互斥承接：在途时重复触发不启动第二轮，等待
//!   并交出同一轮次报告（回显形态随实施票 #1339 定夺并留痕：取「等待并复用」，
//!   与 ADR-0095 前端「进行中重复触发复用唯一在途同步」口径同款）。失败语义
//!   按 ADR-0120 决策 6 三分：已上传段原子（内容确定等同、重传幂等）、已重放
//!   op 逐条原子、manifest 回写失败由下一轮归并续作自愈；「上次成功同步时刻」
//!   只在整轮成功后的整体裁决点落库，中途失败 / 放弃不更新。
//!   轮次编排本身归域（`sync_engine::trigger`，issue #863）：本壳只解包口令、
//!   解析信封模式并把轮次报告原样交出。
//! - `set_sync_channel_config` 写 `app_settings` 经 [`crate::settings`] 单点收口
//!   （置脏豁免，ADR-0032）：通道凭据是本机设备配置（不同步，同步边界见多端
//!   同步域 SyncBoundary），写操作身份 `SetSyncChannelConfig` 以例外白名单登记
//!   （见 `signals_cross_check`）；校验借域侧 `build_channel` 单点（凭据构库 +
//!   空间闭集，不发网络请求），错误码复用域的 `sync-channel.*`。保存前另有
//!   命令层用户输入门：端点为非 https 时拒绝（`sync-channel.endpoint-insecure`，
//!   本版本不提供 http 或自签证书放行开关，#1217）。
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

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::{AppHandle, Manager, Runtime};

use crate::commands::encryption::{active_book_id, active_db_path};
use crate::shell_support::read_entry::read_entry;
use crate::shell_support::write_entry::{Outcome, SegmentLock, write_entry_segmented};
use ledger_infra::db::encryption::{DbFileKind, probe_file_kind, verify_source_passphrase};
use ledger_infra::db::passphrase_cache::{self, CacheLoad};
use ledger_infra::db::{DbState, probe_lock_hold, run_db};
use ledger_infra::error::{AppError, Result};
use ledger_infra::settings::{self, SettingKey};
use ledger_infra::signals::{WriteEvidence, WriteOp};
use ledger_sync_engine::channel::{ConnSegment, RoundConn};
use ledger_sync_engine::trigger::{
    DEFAULT_SPACE_ID, book_unavailable_error, build_channel, configured_channel,
    not_configured_error, probe_channel, run_round_once,
};
use ledger_sync_engine::{
    EnvelopeMode, SessionEnvelope, SyncChannelConfig, SyncRoundReport, bootstrap_from_channel,
    connection_round_key, create_checkpoint, parked_ops,
};
use ledger_sync_protocol::device::device_id;

/// 通道配置回显（设置页通道配置表单，issue #862；S3 单后端形态 issue #1221）：
/// 未配置时各字段为空串（`path_style` 为假）、`configured = false`，表单按空表单
/// 起填（空间字段由前端填默认值提示）。
#[derive(Debug, Serialize)]
pub struct SyncChannelConfigState {
    /// 同步空间（跨端共识的世界身份）。
    pub space_id: String,
    /// S3 兼容端点。
    pub endpoint: String,
    /// S3 签名区域。
    pub region: String,
    /// S3 桶名。
    pub bucket: String,
    /// S3 对象键前缀。
    pub prefix: String,
    /// S3 Access Key ID。
    pub access_key: String,
    /// S3 Secret Access Key（本机配置回显；响应体不经日志与 trace）。
    pub secret_key: String,
    /// S3 寻址方式（`true` = path-style）。
    pub path_style: bool,
    /// 是否已配置过（区分「空表单」与「保存过空值」的表单初态依据）。
    pub configured: bool,
}

/// 通道配置写入参数（表单提交形态）。
///
/// 新增字段带 serde 缺省；退役后端留下的多余键由 serde 默认忽略，老前端形态
/// 照常反序列化（#1221：WebDAV 组字段与后端判别字段整体退役）。
#[derive(Debug, Default, Deserialize)]
pub struct SyncChannelConfigInput {
    /// 同步空间（缺省回 `default`；两端填同一值即同步同一世界）。
    pub space_id: Option<String>,
    /// S3 兼容端点。
    #[serde(default)]
    pub endpoint: String,
    /// S3 签名区域。
    #[serde(default)]
    pub region: String,
    /// S3 桶名。
    #[serde(default)]
    pub bucket: String,
    /// S3 对象键前缀。
    #[serde(default)]
    pub prefix: String,
    /// S3 Access Key ID。
    #[serde(default)]
    pub access_key: String,
    /// S3 Secret Access Key。
    #[serde(default)]
    pub secret_key: String,
    /// S3 寻址方式（`true` = path-style）。
    #[serde(default)]
    pub path_style: bool,
}

/// 同步状态（设置页同步卡片回显，issue #862）。
#[derive(Debug, Serialize)]
pub struct SyncStatusState {
    /// 本机设备标识（首用生成并持久化，参与全序 tiebreak）。
    pub device_id: String,
    /// 通道是否已配置（通道凭据已保存）。
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

/// 手动同步（issue #862「立即同步」/ #863 轮次编排 / #1339 分段取锁）：执行一次
/// 同步轮次——发布自己流新 op + 拉取他人流增量并经同步引擎幂等重放。返回轮次
/// 报告（上传/应用/挂起计数，前端据此轻量提示）；`plaintext_mode` 为真表示本轮
/// 明文上通道（界面显著提示）。挂起明细经 [`get_parked_ops`] 查询（同步卡片回
/// 显面）。
///
/// 信封模式解析见模块文档；`passphrase` 为密文库下的显式主口令（不落日志，
/// 留空则回退本机已记住口令，均不可得报 `sync-channel.passphrase-required`）。
/// 通道未配置报 `sync-channel.not-configured`；账本注册表不可用报
/// `sync-channel.book-unavailable`。成功后随整体裁决点更新「上次成功同步时刻」，
/// 并把口令记入本机会话（后续自动轮询无需再触钥匙串）。
///
/// 锁跨度（ADR-0120 决策 2/4）：连接锁只盖轮次数据库步骤（经 [`SegmentRoundConn`]
/// 每段短取）；网络段（通道配置构库、口令解析验证、manifest 读写、段上传下载、
/// 封包解封）不消费连接、在分段之间执行——同步在途时本地读写照常。在途时重复
/// 触发不启动第二轮：等待并交出同一轮次报告（ADR-0120 决策 3，回显形态见模块
/// 文档）。
#[tauri::command]
pub async fn sync_now<R: Runtime>(
    app: AppHandle<R>,
    passphrase: Option<String>,
) -> Result<SyncRoundReport> {
    let conn = app.state::<DbState>().conn.clone();
    let db_path = active_db_path(&app)?;
    let book = active_book_id(&app);
    // 轮次身份键（同端同库判据，ADR-0120 决策 3）：写入口持有的同一连接互斥体，
    // 与调度侧自动轮次同键——三个触发入口在在途互斥下串行。
    let round_key = connection_round_key(&conn);
    write_entry_segmented(
        "sync_now",
        conn,
        Some(&app),
        WriteOp::SyncRound,
        move |lock| {
            // 读段（短取锁）：通道在位性前置——未配置即早退，不触网。
            let config = lock
                .with_connection(configured_channel)?
                .ok_or_else(not_configured_error)?;
            // 注册表在位性门禁（同步以活动账本为范围；世界身份走同步空间，
            // 见 [`SyncChannelConfig::space_id]）：损坏回退现场拒绝同步。
            if book.is_none() {
                return Err(book_unavailable_error().into());
            }
            let channel = build_channel(&config)?;
            // 口令解析与验证不消费连接（文件探针 + 钥匙串 + 独立验证连接），
            // 在分段之间（锁外）执行。
            let passphrase_holder = resolve_passphrase(&db_path, book.as_deref(), passphrase)?;
            let mode = match passphrase_holder {
                Some(ref passphrase) => EnvelopeMode::Encrypted { passphrase },
                None => EnvelopeMode::Plaintext,
            };
            // 轮次：数据库段经分段锁短取（读段/重放段/落库段），网络段出锁；
            // 同端顺序性由域内轮次在途互斥承接（ADR-0120 决策 3）。
            let locks = SegmentRoundConn {
                lock,
                key: round_key,
            };
            let report = run_round_once(&locks, &channel, &mode)?;
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

/// 手动同步轮次的连接源（ADR-0120 决策 4：手动入口复用分段写入口形态）：
/// 把分段写入口的 [`SegmentLock`] 包成轮次连接接缝交给编排（`SegmentSession`
/// 先例同款，`sync_instrument_info`）。轮次的每个数据库段经分段锁短暂取一次
/// 连接、用完即还；网络段发生在分段之间（锁外）。轮次身份键取自写入口持有的
/// 同一连接互斥体（构造时定格，与调度侧自动轮次同键）。
struct SegmentRoundConn<'a> {
    lock: &'a SegmentLock<'a>,
    key: u64,
}

impl RoundConn for SegmentRoundConn<'_> {
    fn with_connection<R, F>(&self, _segment: ConnSegment, use_connection: F) -> Result<R>
    where
        F: FnOnce(&Connection) -> Result<R>,
    {
        self.lock.with_connection(use_connection)
    }

    fn round_key(&self) -> u64 {
        self.key
    }
}

/// 读取挂起操作清单（issue #863 挂起通知数据面）：不可重放 op 的身份与码化
/// 原因，按全序返回。挂起通知可见是 #863 验收项；数量经 `get_sync_status`
/// 的 `parked_count` 回显，明细经本命令按需拉取。
#[tauri::command]
pub async fn get_parked_ops<R: Runtime>(app: AppHandle<R>) -> Result<Vec<ParkedOpState>> {
    let conn = app.state::<DbState>().read_conn.clone();
    read_entry("get_parked_ops", conn, move |conn| {
        Ok(parked_ops(conn)?
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

impl From<ledger_sync_engine::ParkedOp> for ParkedOpState {
    fn from(op: ledger_sync_engine::ParkedOp) -> Self {
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
    let conn = app.state::<DbState>().read_conn.clone();
    read_entry("get_sync_channel_config", conn, move |conn| {
        Ok(match configured_channel(conn)? {
            Some(config) => SyncChannelConfigState {
                space_id: config.space_id,
                endpoint: config.endpoint,
                region: config.region,
                bucket: config.bucket,
                prefix: config.prefix,
                access_key: config.access_key,
                secret_key: config.secret_key,
                path_style: config.path_style,
                configured: true,
            },
            None => SyncChannelConfigState {
                space_id: String::new(),
                endpoint: String::new(),
                region: String::new(),
                bucket: String::new(),
                prefix: String::new(),
                access_key: String::new(),
                secret_key: String::new(),
                path_style: false,
                configured: false,
            },
        })
    })
    .await
}

/// 保存通道配置（issue #862 / #1217）：非 https 端点拒绝 → 凭据构库校验
/// （`sync-channel.*` 码化错误，不发网络请求）→ 经 settings 单点落
/// `app_settings`。置脏豁免路径（设备本机配置，ADR-0032），零信号——设置页自读
/// 回显。两步校验都通过才写入：失败路径零写入，不产生半配置状态。
#[tauri::command]
pub async fn set_sync_channel_config<R: Runtime>(
    app: AppHandle<R>,
    config: SyncChannelConfigInput,
) -> Result<()> {
    let conn = app.state::<DbState>().conn.clone();
    run_db("set_sync_channel_config", move || {
        let config = normalize_channel_config(config);
        // 用户输入门（#1217）：端点为非 https 即拒（配置类码化错误）；空值留给
        // 构库单点报既有的 `sync-channel.base-url-missing`。
        ensure_secure_endpoint(&config)?;
        // 校验单点（域侧 `build_channel`）：不通过不落库——错误码为域的
        // sync-channel.*。
        build_channel(&config)?;
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        settings::set(&conn, SettingKey::SyncChannelConfig, &config)
    })
    .await
}

/// 保存前「测试连接」（issue #1219）：用表单里**尚未保存**的配置对通道做一次
/// 对象读取探针，当场把「能不能用、问题出在凭据 / 桶名与端点 / 权限还是网络」
/// 答给用户。零持久化副作用：不写 `app_settings`、不改本机既有的通道配置
/// （失败也不留半配置状态），通道侧只发生一次 GET。
///
/// 探针读固定保留键、缺对象即连通，因此**不要求列桶权限**——最小权限子账号
/// （只授权同步空间目录的 GetObject）判定为连通，与真实轮次读到的权限范围一致。
/// 分层错误（`sync-channel.auth-failed` / `target-missing` / `permission-denied` /
/// `network-failed` / `http-failed`）由域侧传输层单点产出，本壳只解包参数。
///
/// 与保存共用 [`ensure_secure_endpoint`] 用户输入门：探测会把凭据发到端点，明文
/// http 与保存同规拒绝，避免「测试通过但存不进去」的双口径，也不让密钥走明文。
///
/// 走 [`run_db`] 的阻塞线程池：探针是同步阻塞 IO（域侧 S3 后端的桥接形态），
/// 不能在事件循环线程上跑。
#[tauri::command]
pub async fn test_sync_channel_connection(config: SyncChannelConfigInput) -> Result<()> {
    run_db("test_sync_channel_connection", move || {
        let config = normalize_channel_config(config);
        ensure_secure_endpoint(&config)?;
        probe_channel(&config)
    })
    .await
}

/// 表单入参 → 持久化形态的规格化单点（保存与「测试连接」共用）：`space_id`
/// 缺省回默认同步空间，其余字段原样搬运。
///
/// 两个命令共用同一次转换，是因为它们必须对同一份表单给出同一个结论——各写一份
/// 转换会让「测试连接通过」与「保存后跑不起来」在字段缺省上分叉。
fn normalize_channel_config(input: SyncChannelConfigInput) -> SyncChannelConfig {
    SyncChannelConfig {
        space_id: input
            .space_id
            .unwrap_or_else(|| DEFAULT_SPACE_ID.to_string()),
        endpoint: input.endpoint,
        region: input.region,
        bucket: input.bucket,
        prefix: input.prefix,
        access_key: input.access_key,
        secret_key: input.secret_key,
        path_style: input.path_style,
    }
}

/// 保存前的地址形态门（#1217）：通道端点只接受 `https://`（MVP 不提供 http 或
/// 自签证书放行开关）。空端点留给 [`build_channel`] 报既有码
/// `sync-channel.base-url-missing`，本门不改变该口径。
///
/// **为什么住命令层**：这是用户输入门而非通道语义——它只拦「人填的表单」，不
/// 改变已落库配置的可用性（测试直置的明文 http S3 桩配置照常经 [`build_channel`]
/// 跑轮次）。域侧构库单点保持「连接参数定型 + 布局构造」的单一职责，不掺入
/// 面向表单的策略；取舍（含为何测试不经保存命令注入配置）记在 PR 正文。
fn ensure_secure_endpoint(config: &SyncChannelConfig) -> Result<()> {
    let endpoint = config.endpoint.trim();
    if endpoint.is_empty() || endpoint.to_ascii_lowercase().starts_with("https://") {
        return Ok(());
    }
    Err(AppError::coded(
        "sync-channel.endpoint-insecure",
        "同步通道端点必须使用 https 地址",
    ))
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
///
/// 锁跨度（issue #1283 / ADR-0069 决策 4）：锁只为读取通道配置（`app_settings`
/// 单行读，走读入口短锁）；manifest GET 是网络等待、不消费连接，在锁外经阻塞
/// 线程池完成——网络慢或超时时不阻塞其它命令。返回形态与错误码不变。
#[tauri::command]
pub async fn get_sync_channel_checkpoint<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Option<SyncCheckpointInfoState>> {
    let read_conn = app.state::<DbState>().read_conn.clone();
    let config = read_entry("get_sync_channel_checkpoint", read_conn, move |conn| {
        configured_channel(conn)?.ok_or_else(not_configured_error)
    })
    .await?;
    run_db("get_sync_channel_checkpoint", move || {
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
/// 无法在事务内执行，故不经统一写入口 [`crate::shell_support::write_entry::write_entry`]。
/// 锁跨度（issue #1284，判据同 ADR-0120 / ADR-0069 决策 4）：通道配置读取走
/// 读入口短锁；主连接锁只盖快照产出（位点与快照同刻成对约束，
/// `create_checkpoint`）；口令解析、封包（KDF）与三次通道往返不消费连接，
/// 出锁执行——发布期间网络慢或超时不再阻塞全应用读写。信封模式解析与
/// `sync_now` 同款（`resolve_passphrase` 单点：密文库凭显式口令或钥匙串，
/// 先验证后封包）。
#[tauri::command]
pub async fn publish_sync_checkpoint<R: Runtime>(
    app: AppHandle<R>,
    passphrase: Option<String>,
) -> Result<SyncCheckpointPublished> {
    let read_conn = app.state::<DbState>().read_conn.clone();
    let conn = app.state::<DbState>().conn.clone();
    let db_path = active_db_path(&app)?;
    let book = active_book_id(&app);

    // 通道在位性前置（读入口短锁）：未配置即早退，不触网。
    let config = read_entry("publish_sync_checkpoint", read_conn, |conn| {
        configured_channel(conn)?.ok_or_else(not_configured_error)
    })
    .await?;

    run_db("publish_sync_checkpoint", move || {
        // 账本门与构库单点：不发网络请求（错误码为域的 sync-channel.*）。
        if book.is_none() {
            return Err(book_unavailable_error());
        }
        let channel = build_channel(&config)?;
        // 口令持有串活在轮次作用域，信封模式借出形态对齐（无泄漏）。
        let passphrase_holder = resolve_passphrase(&db_path, book.as_deref(), passphrase)?;
        let mode = match &passphrase_holder {
            Some(passphrase) => EnvelopeMode::Encrypted { passphrase },
            None => EnvelopeMode::Plaintext,
        };
        // 锁内段：快照产出——位点与快照同刻成对是唯一需要连接互斥的步骤，
        // 主连接锁只盖这一步，随语句块立即释放。
        let checkpoint = {
            let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
            create_checkpoint(&conn)?
        };
        // 锁外段：封包（KDF）与通道网络往返（检查点上传、manifest 换指针）。
        let pointer = channel.upload_checkpoint(&checkpoint, &mode)?;
        Ok(SyncCheckpointPublished {
            generation: pointer.generation,
            size: pointer.size,
            plaintext_mode: mode.is_plaintext(),
        })
    })
    .await
}

/// 引导结果（域 [`ledger_sync_engine::BootstrapOutcome`] 的 wire 投影）。
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
/// 「加入即新库」的显式向导动作（ADR-0098 决策 5：不挂自动轮次），成功后由
/// 前端原位重引导（`restart_app`）。
///
/// 编排全在域单点 [`ledger_sync_engine::bootstrap_from_channel`]（前置守卫、
/// 拉取、复验、信封形态对齐、整库换入、簿记清理与转密文决策）；本命令不经
/// 统一写入口（整库替换同 Restore 先例，零信号：引导后前端立即原位重引导，
/// 信号无消费窗口）。
///
/// 锁跨度（issue #1285 / ADR-0120 判据同簇适用）：配置读取走读入口短锁
///（同 #1283 预检先例）；主连接经段接缝按段短取——段1 前置守卫、段2 复验 +
/// 整库换入（对并发本地写互斥，防静默覆盖丢账），中间的整库快照下载是纯
/// 网络等待、在锁外完成（ADR-0069 决策 4）。锁失败映射与持锁时长探针内化
/// 在 [`MainConnSegments`]。
#[tauri::command]
pub async fn bootstrap_sync_from_channel<R: Runtime>(
    app: AppHandle<R>,
    passphrase: Option<String>,
) -> Result<SyncBootstrapOutcome> {
    let read_conn = app.state::<DbState>().read_conn.clone();
    // 通道在位性前置（读入口短锁）：未配置即早退，不触网。
    let config = read_entry("bootstrap_sync_from_channel", read_conn, move |conn| {
        configured_channel(conn)?.ok_or_else(not_configured_error)
    })
    .await?;
    let conn = app.state::<DbState>().conn.clone();
    let db_path = active_db_path(&app)?;
    run_db("bootstrap_sync_from_channel", move || {
        let channel = build_channel(&config)?;
        // 编排归域（`bootstrap_from_channel`，ADR-0056）：前置守卫、拉取、
        // 复验、信封形态对齐、整库换入、簿记清理与转密文决策全在域内单点，
        // 本壳只实现段接缝（[`MainConnSegments`]，短取锁）并把结果投影为
        // wire 形态。
        let segments = MainConnSegments { conn };
        let outcome = bootstrap_from_channel(&segments, &db_path, &channel, passphrase.as_deref())?;
        Ok(SyncBootstrapOutcome {
            generation: outcome.generation,
            size: outcome.size,
            reencrypted: outcome.reencrypted,
        })
    })
    .await
}

/// 引导命令的主连接段接缝实现（issue #1285，域接缝
/// [`ledger_sync_engine::BootstrapConnSegments`] 的壳侧唯一实现）：每段短取
/// 一次主连接——锁失败映射与持锁时长探针（#1276 口径）内化此处；返回即
/// 释放，整库快照下载发生在段与段之间，结构上不占锁。
///
/// 与分段写入口的 [`crate::shell_support::write_entry::SegmentLock`] 同形状的
/// 近亲，不合并的原因：引导不经写入口（零信号，Restore 先例），且换入段需
/// `&mut Connection`（SegmentLock 只递 `&Connection`）——就近住命令文件，
/// 经读侧豁免清单看守（signals_cross_check）。
struct MainConnSegments {
    conn: Arc<Mutex<Connection>>,
}

impl ledger_sync_engine::BootstrapConnSegments for MainConnSegments {
    fn with_conn<T>(&self, use_conn: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T> {
        let hold_started = Instant::now();
        let mut conn = self.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        let result = use_conn(&mut conn);
        probe_lock_hold(hold_started.elapsed());
        result
    }
}
