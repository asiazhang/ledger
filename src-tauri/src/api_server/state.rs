//! HTTP 服务器状态与注入接缝：数据库连接 + 失效信号发射槽 + 可选东财基金/股票详情接缝 + 加密锁定门。

use std::sync::{Arc, Mutex};

use crate::db::boot::BootFailureGate;
use crate::db::encryption::EncryptionGate;
use crate::error::AppError;
use crate::events::SignalEmitter;
use crate::investment::{FundDetail, StockQuote};
use axum::extract::FromRef;
use rusqlite::Connection;

/// 东财基金详情获取函数接缝（issue #304 / ADR-0039）：`基金代码 → Result<FundDetail>`，
/// 查无此码以 `AppError::Invalid`（中文错误）上抛——与 IPC/BDD 的
/// `add_fund_by_code_with` 注入接缝同一形状约定。生产路径为东财 FundSearchAPI
/// （`fetch_fund_detail_production`）；HTTP 集成测试以注入桩离线驱动
/// （`setup_app_with_fund_fetch`），全部基金端点集成测试不触真实网络。
pub type FundDetailFetcher = Arc<dyn Fn(&str) -> Result<FundDetail, AppError> + Send + Sync>;

/// 东财股票行情获取函数接缝（issue #693 / ADR-0081）：`(市场, 代码) → Result<StockQuote>`，
/// 查无此码以码化中文错误上抛——注入桩形态与 [`FundDetailFetcher`] 同构。市场/代码
/// 形态解析在投资域单点完成（`resolve_stock_market`），本接缝只接归一化后的查询。
/// 生产路径为东财单点行情（`fetch_stock_quote_production`）；HTTP 集成测试以注入桩
/// 离线驱动，全部股票端点集成测试不触真实网络。
pub type StockQuoteFetcher = Arc<dyn Fn(&str, &str) -> Result<StockQuote, AppError> + Send + Sync>;

/// 失效信号发射槽（壳层 handler 的提取形状，ADR-0054 #367 修订）：写事务提交
/// 成功后经信号映射单点发射失效信号的机制槽位，收口于发射器接缝
/// `events::SignalEmitter`（spec #366 固化）。`None` = 集成测试跳过发射分支；
/// 生产注入 `AppHandle`（主线程非阻塞投递实现）经未尺寸化强转装入。
pub type EmitterSlot = Option<Arc<dyn SignalEmitter>>;

/// HTTP 服务器状态：数据库连接 + 失效信号发射槽 + 可选东财基金详情接缝 + 加密锁定门。
///
/// `emitter`（发射槽，ADR-0044 / ADR-0054）：`Some` 时写事务提交成功后经信号
/// 映射单点发射失效信号。生产路径由 `start_http_server` 注入
/// `Some(Arc::new(app))`——同一 `AppHandle` 发射器实现，行为与泛化前零变化；
/// 集成测试（`tests/api_server/`）不经真实 Tauri 运行时构建路由，传 `None`
/// 跳过发射分支，或注入受控发射器观察「写请求返回后信号最终到达」的
/// 外部行为（spec #367，`signal_delivery.rs`）。
///
/// `fund_fetch` 为东财基金详情获取接缝：`None` = 生产路径（真实东财，
/// `spawn_blocking` 连接锁外往返）；集成测试注入桩离线驱动（issue #304）。
/// `stock_fetch` 为东财股票行情获取接缝，同构（issue #693）。
///
/// `lock_gate` 为加密锁定门（issue #570 / ADR-0075 决策 5）：与 IPC 壳共享
/// 同一进程级门实例（`lib.rs` 创建的 [`crate::db::encryption::EncryptionGate`]），
/// 锁定期间门禁中间件对数据端点统一返回码化错误——AI 导入 HTTP 面在解锁前
/// 不可用；明文库路径门不锁，行为零变化。
///
/// `boot_gate` 为启动失败门（issue #601 / ADR-0075 决策 5 修订）：与 IPC 壳
/// 共享同一实例（[`crate::db::boot::BootFailureGate`]），失败期间数据端点
/// 同口径返回码化错误——占位连接不是业务库，不得触达。
#[derive(Clone)]
pub struct ApiState {
    pub conn: Arc<Mutex<Connection>>,
    pub emitter: EmitterSlot,
    pub fund_fetch: Option<FundDetailFetcher>,
    pub stock_fetch: Option<StockQuoteFetcher>,
    pub lock_gate: EncryptionGate,
    pub boot_gate: BootFailureGate,
}

impl FromRef<ApiState> for Arc<Mutex<Connection>> {
    fn from_ref(state: &ApiState) -> Self {
        state.conn.clone()
    }
}

impl FromRef<ApiState> for EmitterSlot {
    fn from_ref(state: &ApiState) -> Self {
        state.emitter.clone()
    }
}
