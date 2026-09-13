//! 同步触发编排（issue #863 / #958 / ADR-0091 决策 9、ADR-0098）：把「什么时候
//! 同步」的知识从壳层收进本域——打开应用即同步、桌面运行期低频轮询、手动「立即
//! 同步」三条入口共用同一段轮次编排与同一份通道配置读取。
//!
//! 职责地图（issue #958：每个文件只有一个变更原因，按「改一处只动一个文件」拆）：
//! - [`channel`]（改「凭据与空间怎么配、句柄怎么跑轮次」只动这里）：通道配置持久化
//!   形态与缺省值、配置读取与构库校验单点、轮次复用的通道句柄；
//! - [`session`]（改「口令从哪来」只动这里）：本机会话密钥形态单例——解锁密文库
//!   或一次成功的手动同步后记入，同步轮次据此判定信封模式，且**不读钥匙串**
//!   （钥匙串读取在发布构建下带生物认证门，后台轮询不得弹交互，ADR-0098）；
//! - [`scheduler`]（改「什么时候同步、手动入口报什么错」只动这里）：轮次编排与
//!   成功时刻落库、三条触发入口（打开即同步 / 桌面低频轮询 / 写 op 后去抖合流）
//!   与手动入口的码化错误构造。
//!
//! 对外接缝：本模块的 `pub use` 保持 `sync_engine` 再导出面零变化（壳层与测试
//! 调用点不动）；域内行为断言见 `trigger/tests.rs`（ADR-0087）。
//!
//! 触发时机是**可逆工程决策**（ADR-0091「后果」段明言不入 ADR），轮询周期与
//! 「自动轮询不做新端引导」两条留痕见 ADR-0098。

mod channel;
mod scheduler;
mod session;

pub(crate) use channel::DEFAULT_SPACE_ID;
pub use channel::{
    ChannelBackend, SyncChannel, SyncChannelConfig, build_channel, configured_channel,
};
pub use scheduler::{
    TriggerTimings, book_unavailable_error, install_after_write_hook, not_configured_error,
    run_auto_round, run_round_once, start_sync_scheduler, start_sync_scheduler_with,
    start_triggers, sync_after_write, sync_on_start,
};
// 测试接缝：信号侧两个 helper 只被 `trigger/tests.rs` 经 `trigger::` 路径消费，
// 故再导出仅测试构建存在（生产构建不引即触发 unused-imports）；可见性取
// `crate::sync_engine` 子树，与 `scheduler` 内的声明一致（不放到 crate 级）。
#[cfg(test)]
pub(in crate::sync_engine) use scheduler::{drain_write_signals, notify_write_signal};
pub use session::SessionEnvelope;

#[cfg(test)]
mod tests;
