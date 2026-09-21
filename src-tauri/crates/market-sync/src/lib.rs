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
//! HTTP 网络爬取与增量同步编排在本域收口：
//! - [`bulk`]：行情批量取数面（ADR-0121 / issue #1374 / ADR-0130 决策 2）——
//!   新浪 `f_` 场外基金批量面（按本次现场的基金代码一次请求取回名称与最新净值，
//!   各整次同步最多一次逻辑请求）、fail-closed 降级、跨同步记忆与缺口
//!   容忍；
//! - [`channels`]：同步网络通道束（issue #1276）——六个逐标的抓取闭包 + 一个批量
//!   取数面的打包形态，生产接 HTTP 层、测试注入桩经命令壳换装；
//! - [`csrc`]：证监会基金电子披露取数单元（issue #1562 / ADR-0130）——官方场外
//!   基金净值披露的单只基金区间查询与解析：名称、单位净值、累计净值、净值日期
//!   与货基自报形态信号（单位净值为空、万份收益与七日年化有值，ADR-0126 决策 3
//!   换源后的判定信号）；DataTables 参数全集请求构造单点、汇总行与份额行混排
//!   过滤、已终止基金可取（存在性与最后一期净值的权威兜底）；异常响应
//!   fail-closed 报 sync.disclosure-source-malformed，不误判「查无此码」。
//!   判定确认（[`csrc::confirm_money_fund_form`]，issue #1563）接线在逐只刷新与
//!   历史首刷两确认点；区间取数的翻页面归 #1568 的查询创建接线消费；
//! - [`daily_refresh`]：现价刷新的后台每日形态（ADR-0122 决策 3 / issue #1377）
//!   ——启动后延迟补跑一次 + 每自然日窗口一次，与手动同步同形同取数面（后台
//!   车道让行、不占用户动作在途槽位、收尾裁决同判）；
//! - [`ecb`]：ECB 参考汇率取数单元（ADR-0019 修订记录 / issue #1542）——全量历史与
//!   90 天增量两个取数入口（Cube 报文解析）、同日两腿交叉推导（EUR 作基准腿）与
//!   周采样序列产出；非预期形状（空 / 非 XML / 截断）报 fx.source-malformed 码化
//!   错误，不静默产出空序列。本票只产出可落库序列，不落库、不接 UI；
//! - [`fund`]：场外基金按代码取价编排（issue #301 / ADR-0038；#1568 换源）——
//!   行情接入接缝查询半边的场外实例（统一载荷 [`ledger_investment::Quote`]，
//!   ADR-0103）：新浪批量面为主源、官方披露判定确认与已终止兜底，三臂编排
//!   收口本模块；
//! - [`fund_backfill`]：基金历史回填单元（issue #1062 / #1377 / #1388）——服务
//!   价格历史后台补全的逐只编排：首刷近两年、增量按水位，取数走新浪单只全历史面
//!   并由本地裁剪窗口（issue #1566）；
//! - [`fund_nav`]：基金净值同步共享件（issue #303 / ADR-0038 决策 6；issue #1388
//!   自通道拆出编排后留守）——净值点形状、净值水位窗口与水位读；东财 lsjz
//!   分页通道随逐只回退换源新浪全历史面退役删除（issue #1571），详情页数据
//!   文件的单请求全量通道（issue #1062）与档案通道（issue #1212）已随换源
//!   退役删除（issue #1566 / #1568）；
//! - [`fund_price_refresh`]：基金现价刷新单元（issue #1377 / #1388）——服务标的
//!   信息同步的逐只编排：批量面命中零请求、未命中退新浪单只全历史通道
//!   （issue #1571）；
//! - [`fx`]：ECB 汇率同步编排（issue #1544 / ADR-0019 修订记录）——汇率序列
//!   「要拉多深」的窗口判据单点：深度 = 账本中最早的非本位币痕迹日期（非本位币
//!   账户创建日 / 非本位币交易日取 MIN，软删排除）所属 ISO 周的周一再前推一周，
//!   与标的 K 线的「近两年」窗口和首刷 / 缺周队列彻底无关；按判据分派全量回填
//!   （按窗口起点裁剪）或 90 天增量，经通道束接 [`ecb`] 取数、[`persist`] 落库
//!   （单一事务、整周覆盖幂等、不产同步 op）。失败原因三态互不吞并（spec #1540
//!   「数据源不可达 vs 该来源无数据要能分辨」，issue #1545）：取数网络失败 →
//!   `fx.source-unreachable`，取数成功但推导零点 → `fx.source-no-data`，
//!   `fx.source-malformed` 原样透传。手动入口（#1545，壳层命令）/ 每日调度
//!   （#1546，[`fx_daily`] 后台车道）两个触发面已接线，本单元即其共同消费的
//!   唯一编排；
//! - [`history`]：价格历史后台补全（ADR-0122 / issue #1375）——派生事实队列
//!   （有价格通道但历史不完整，持仓优先）+ 一轮排空（后台补全专用的单只
//!   回填单元，issue #1377 起不再与现价刷新共用）+ 启动延迟与自然日窗口调度 +
//!   收尾裁决（置脏 + 价格失效信号）；唯一可见面是静默标的级计数（新事件名，见
//!   [`progress`]），并发布走势空态三态的运行态判据（[`ledger_investment::backfill`]）；
//! - [`http`]：HTTP 请求（含多主机切换、重试、限流冷却、Referer）与响应解析（报价
//!   / 日 K / 汇率 K），价格换算按精度位单点收口（批量报价与单点行情共用），可独立测试；
//! - [`incremental`]：标的信息同步编排（issue #103，#137 升级，#303 基金分区，#695
//!   ETF 纳入行情分区，#827 覆盖面放开至库内全部标的 + 名称随行刷新）——ADR-0122 /
//!   issue #1377 起**只刷现价**：现价 upsert + 有历史序列者当周采样点直落（不另发
//!   逐只请求）+ 汇率 K 线落 `fx_rate_history`（ADR-0019）+ 基金现价刷新（批量面
//!   命中零请求，未命中退逐只短窗）+ 数据源权威名称随行刷新（「随用随修 + 同步
//!   随行刷新」，ADR-0036/0081 修订）；历史采集归 [`history`]；
//! - [`lane`]：后台车道单轮骨架（issue #1426）——两条后台车道（历史补全 /
//!   每日现价刷新）共用的换装 / 会话 / 见证 / 收尾裁决 / 发射 / 失败日志单点，
//!   车道侧只留编排（[`lane::LaneRound`] 实现）与统计日志（ADR-0122 决策 3
//!   「同形调度」的代码单点）；
//! - [`model`]：域模型——标的信息同步结果类型（#407 随域归位；基金行情 DTO 已因
//!   #422 Q11 归属修正迁入投资域，ADR-0103 后又收口为 `ledger_investment::quote`
//!   的统一载荷 [`ledger_investment::Quote`]）；
//! - [`persist`]：ECB 汇率落库单元（issue #1543）——周采样序列 → `fx_rate_history`
//!   （币种对 × 周键整周覆盖幂等）+ 当期汇率表（每对最新一条，人工行不覆盖），
//!   单一事务、不产同步 op；东财 FX 通道的周采样 upsert 同住（#1551 退役前）；
//!   价格写入单点已随投资域归位迁入 [`ledger_investment::prices`]，#401 / ADR-0056）；
//! - [`progress`]：同步确定进度事件（issue #897 / ADR-0095）——带 payload
//!   `{ done, total }` 的 `ledger:instrument-sync-progress` 事件，事件名常量、
//!   载荷与 [`progress::ProgressEmitter`] 发射器接缝收口于此（不经失效信号映射，
//!   只共用 `events` 的主线程非阻塞投递机制）；
//! - [`session`]：作用域会话接缝（issue #1275 / ADR-0112 决策 5 挂载点⑥；
//!   async 形态见 issue #1412 / ADR-0125 决策 5）——编排获取数据库连接的唯一
//!   通道（取连接作业 async、经门面写槽裸作业），闭包内同步 rusqlite；生产实现
//!   [`session::FacadeWriteSession`]（命令壳侧与域内后台车道各自持门面句柄
//!   构造），编排抓取路径在类型上取不到连接；
//! - [`sina_fund`]：新浪场外基金取数单元（ADR-0130 决策 2 / issue #1564）——批量
//!   最新净值面（`f_` 前缀一次请求多只，GBK、必须带 Referer）与单只全历史面
//!   （一次请求取整只历史，含已终止基金末点）；货基行的字段错位（万份收益放在
//!   单位净值位）按「前一日单位净值位为空」单点判别并显式分类，错位行不产出
//!   价格点（ADR-0130 决策 6）；全历史空序列不等于「查无此码」，非预期形状
//!   fail-closed（批量面报 sync.fund-batch-source-malformed，#1612 对齐披露面
//!   码化形状）。批量面的 crate 内消费点：现价刷新（issue #1565）与基金按代码
//!   查询（issue #1568）；单只全历史面随价格历史后台补全（issue #1566）；
//! - [`stock`]：股票按代码查询的取数适配层——按（市场，代码）查腾讯行情
//!   （issue #693 / ADR-0081；issue #1567 接线腾讯，ADR-0130），解析与类型探测
//!   收口在 [`tencent`] 取数单元，同接缝查询半边的场内实例；
//! - [`tencent`]：腾讯行情批量报价取数单元（ADR-0130 决策 2/3 / issue #1558）
//!   ——一次请求携带多只沪深港美股票与场内基金（GBK、无需 Referer），解出代码 /
//!   名称 / 价格 / 价格日期 / 证券类型码 / 币种 / 交易所后缀；三套字段布局与类型
//!   探测收口单点，非预期响应 fail-closed；现价刷新接线见 [`channels`]（issue #1560），
//!   按代码查询 / 创建接线随 #1567；
//! - [`tencent_kline`]：腾讯日线 K 线取数单元（ADR-0130 决策 2 / issue #1559）
//!   ——「市场 + 代码」→ 腾讯查询键（沪深港前缀 + 美股三市场交易所后缀）、区间 /
//!   根数参数与日线报文解析（收盘价在下标 2；港美行可带多余元素）；无效代码返回
//!   空序列而非错误，非预期形状 fail-closed。本票只产出日线序列，接线归 #1561；
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
//! ADR-0103；#1543 起另消费汇率写入 `crud::upsert_auto_exchange_rate`，人工
//! 行保护）——四条即票面 AC 允许集全量，域→域均为上层消费下层的合法直呼
//!（ADR-0112 决策 2；本域为 P4 起点，不再反向依赖任何同级业务域）。对根包（壳层）
//! 与多端同步域零生产依赖，反向引用由 cargo 依赖图编译期拒绝（生产依赖面无根包，
//! dev-dependency 环只覆盖测试目标）。
//!
//! 兼容面（ADR-0112 决策 3「调用点零改动」）：根包以
//! `pub use ledger_market_sync as sync;` 再导出保留原引用路径——壳层
//! `commands::sync`（只做参数解包与信号发射，对外暴露 `sync_instrument_info`
//! 标的信息同步与 `sync_exchange_rates` 手动汇率同步（issue #1545）两个 IPC
//! 命令，前者只刷现价 + 名称随行刷新，issue #827 改名、ADR-0122 / issue #1377
//! 起 history 采集移出）、
//! `commands::investment` 与 `api_server` 的行情查询注入点、e2e 与汇总文档的
//! `crate::sync::…` / `tauri_app_lib::ledger_market_sync::…` 引用零改动。
//!
//! [`Quote`]: ledger_investment::Quote

