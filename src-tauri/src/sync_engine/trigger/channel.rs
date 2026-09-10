//! 同步通道配置与句柄（issue #958 拆分自 `trigger.rs`；#863 / ADR-0091 决策 9）：
//! 通道配置的持久化形态与缺省值、配置读取与构库校验单点、轮次复用的通道句柄。
//!
//! 变更原因单一：改配置字段、缺省值或构库校验序列只动本文件。轮次编排与触发
//! 时机见 [`super::scheduler`]，会话密钥形态见 [`super::session`]。

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::settings::{self, SettingKey};
use crate::sync_engine::channel::{ChannelLayout, ChannelOptions, SyncRoundReport, run_round_with};
use crate::sync_engine::envelope::EnvelopeMode;
use crate::sync_engine::transport::webdav::{WebDavConfig, WebDavTransport};

/// 通道空间默认值（同步世界身份的 v1 共识形态，见 [`SyncChannelConfig::space_id`]）。
pub(crate) const DEFAULT_SPACE_ID: &str = "default";

/// 通道配置持久化形态（`app_settings` 的 `sync.channel.config`，JSON 对象）。
///
/// 本机设备配置（不同步，同步边界见多端同步域 SyncBoundary）：凭据不随信封走，
/// 新端各自配置。写入经设置域单点（壳层 `set_sync_channel_config`），读取经
/// [`configured_channel`] 单点。
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
    /// 一个空间。反序列化缺省回默认（旧配置升级）。
    #[serde(default = "default_space_id")]
    pub space_id: String,
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
#[derive(Debug)]
pub struct SyncChannel {
    transport: WebDavTransport,
    layout: ChannelLayout,
}

impl SyncChannel {
    /// 传输后端（测试夹具读通道产物用；生产轮次走 [`SyncChannel::run_round`]）。
    pub fn transport(&self) -> &WebDavTransport {
        &self.transport
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
        run_round_with(conn, &self.transport, &self.layout, mode, options)
    }
}

/// 读取已配置的通道（未配置回 `None`；缺 key / 缺表按设置域读口径回默认）。
pub fn configured_channel(conn: &Connection) -> Result<Option<SyncChannelConfig>> {
    settings::get(conn, SettingKey::SyncChannelConfig, None)
}

/// 从配置构库（连接参数定型 + 布局构造，不发网络请求；错误码复用域的
/// `sync-channel.*`）。保存配置与轮次入口共用本单点，校验序列不重复。
pub fn build_channel(config: &SyncChannelConfig) -> Result<SyncChannel> {
    let transport = WebDavTransport::new(WebDavConfig {
        base_url: config.base_url.clone(),
        username: config.username.clone(),
        password: config.password.clone(),
    })?;
    let layout = ChannelLayout::new(&config.space_id)?;
    Ok(SyncChannel { transport, layout })
}
