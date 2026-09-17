//! HTTP 服务器状态与注入接缝：数据库连接 + 失效信号发射槽 + 可选东财基金/股票详情接缝 + 加密锁定门。

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use axum::extract::FromRef;
use ledger_infra::db::boot::BootFailureGate;
use ledger_infra::db::encryption::EncryptionGate;
use ledger_infra::db::{DbReadHandle, DbSlotPair, DbWriteHandle};
use ledger_infra::error::AppError;
use ledger_infra::events::SignalEmitter;
use ledger_investment::Quote;
use rusqlite::Connection;

/// 东财基金报价获取函数接缝（issue #304 / ADR-0039）：`基金代码 → future<Result<Quote>>`，
/// 查无此码以 `AppError::Invalid`（中文错误）上抛——与 IPC 壳按代码即拉生产入口
/// 同一载荷（行情接入统一报价，ADR-0103）；本接缝是查询端点的场外通道注入
///（按代码一参），与接缝的统一注入签名（代码，市场）同源不同面。生产路径为
/// 东财 FundSearchAPI（`fetch_fund_quote_production`，async 形态直接 await，
/// ADR-0125 决策 7 / issue #1413）；HTTP 集成测试以注入桩离线驱动
///（`setup_app_with_fund_fetch`），全部基金端点集成测试不触真实网络。
pub type QuoteFuture = Pin<Box<dyn Future<Output = Result<Quote, AppError>> + Send>>;
pub type FundQuoteFetcher = Arc<dyn Fn(&str) -> QuoteFuture + Send + Sync>;

/// 东财股票行情获取函数接缝（issue #693 / ADR-0081）：`(市场, 代码) → future<Result<Quote>>`，
/// 查无此码以码化中文错误上抛——注入桩形态与 [`FundQuoteFetcher`] 同构（统一报价
/// 载荷，ADR-0103）。市场/代码
/// 形态解析在投资域单点完成（`resolve_stock_quote_candidates`），本接缝只接归一化后的查询；
/// 美股 ticker 的候选遍历由壳层共享助手 `fetch_stock_quote_first_hit_for_api` 执行（issue #696）。
/// 生产路径为东财单点行情（`fetch_stock_quote_production`，async 形态直接 await）；
/// HTTP 集成测试以注入桩离线驱动，全部股票端点集成测试不触真实网络。
pub type StockQuoteFetcher = Arc<dyn Fn(&str, &str) -> QuoteFuture + Send + Sync>;

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
/// `conn` 为写连接（单写者互斥的既有接缝）；`read_conn` 为只读读连接
///（issue #1280 / ADR-0117 决策 1）——读端点经 [`ReadConn`] 提取器消费，
/// 写端点照旧经 `FromRef<ApiState> for Arc<Mutex<Connection>>` 取写连接。
/// 两槽同用「共享句柄 + 互斥体内槽替换」形态：壳层换连后本状态持有的克隆
/// 同步可见（ADR-0080）。
///
/// `fund_fetch` 为东财基金详情获取接缝：`None` = 生产路径（真实东财，async
/// 生产入口直接 await，连接锁外往返）；集成测试注入桩离线驱动（issue #304）。
/// `stock_fetch` 为东财股票行情获取接缝，同构（issue #693）。
///
/// `lock_gate` 为加密锁定门（issue #570 / ADR-0075 决策 5）：与 IPC 壳共享
/// 同一进程级门实例（`lib.rs` 创建的 [`ledger_infra::db::encryption::EncryptionGate`]），
/// 锁定期间门禁中间件对数据端点统一返回码化错误——AI 导入 HTTP 面在解锁前
/// 不可用；明文库路径门不锁，行为零变化。
///
/// `boot_gate` 为启动失败门（issue #601 / ADR-0075 决策 5 修订）：与 IPC 壳
/// 共享同一实例（[`ledger_infra::db::boot::BootFailureGate`]），失败期间数据端点
/// 同口径返回码化错误——占位连接不是业务库，不得触达。
#[derive(Clone)]
pub struct ApiState {
    pub conn: Arc<Mutex<Connection>>,
    pub read_conn: Arc<Mutex<Connection>>,
    pub emitter: EmitterSlot,
    pub fund_fetch: Option<FundQuoteFetcher>,
    pub stock_fetch: Option<StockQuoteFetcher>,
    pub lock_gate: EncryptionGate,
    pub boot_gate: BootFailureGate,
}