mod bulk;
mod channels;
/// 证监会基金电子披露取数单元（issue #1562）：判定确认（[`csrc::confirm_money_fund_form`]，
/// #1563 接线逐只刷新与历史首刷、#1568 接线基金查询创建）与区间取数面（翻页 +
/// 完整性核验，#1568 接线已终止基金存在性兜底）。
mod csrc;
mod daily_refresh;
/// ECB 参考汇率取数单元（issue #1542）：单元本体与测试已就位，序列类型已由落库
/// 单元（persist，#1543）消费；两个取数入口与交叉推导已由汇率同步编排
///（[`fx`]，issue #1544）经通道束消费。
mod ecb;
mod fund;
mod fund_backfill;
mod fund_nav;
mod fund_price_refresh;
/// ECB 汇率同步编排（issue #1544，#1545 手动入口与 #1546 每日调度两个触发面
/// 已接线）：窗口判据与全量 / 增量
/// 两腿的分派、失败三态分类（fx.source-unreachable / fx.source-no-data /
/// fx.source-malformed 透传）已就位。
mod fx;
/// 汇率每日自动增量同步的后台车道（ADR-0019 修订记录 / issue #1546）：启动
/// 延迟补跑一次 + 每自然日窗口一次，经壳层后台服务编排单点拉起（issue #961
/// 名单）；与手动入口共用 [`fx`] 编排，失败记日志可见、已有汇率不受影响。
mod fx_daily;
mod history;
mod http;
mod incremental;
mod lane;
mod model;
mod persist;
mod progress;
mod session;
/// 新浪场外基金取数单元（issue #1564）：批量最新净值面随现价刷新（issue #1565，
/// 通道束的批量面闭包）与基金按代码查询（issue #1568）接线，单只全历史面随价格
/// 历史后台补全（issue #1566，通道束的全历史闭包）接线。
mod sina_fund;
mod stock;
/// 腾讯行情批量报价取数单元（issue #1558）：现价刷新接线见 [`channels`]
/// （issue #1560）；按代码查询 / 创建接线随 #1567。
mod tencent;
/// 腾讯日线 K 线取数单元（issue #1559）：单元本体与测试已就位，crate 内消费点
/// 随历史补全接线（issue #1561，通道束的日 K 闭包）落地。
mod tencent_kline;

