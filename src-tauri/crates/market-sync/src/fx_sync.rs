//! ECB 汇率增量同步编排（issue #1545 设置页「同步汇率」手动入口；#1546 每日
//! 自动增量将复用同一「同步一次」单元，与标的行情每日刷新的后台形态解耦）。
//!
//! 一次同步的三段（网络等待不持连接，ADR-0069 决策 4 / #1275 会话接缝）：
//! ① 会话内读币种对——字典全部非本位币币种 → 本位币（方向口径与 ExchangeRate
//!    读路径一致，正反向兜底仍归查询层）；
//! ② 会话外取数推导——[`super::ecb`] 的 90 天增量入口（与 #1546 每日自动同步
//!    同一数据面；全量历史回填的窗口判据归 #1544，不在本单元）；
//! ③ 会话内落库——[`super::persist::persist_ecb_fx_series`] 单事务幂等落库，
//!    返回 [`super::persist::FxPersistReport`]（覆盖区间 / 条数，#1545 结果面）。
//!
//! 失败原因三态互不吞并（spec #1540「数据源不可达 vs 该来源无数据要能分辨」）：
//! - 取数网络失败 → `fx.source-unreachable`（新码，双语模板随票补齐）；
//! - 报文可解析但推导零点（该来源无可用数据，如响应缺本位币腿）→
//!   `fx.source-no-data`（新码）——零点不白落一次空事务、也不冒充成功；
//! - 报文非预期形状 → `fx.source-malformed` 原样透传（#1542 既有条件，不折算）。
//!
//! 幂等与并发：落库由整周覆盖幂等（#1543）保证，重复触发 / 多端并发各自落库
//! 不冲突（#1546 同款边界）；本单元不产同步 op、不发失效信号（当期汇率表不在
//! `ledger:prices-changed` 覆盖内，ADR-0031；自动采集按可重建缓存对待，ADR-0019
//! 修订记录）。

use rusqlite::Connection;

use ledger_infra::error::{AppError, Result};
use ledger_transaction::amount::default_currency_code;

use super::ecb::{ECB_HOSTS, derive_ecb_weekly_series, fetch_ecb_90d_incremental};
use super::http::{Pacer, build_client};
use super::persist::{FxPersistReport, persist_ecb_fx_series};
use super::session::ScopedSession;

/// 「数据源不可达」码化错误（spec #1540 用户故事 12：失败提示可自助补救——
/// 先分清是网络问题还是账本问题）。params 无动态值（ADR-0050）。
fn source_unreachable() -> AppError {
    AppError::coded(
        "fx.source-unreachable",
        "汇率数据源暂时不可达，请检查网络后重试",
    )
}

/// 「该来源无数据」码化错误：源可达、报文合法，但没有推导出任何可用汇率点
///（如响应缺本位币腿、字典缺非本位币）。params 无动态值（ADR-0050）。
fn source_no_data() -> AppError {
    AppError::coded(
        "fx.source-no-data",
        "数据源已连通，但当前没有可用的汇率数据，请稍后重试",
    )
}

/// 取数失败的归类（#1545 AC：不可达与无数据可分辨）：`fx.source-malformed`
/// 是独立条件（源返回了无法解析的内容），原样透传；其余（连接失败 / 超时 /
/// 限流放弃 / HTTP 状态异常，HTTP 层报 `AppError::Io`）收口为「数据源不可达」。
fn classify_fetch_error(error: AppError) -> AppError {
    if error.is_code("fx.source-malformed") {
        return error;
    }
    tracing::warn!(error = %error, "汇率数据源不可达（取数失败已归类）");
    source_unreachable()
}

/// 收集本次同步的币种对：字典全部非本位币币种 → 本位币（一条 SQL，按 base 代码
/// 升序保证报告与日志确定性）。方向口径与 ExchangeRate 读路径一致（#1543 /
/// ADR-0059：实体归属随币种参考数据），正反向兜底仍归查询层。
pub fn collect_fx_pairs(conn: &Connection) -> Result<Vec<(String, String)>> {
    let base = default_currency_code(conn)?;
    let mut stmt = conn.prepare("SELECT code FROM currencies WHERE code <> ?1 ORDER BY code")?;
    let rows = stmt.query_map([&base], |r| Ok((r.get::<_, String>(0)?, base.clone())))?;
    let mut pairs = Vec::new();
    for row in rows {
        pairs.push(row?);
    }
    Ok(pairs)
}

/// 生产入口：构造生产 HTTP 客户端与限速器后对接 ECB 官方主机（与
/// [`super::fund::fetch_fund_quote_production`] 同款形态；单请求同步，限速器
/// 不跨同步记忆）。
pub async fn run_fx_incremental_sync(session: &impl ScopedSession) -> Result<FxPersistReport> {
    let client = build_client()?;
    let mut pacer = Pacer::default();
    run_fx_incremental_sync_with(session, &client, &mut pacer, ECB_HOSTS).await
}

/// 注入入口（测试 / 后台车道换装）：主机与客户端/限速器由调用方提供，
/// 会话只约束连接取用（「持着连接做网络 I/O」在类型上不可表达）。
pub async fn run_fx_incremental_sync_with<S: ScopedSession>(
    session: &S,
    client: &reqwest::Client,
    pacer: &mut Pacer,
    hosts: &[&str],
) -> Result<FxPersistReport> {
    // ① 币种对（会话内，同步 rusqlite）。
    let pairs = session.with_connection(collect_fx_pairs).await?;
    // ② 取数 + 推导（会话外，网络等待以 await 表达）。
    let days = fetch_ecb_90d_incremental(client, pacer, hosts)
        .await
        .map_err(classify_fetch_error)?;
    let series = derive_ecb_weekly_series(&days, &pairs);
    // ③ 零点 = 该来源无数据：显式报错，不冒充成功、不白落空事务。
    if series.iter().all(|s| s.points.is_empty()) {
        return Err(source_no_data());
    }
    // ④ 落库（会话内，单事务幂等）。序列按值进作业闭包（作业要求 Send + 'static，
    // 编排现场的可变状态进不了闭包，session.rs 接缝约定）。
    session
        .with_connection(move |conn| persist_ecb_fx_series(conn, &series))
        .await
}
