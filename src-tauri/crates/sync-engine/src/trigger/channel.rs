//! 同步通道配置与句柄（issue #958 拆分自 `trigger.rs`；#863 / ADR-0091 决策 9）：
//! 通道配置的持久化形态与缺省值、配置读取与构库校验单点、轮次复用的通道句柄。
//!
//! 变更原因单一：改配置字段、缺省值或构库校验序列只动本文件。轮次编排与触发
//! 时机见 [`super::scheduler`]，会话密钥形态见 [`super::session`]。

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::channel::{
    ChannelLayout, ChannelOptions, CheckpointPointer, FetchedCheckpoint, RoundConn,
    SyncRoundReport, fetch_checkpoint, peek_checkpoint_pointer, run_round_with, upload_checkpoint,
};
use crate::checkpoint::Checkpoint;
use crate::envelope::EnvelopeMode;
use crate::transport::Transport;
use crate::transport::s3::{S3Config, S3Transport};
use ledger_infra::error::Result;
use ledger_infra::settings::{self, SettingKey};

/// 通道空间默认值（同步世界身份的 v1 共识形态，见 [`SyncChannelConfig::space_id`]）。
pub const DEFAULT_SPACE_ID: &str = "default";

/// 通道配置持久化形态（`app_settings` 的 `sync.channel.config`，JSON 对象）。
///
/// 本机设备配置（不同步，同步边界见多端同步域 SyncBoundary）：凭据不随信封走，
/// 新端各自配置。写入经设置域单点（壳层 `set_sync_channel_config`），读取经
/// [`configured_channel`] 单点。
///
/// v1 只有一条后端路径（S3 兼容对象存储，[`build_channel`] 单点构造），配置面
/// 因此没有后端判别字段——WebDAV 后端、它的字段与判别字段随 #1221 整体退役
/// （ADR-0091 决策 1 修订）。新增字段一律带 `#[serde(default)]`：老配置缺键
/// 照常解析；退役后端留下的旧键被 serde 默认忽略，不阻断升级解析。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SyncChannelConfig {
    /// 同步空间（通道世界身份，`book-<space>` 目录）：**跨端共识值**——两端
    /// 配置同一同步空间即对齐同一世界。账本登记的标识按 ADR-0089 是本机
    /// 事实（目录哈希 / 本机生成 UUID），跨设备不成立，故世界身份由用户在
    /// 通道配置里显式约定（v1 默认 `default`）；多账本各自独立同步 = 各配
    /// 一个空间。反序列化缺省回默认（旧配置升级）。
    #[serde(default = "default_space_id")]
    pub space_id: String,
    /// S3 兼容端点（如 `https://s3.example.com`；MVP 只接受 https，非 https
    /// 保存被壳层单点拒绝）。
    #[serde(default)]
    pub endpoint: String,
    /// S3 签名区域（如 `cn-hangzhou`）。
    #[serde(default)]
    pub region: String,
    /// S3 桶名。
    #[serde(default)]
    pub bucket: String,
    /// S3 对象键前缀（空串 = 桶根；多账本各配一个前缀即落在同一桶的不同世界）。
    #[serde(default)]
    pub prefix: String,
    /// S3 Access Key ID（标识，不是秘密；秘密字段见 [`Self::secret_key`]）。
    #[serde(default)]
    pub access_key: String,
    /// S3 Secret Access Key（与主口令同级处置：进 IPC 脱敏白名单，不落日志与
    /// 追踪，见 `shell_support::redact`）。
    #[serde(default)]
    pub secret_key: String,
    /// 寻址方式：`true` = path-style（`/{bucket}/{key}`，自建兼容端点用），
    /// `false` = virtual-host（默认；国内主流厂商多数只支持或推荐它）。
    #[serde(default)]
    pub path_style: bool,
}

/// [`SyncChannelConfig::space_id`] 的 serde 缺省值单点。
fn default_space_id() -> String {
    DEFAULT_SPACE_ID.to_string()
}

/// 通道句柄（传输 + 布局）：由配置构库一次、轮次复用。
///
/// 轮次协议内聚为 [`SyncChannel::run_round`]——调用方交连接与信封模式即可跑一轮，
/// 不必解包传输与布局（issue #958 对 `transport()`/`layout()` 中间人的处置）。
/// 两个访问器保留给测试夹具直接读写通道产物（e2e 读 manifest、投递坏段），
/// 生产轮次不经它们解包。
pub struct SyncChannel {
    transport: Box<dyn Transport>,
    layout: ChannelLayout,
}

impl std::fmt::Debug for SyncChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyncChannel")
            .field("layout", &self.layout)
            .finish_non_exhaustive()
    }
}

impl SyncChannel {
    /// 测试构造（域单测合成通道：内存假 Transport 直装；生产构库走
    /// [`build_channel`] 单点，不发网络请求）。
    #[cfg(test)]
    pub(crate) fn from_parts(transport: Box<dyn Transport>, layout: ChannelLayout) -> Self {
        Self { transport, layout }
    }

    /// 传输后端（测试夹具读通道产物用；生产轮次走 [`SyncChannel::run_round`]）。
    pub fn transport(&self) -> &dyn Transport {
        self.transport.as_ref()
    }

    /// 通道布局（同上）。
    pub fn layout(&self) -> &ChannelLayout {
        &self.layout
    }

