// 测试整体豁免（ADR-0060）：clippy 六件套 deny 仅约束生产路径；单元测试目标
// （含 src/** 内 #[cfg(test)] 模块与外挂 tests/）经 crate 根 cfg(test) 整体
// 放行，生产构建零放宽。
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable
    )
)]

//! 行情同步域（MarketSync，#407 域目录化归位 ADR-0056；spec #1086 / issue #1106
//! 自根包域目录拆为 workspace 成员 `ledger-market-sync`）。
//!
//! HTTP 网络爬取、东财基金访问与增量同步编排在本域收口：
//! - [`http`]：HTTP 请求（含多主机切换、重试、限流冷却、Referer）与响应解析（报价
//!   / 日 K / 汇率 K），价格换算按精度位单点收口（批量报价与单点行情共用），可独立测试；
//! - [`fund`]：东财基金报价访问（按代码即拉，issue #301 / ADR-0038）——行情接入
//!   接缝查询半边的场外实例（统一载荷 [`ledger_investment::Quote`]，ADR-0103）；
//! - [`stock`]：东财股票单点行情访问——按（市场，代码）实时查询（issue #693 /
//!   ADR-0081），类型特征探测单点隔离，同接缝查询半边的场内实例；
//! - [`fund_nav`]：东财历史净值通道——lsjz 访问、报文解析、水位语义与基金分区
//!   编排（issue #303 / ADR-0038 决策 6）；首刷深回填另走详情页数据文件的
//!   单请求全量通道、失败 fail-closed 回退 lsjz 分页（issue #1062）；
//! - [`persist`]：`fx_rate_history` 周采样 upsert（issue #137；价格写入单点已随
//!   投资域归位迁入 [`ledger_investment::prices`]，#401 / ADR-0056）；
//! - [`incremental`]：标的信息同步编排（issue #103，#137 升级，#303 基金分区，#695
//!   ETF 纳入行情分区，#827 覆盖面放开至库内全部标的 + 名称随行刷新）——ADR-0122 /
//!   issue #1377 起**只刷现价**：现价 upsert + 有历史序列者当周采样点直落（不另发
//!   逐只请求）+ 汇率 K 线落 `fx_rate_history`（ADR-0019）+ 基金现价刷新（批量面
//!   命中零请求，未命中退逐只短窗）+ 数据源权威名称随行刷新（「随用随修 + 同步
//!   随行刷新」，ADR-0036/0081 修订）；历史采集归 [`history`]；
//! - [`daily_refresh`]：现价刷新的后台每日形态（ADR-0122 决策 3 / issue #1377）
//!   ——启动后延迟补跑一次 + 每自然日窗口一次，与手动同步同形同取数面（后台
//!   车道让行、不占用户动作在途槽位、收尾裁决同判）；
//! - [`history`]：价格历史后台补全（ADR-0122 / issue #1375）——派生事实队列
//!   （有价格通道但历史不完整，持仓优先）+ 一轮排空（后台补全专用的单只
//!   回填单元，issue #1377 起不再与现价刷新共用）+ 启动延迟与自然日窗口调度 +
//!   收尾裁决（置脏 + 价格失效信号）；唯一可见面是静默标的级计数（新事件名，见
//!   [`progress`]），并发布走势空态三态的运行态判据（[`ledger_investment::backfill`]）；
//! - [`channels`]：同步网络通道束（issue #1276）——六个逐标的抓取闭包 + 两个批量
//!   取数面的打包形态，生产接 HTTP 层、测试注入桩经命令壳换装；
//! - [`bulk`]：行情批量取数面（ADR-0121 / issue #1374）——名称全量字典 + 场外基金
//!   净值全市场批量面（各整次同步一次请求）、fail-closed 降级、同步内熔断、跨同步
//!   记忆与缺口容忍；取数方式与价格来源正交（来源标记不变）；
//! - [`progress`]：同步确定进度事件（issue #897 / ADR-0095）——带 payload
//!   `{ done, total }` 的 `ledger:instrument-sync-progress` 事件，事件名常量、
//!   载荷与 [`progress::ProgressEmitter`] 发射器接缝收口于此（不经失效信号映射，
//!   只共用 `events` 的主线程非阻塞投递机制）；
//! - [`session`]：作用域会话接缝（issue #1275）——编排获取数据库连接的唯一
//!   通道，域定义 trait、壳层实现并在命令壳接线；编排抓取路径在类型上取不到
//!   连接（父 spec #1274 预重构，行为零变化）；
//! - [`model`]：域模型——标的信息同步结果类型（#407 随域归位；基金行情 DTO 已因
//!   #422 Q11 归属修正迁入投资域，ADR-0103 后又收口为 `ledger_investment::quote`
//!   的统一载荷 [`ledger_investment::Quote`]）；
//! - `tests`：域内测试（HTTP 层经本地 HTTP 服务独立测试，不依赖真实网络）。
//!
//! 标的全量同步（修字典）已整体退役（ADR-0081 决策 3，issue #698）：编排、
//! 进度/取消事件与中断状态随命令删除，股票字典修正归「按代码查询/创建带回
//! 权威名称」（投资域 `stock` / `crud`）。
//!
//! 依赖方向（spec #1086 / issue #1106 AC）：本 crate 消费**基础设施**（`db` 时间
//! 与身份工厂 / `error` 码化错误 / `events` 主线程非阻塞投递）、**同步协议**
//!（op 落库行的 `device_id` 字段）、**核心交易域**（币种缺省推导
//! `transaction::amount::default_currency_code`）与**投资域**（价格写入单点
//! `prices`、名称随行刷新 `crud`、通道派生 `channel` 与统一报价载荷 [`Quote`]，
//! ADR-0103）——四条即票面 AC 允许集全量，域→域均为上层消费下层的合法直呼
//!（ADR-0112 决策 2；本域为 P4 起点，不再反向依赖任何同级业务域）。对根包（壳层）
//! 与多端同步域零生产依赖，反向引用由 cargo 依赖图编译期拒绝（生产依赖面无根包，
//! dev-dependency 环只覆盖测试目标）。
//!
//! 兼容面（ADR-0112 决策 3「调用点零改动」）：根包以
//! `pub use ledger_market_sync as sync;` 再导出保留原引用路径——壳层
//! `commands::sync`（只做参数解包与信号发射，对外暴露 `sync_instrument_info`
//! 标的信息同步一个 IPC 命令，只刷现价 + 名称随行刷新，issue #827 改名、
//! ADR-0122 / issue #1377 起 history 采集移出）、
//! `commands::investment` 与 `api_server` 的行情查询注入点、e2e 与汇总文档的
//! `crate::sync::…` / `tauri_app_lib::ledger_market_sync::…` 引用零改动。
//!
//! [`Quote`]: ledger_investment::Quote