/// 读端点的门面句柄提取器（issue #1280 / ADR-0117；取用形态经 ADR-0125 决策 1/4
/// 更替为门面读句柄，issue #1410）：提取只读读句柄。
/// `FromRef<ApiState> for DbWriteHandle` 保持返回写句柄（写端点既有提取零改动，
/// 只换类型），读侧以新类型区分——读端点写法只多一层新类型解包，
/// 读入口签名与读路径语义不变（ADR-0104）。
#[derive(Clone)]
pub struct ReadConn(pub DbReadHandle);

impl FromRef<ApiState> for ReadConn {
    fn from_ref(state: &ApiState) -> Self {
        Self(state.read_handle())
    }
}

impl FromRef<ApiState> for DbWriteHandle {
    fn from_ref(state: &ApiState) -> Self {
        state.write_handle()
    }
}

impl FromRef<ApiState> for EmitterSlot {
    fn from_ref(state: &ApiState) -> Self {
        state.emitter.clone()
    }
}

impl ApiState {
    /// 连接槽对（issue #1410）：HTTP 壳的连接面经槽对取句柄——槽保持原形
    /// （两槽共享句柄 + 互斥体内槽替换，ADR-0080），句柄按槽对解析门面。
    pub fn slots(&self) -> DbSlotPair {
        DbSlotPair::new(self.conn.clone(), self.read_conn.clone())
    }

    /// 写侧门面句柄（写端点的提取形态，issue #1410）。
    pub fn write_handle(&self) -> DbWriteHandle {
        self.slots().write_handle()
    }

    /// 读侧门面句柄（读端点的提取形态，issue #1410）。
    pub fn read_handle(&self) -> DbReadHandle {
        self.slots().read_handle()
    }
}

#[cfg(test)]
mod quote_seam_guard_tests {
    //! 两壳行情接缝的异步形态守门（ADR-0125 决策 7 / issue #1413，删除即变红）：
    //! 基金 / 股票行情获取接缝的生产分支必须在异步上下文直接 `await` async 生产
    //! 入口——阻塞包装（同步闭包 + `spawn_blocking` + JoinError 归一化）是 #1403
    //! 纪律退化的载体，回归即红。生产行为分支触真实网络，测试面不可达（全部
    //! 行情端点集成测试离线驱动，见 tests/api_server/），故以源码扫描守门替代
    //! （先例：#959/#961 接线在测试不可达处以扫描守门替代，ADR-0087）；掩码器具
    //! 复用 `signals_cross_check`，规则无第二份。

    #[test]
    fn quote_fetch_seams_have_no_blocking_wrapper() {
        let funds = crate::test_support::scan::mask_non_code(include_str!("handlers/funds.rs"));
        let stocks = crate::test_support::scan::mask_non_code(include_str!("handlers/stocks.rs"));

        for (name, text) in [("funds.rs", &funds), ("stocks.rs", &stocks)] {
            assert!(
                !text.contains("spawn_blocking"),
                "{name} 的行情获取接缝不得回归 spawn_blocking 阻塞包装（ADR-0125 决策 7 / \
                 issue #1413）：async 生产入口在异步上下文直接 await"
            );
            assert!(
                !text.contains("任务执行失败"),
                "{name} 的行情获取接缝不得回归 JoinError 归一化错误消息（ADR-0125 决策 7 / \
                 issue #1413）"
            );
        }

        // 生产分支接线不被绕过（ADR-0125 决策 8「生产分支覆盖」的扫描半边）：
        // 接缝生产分支必须直呼行情同步域的生产拉取入口，不得另行拼装取数路径。
        let fund_calls = funds.matches("fetch_fund_quote_production(").count();
        assert_eq!(
            fund_calls, 1,
            "基金接缝生产分支应恰直呼一次 fetch_fund_quote_production"
        );
        let stock_calls = stocks.matches("fetch_stock_quote_production(").count();
        assert_eq!(
            stock_calls, 1,
            "股票接缝生产分支应恰直呼一次 fetch_stock_quote_production"
        );
    }

    #[test]
    fn quote_fetcher_types_are_async_shaped() {
        let state = crate::test_support::scan::mask_non_code(include_str!("state.rs"));
        assert!(
            state.contains("pub type QuoteFuture = Pin<Box<dyn Future"),
            "行情获取接缝的 future 装箱类型应在位（QuoteFuture）——接缝闭包为 \
             async 形态（ADR-0125 决策 7 / issue #1413），类型退回同步签名即编译期 \
             拒绝异步生产入口"
        );
        assert!(
            !state.contains("Fn(&str) -> Result<Quote")
                && !state.contains("Fn(&str, &str) -> Result<Quote"),
            "行情获取接缝闭包类型不得退回同步签名（同步闭包是阻塞包装的载体，\
             ADR-0125 决策 7 / issue #1413）"
        );
    }
}
