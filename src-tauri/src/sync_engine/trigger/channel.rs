//! 同步通道配置与句柄（issue #958 拆分自 `trigger.rs`；#863 / ADR-0091 决策 9）：
//! 通道配置的持久化形态与缺省值、配置读取与构库校验单点、轮次复用的通道句柄。
//!
//! 变更原因单一：改配置字段、缺省值或构库校验序列只动本文件。轮次编排与触发
//! 时机见 [`super::scheduler`]，会话密钥形态见 [`super::session`]。

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::settings::{self, SettingKey};
use crate::sync_engine::channel::{
    ChannelLayout, ChannelOptions, CheckpointPointer, FetchedCheckpoint, SyncRoundReport,
    fetch_checkpoint, peek_checkpoint_pointer, publish_checkpoint, run_round_with,
};
use crate::sync_engine::envelope::EnvelopeMode;
use crate::sync_engine::transport::Transport;
use crate::sync_engine::transport::s3::{S3Config, S3Transport};
use crate::sync_engine::transport::webdav::{WebDavConfig, WebDavTransport};

/// 通道空间默认值（同步世界身份的 v1 共识形态，见 [`SyncChannelConfig::space_id`]）。
pub(crate) const DEFAULT_SPACE_ID: &str = "default";

/// 同步通道后端判别（issue #1217 / ADR-0091 决策 1 修订）：通道配置选哪条传输
/// 实现。
///
/// serde 变体名取小写（`webdav` / `s3`，与前端 `ChannelBackend` 联合类型逐字
/// 一致；注意 `snake_case` 会把 `WebDav` 写成 `web_dav`，故用 `lowercase`）；
/// 缺省回 [`ChannelBackend::WebDav`]——判别字段落地晚于既有配置，老配置（没有
/// `backend` 键）据此照常解析（#1217 验收「缺后端判别字段的既有配置仍可解
/// 析」）。WebDAV 变体随 #1221 整体退役。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChannelBackend {
    /// WebDAV 网盘目录（v1 既有后端，退出计划见 #1221）。
    #[default]
    WebDav,
    /// S3 兼容对象存储（#1216 后端）。
    S3,
}

/// 通道配置持久化形态（`app_settings` 的 `sync.channel.config`，JSON 对象）。
///
/// 本机设备配置（不同步，同步边界见多端同步域 SyncBoundary）：凭据不随信封走，
/// 新端各自配置。写入经设置域单点（壳层 `set_sync_channel_config`），读取经
/// [`configured_channel`] 单点。
///
/// 判别字段 `backend` 决定 [`build_channel`] 消费哪组地址/凭据字段：WebDAV 组
/// （`base_url` / `username` / `password`）与 S3 组（`endpoint` / `region` /
/// `bucket` / `prefix` / `access_key` / `secret_key` / `path_style`）并存，
/// 空字段不参与对应后端——WebDAV 组随 #1221 退役。新增字段一律带
/// `#[serde(default)]`：老配置缺键照常解析。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SyncChannelConfig {
    /// 后端判别（缺省回 WebDAV，老配置升级路径）。
    #[serde(default)]
    pub backend: ChannelBackend,
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
    /// 传输后端（测试夹具读通道产物用；生产轮次走 [`SyncChannel::run_round`]）。
    pub fn transport(&self) -> &dyn Transport {
        self.transport.as_ref()
    }

    /// 通道布局（同上）。
    pub fn layout(&self) -> &ChannelLayout {
        &self.layout
    }

    /// 跑一轮通道协议（发布自己流 + 拉取他人流）：把本句柄持有的传输与布局
    /// 一起交给 [`run_round_with`]，调用方不必解包句柄。
    ///
    /// 只做轮次协议本身；「成功时刻落库」是调度侧簿记，留在
    /// [`super::scheduler::run_round_once`]——通道配置不为簿记而改。
    pub fn run_round(
        &self,
        conn: &Connection,
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

    /// 发布检查点（全量快照 + 位点成对封包上通道，manifest 换指针）：
    /// 与轮次同款「句柄交出传输与布局」形态，调用方不必解包句柄。
    ///
    /// 须在单连接互斥锁内调用（位点与快照同刻成对，
    /// [`crate::sync_engine::checkpoint::create_checkpoint`] 约束）。
    pub fn publish_checkpoint(
        &self,
        conn: &Connection,
        mode: &EnvelopeMode<'_>,
    ) -> Result<CheckpointPointer> {
        publish_checkpoint(conn, self.transport.as_ref(), &self.layout, mode)
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
/// `sync-channel.*`）。保存配置与轮次入口共用本单点，校验序列不重复。
pub fn build_channel(config: &SyncChannelConfig) -> Result<SyncChannel> {
    // 按判别字段分派（#1217）：两组字段并存，各后端只消费自己那组；两处构库
    // 都只做参数定型，不发网络请求。
    let transport: Box<dyn Transport> = match config.backend {
        ChannelBackend::WebDav => Box::new(WebDavTransport::new(WebDavConfig {
            base_url: config.base_url.clone(),
            username: config.username.clone(),
            password: config.password.clone(),
        })?),
        ChannelBackend::S3 => Box::new(S3Transport::new(S3Config {
            endpoint: config.endpoint.clone(),
            region: config.region.clone(),
            bucket: config.bucket.clone(),
            access_key: config.access_key.clone(),
            secret_key: config.secret_key.clone(),
            prefix: config.prefix.clone(),
            path_style: config.path_style,
        })?),
    };
    let layout = ChannelLayout::new(&config.space_id)?;
    Ok(SyncChannel { transport, layout })
}