mod bulk;
mod channels;
mod daily_refresh;
mod fund;
mod fund_nav;
mod history;
mod http;
mod incremental;
mod js;
mod model;
mod persist;
mod progress;
mod session;
mod stock;

#[cfg(test)]
mod tests;

pub use bulk::{
    BULK_DISABLE_PERIOD, BULK_FAILURE_THRESHOLD, BulkFetchCircuit, BulkFetchSurfaces, BulkNavPoint,
    FundNameDictionary, FundNavTable,
};
pub use channels::{
    FetchFundName, FetchKline, FetchNavFull, FetchNavPage, FetchUlist, SyncFetchChannels,
    do_incremental_sync_channels,
};
pub use daily_refresh::{
    DailyPriceRefreshChannelsSlot, DailyPriceRefreshTimings, start_daily_price_refresh,
    start_daily_price_refresh_with,
};
// 通道束载荷 DTO（issue #1276）：通道束是壳层注入接缝的公开面，桩实现方需要
// 能命名与构造应答形状（StockItem 可构造；Kline/Nav 形状测试回空表即可命名）。
pub use fund::fetch_fund_quote_production;
pub use fund_nav::{LsjzPage, NavPoint, NavQuery};
pub use history::{
    BackfillChannelsSlot, BackfillTimings, start_history_backfill, start_history_backfill_with,
};
pub use http::{KlineBar, StockItem};
pub use model::{SyncInstrumentInfoResult, WriteWitness};
pub use progress::{
    BackfillProgressEmitter, FundNavProgress, HISTORY_BACKFILL_PROGRESS, INSTRUMENT_SYNC_PROGRESS,
    ProgressEmitter, SyncProgress,
};
pub use session::ScopedSession;
pub use stock::fetch_stock_quote_production;