    /// 测试接缝（`sha256_hex` 同款 `#[doc(hidden)]` 放宽，ADR-0084 决策 2）：
    /// 以给定的传输与布局直接构造句柄——触发编排域单测以内存假 Transport 驱动
    /// `run_round_once` / `run_auto_round` 全链，不经真实 S3。生产路径不经本
    /// 构造器（生产一律经 [`build_channel`]）。
    #[doc(hidden)]
    pub fn from_parts(transport: Box<dyn Transport>, layout: ChannelLayout) -> Self {
        Self { transport, layout }
    }

    /// 跑一轮通道协议（发布自己流 + 拉取他人流）：把本句柄持有的传输与布局
    /// 一起交给 [`run_round_with`]，调用方不必解包句柄。
    ///
    /// 分段取锁（ADR-0120 决策 2）：连接经 [`RoundConn`] 接缝按数据库段短取，
    /// 网络段不消费连接；只做轮次协议本身，「成功时刻落库」是调度侧簿记，留在
    /// [`super::scheduler::run_round_once`]——通道配置不为簿记而改。
    pub fn run_round<S: RoundConn>(
        &self,
        conn: &S,
        mode: &EnvelopeMode<'_>,
        options: &ChannelOptions,
    ) -> Result<SyncRoundReport> {
        run_round_with(conn, self.transport.as_ref(), &self.layout, mode, options)
    }

    /// 读取通道上的当前检查点指针（不下载快照体；新端引导前的预检接缝，
    /// 壳层向导据此区分「通道上还没有检查点」与「发现检查点可引导」，#864）。
    pub fn checkpoint_pointer(&self) -> Result<Option<CheckpointPointer>> {
        peek_checkpoint_pointer(self.transport.as_ref(), &self.layout)
    }

    /// 连通性探测（issue #1219 保存前「测试连接」）：对固定探针对象做一次
    /// **读取**，缺对象（`Ok(None)`）视为连通——探针读的是同步轮次不写、
    /// 也永远不要求其存在的保留键，因此「桶刚建、还是空的」与「桶名写错」
    /// 被分成两条不同结论（前者连通、后者 [`crate::transport`]
    /// 的 `sync-channel.target-missing`）。
    ///
    /// 只读是该探测的核心约束：最小权限子账号通常没有 `ListBucket` 权限，
    /// 用列桶/列对象当探针会把「能同步」的账号判成不通。同理也不做写入探测——
    /// 「保存前试写」会给用户的桶留垃圾对象，且与读取探针要回答的问题
    /// （凭据 / 目标 / 权限 / 网络是否就位）无关。
    pub fn probe_connection(&self) -> Result<()> {
        self.transport
            .read_file(&self.layout.probe_path())
            .map(|_| ())
    }

    /// 发布检查点——发布段（快照字节 + 位点成对封包上通道，manifest 换指针）：
    /// 与轮次同款「句柄交出传输与布局」形态，调用方不必解包句柄。
    ///
    /// 消费的是已定格的快照字节（产出段 [`crate::checkpoint::create_checkpoint`]，
    /// 须在单连接互斥锁内调用完成位点与快照同刻成对）；本段封包（KDF）与网络
    /// 往返不消费连接——在连接锁外调用（#1284，判据同 ADR-0120）。
    pub fn upload_checkpoint(
        &self,
        checkpoint: &Checkpoint,
        mode: &EnvelopeMode<'_>,
    ) -> Result<CheckpointPointer> {
        upload_checkpoint(self.transport.as_ref(), &self.layout, mode, checkpoint)
    }

    /// 拉取通道上的当前检查点（新端引导取件；解封凭主口令，明文模式免口令）。
    /// 返回值携带封包形态标记（[`FetchedCheckpoint::sealed`]），引导端据此
    /// 对齐本库加密形态（#864）。
    pub fn fetch_checkpoint(&self, passphrase: Option<&str>) -> Result<FetchedCheckpoint> {
        fetch_checkpoint(self.transport.as_ref(), &self.layout, passphrase)
    }
}

/// 读取已配置的通道（未配置回 `None`；缺 key / 缺表按设置域读口径回默认）。
pub fn configured_channel(conn: &Connection) -> Result<Option<SyncChannelConfig>> {
    settings::get(conn, SettingKey::SyncChannelConfig, None)
}

/// 从配置构库（连接参数定型 + 布局构造，不发网络请求；错误码复用域的
/// `sync-channel.*`）。保存配置、手动同步与「测试连接」三处共用本单点，
/// 校验序列不重复。
pub fn build_channel(config: &SyncChannelConfig) -> Result<SyncChannel> {
    // v1 唯一后端是 S3 兼容对象存储（#1221 收口）：构造只做参数定型，不发网络请求。
    let transport: Box<dyn Transport> = Box::new(S3Transport::new(S3Config {
        endpoint: config.endpoint.clone(),
        region: config.region.clone(),
        bucket: config.bucket.clone(),
        access_key: config.access_key.clone(),
        secret_key: config.secret_key.clone(),
        prefix: config.prefix.clone(),
        path_style: config.path_style,
    })?);
    let layout = ChannelLayout::new(&config.space_id)?;
    Ok(SyncChannel { transport, layout })
}

/// 用一份（通常还没落库的）通道配置探测连通性（issue #1219 保存前
/// 「测试连接」）：经 [`build_channel`] 单点做构库校验，再跑读取探针。
///
/// 与保存路径共用同一构库单点，是为了让「测试连接通过」与「保存后能同步」
/// 给出同一个答案——探针读的正是轮次要读的通道与权限范围，不另起一套校验。
/// 本函数只做只读探测：不改本机配置、不写通道。
pub fn probe_channel(config: &SyncChannelConfig) -> Result<()> {
    build_channel(config)?.probe_connection()
}