#[cfg(test)]
mod tests;

pub use bulk::{
    BULK_DISABLE_PERIOD, BULK_FAILURE_THRESHOLD, BulkFetchCircuit, BulkFetchSurfaces, BulkNavPoint,
    FundBatch, FundNameDictionary, FundNavTable,
};
pub use channels::{
    FetchFundName, FetchFxKline, FetchKline, FetchMoneyFundForm, FetchNavHistory, FetchQuotes,
    QuoteItem, QuoteQuery, SyncFetchChannels, do_incremental_sync_channels,
};
pub use daily_refresh::{
    DailyPriceRefreshChannelsSlot, DailyPriceRefreshTimings, start_daily_price_refresh,
    start_daily_price_refresh_with,
};
// 通道束载荷 DTO（issue #1276）：通道束是壳层注入接缝的公开面，桩实现方需要
// 能命名与构造应答形状（QuoteItem 可构造；Kline/Nav 形状测试回空表即可命名；
// EcbDayRates 同理——FxSyncChannels 两条取数腿的应答形状，#1546 每日车道
// 接线测试经槽接缝注入桩束时命名）。
pub use ecb::EcbDayRates;
pub use fund::fetch_fund_quote_production;
pub use fund_nav::NavPoint;
pub use fx::{FxSyncChannels, FxSyncReport, sync_fx_rates};
pub use fx_daily::{
    DailyFxSyncChannelsSlot, DailyFxSyncTimings, start_daily_fx_sync, start_daily_fx_sync_with,
};
pub use history::{
    BackfillChannelsSlot, BackfillTimings, start_history_backfill, start_history_backfill_with,
};
pub use http::KlineBar;
pub use model::{SyncInstrumentInfoResult, WriteWitness};
pub use persist::FxPersistReport;
pub use progress::{
    BackfillProgressEmitter, HISTORY_BACKFILL_PROGRESS, INSTRUMENT_SYNC_PROGRESS, ProgressEmitter,
    SyncProgress,
};
pub use session::{FacadeWriteSession, ScopedSession};
pub use stock::fetch_stock_quote_production;
