//! 股票按代码查询的取数适配层（ADR-0130 决策 2 / issue #1567；市场闭集类型化
//! issue #1673）：按（路由，代码）单只查询腾讯行情批量报价端点（`GET /q=<市场
//! 前缀代码>`，单只 = 一批一条），解析与类型探测收口在 [`super::tencent`] 取数
//! 单元（三套字段布局、类型码探测、美股交易所后缀映射、fail-closed 语义均归该
//! 单点），本模块只做查询键构造、命中挑选与统一报价载荷投影，投影为行情接入
//! 接缝的 [`Quote`]（ADR-0103 决策 2），供 stocks 查询端点与创建增强、添加投资
//! 标的壳共用（即接缝的**查询半边**场内通道，注入签名与基金报价获取接缝同形）。
//! 路由位是投资域解析流程内部小闭集 [`StockRoute`]（沪深港精确市场 + 美股聚合
//! `Us`）——四值全部可构造查询键，键构造是全函数，不可路由市场的运行时兜底在
//! 类型上不可表达。网络复用行情 HTTP 层的重试与限流；测试以本地 HTTP 服务注入
//! 主机，驱动生产适配钉住请求形态（删除接线即红）。
//!
//! 命中判定 = 响应回显代码与请求归一化代码全等（回显全等是防错配的关键，与
//! 东财 f57 回显同判据）；未命中（数据源明示批量内全部无效，或回显不等）返回
//! 码化「查无此码」，网络失败 / 风控拦截由 HTTP 层与取数单元 fail-closed 上抛。

use super::http::{Pacer, build_client};
use super::tencent::{TENCENT_QUOTE_HOSTS, fetch_tencent_batch, tencent_query_key};
use ledger_infra::error::{AppError, Result};
use ledger_investment::{Quote, StockRoute};

/// 按（路由，代码）拉取单只腾讯行情并投影统一报价。路由为投资域候选解析产物
/// [`StockRoute`]（沪深港精确市场，或美股聚合路由 `Us`——腾讯不区分交易所，
/// 精确市场由响应自报后缀给出，ADR-0130 决策 2/4；issue #1673 类型化后键构造
/// 全函数化，无「市场无法路由」分支）；查无此码返回码化中文错误（Invalid → 400），
/// 网络失败 / 风控拦截由 HTTP 层重试后上抛（Io → 500）。
pub(super) async fn fetch_stock_quote(
    client: &reqwest::Client,
    pacer: &mut Pacer,
    hosts: &[&str],
    route: StockRoute,
    code: &str,
) -> Result<Quote> {
    let key = tencent_query_key(route, code);
    let quotes = fetch_tencent_batch(client, pacer, hosts, &key).await?;
    match quotes.into_iter().find(|quote| quote.code == code) {
        Some(quote) => Ok(quote.into_quote()),
        None => Err(AppError::codedp(
            "sync.stock-not-found",
            format!("查无股票代码 {code}，请核对后重试"),
            &[code],
        )),
    }
}

/// 生产拉取入口：构建客户端与限流器后执行单次行情查询（不经数据库连接，
/// 供 HTTP 壳在连接锁外完成网络往返，先例：`fetch_fund_quote_production`，
/// 单请求叠加限流冷却重试最长可达分钟级）。async 形态（ADR-0125 决策 5/7，
/// issue #1413）：网络等待以 `await` 表达，在异步上下文内直接可调，#1411 的
/// 过渡同步桥已随接缝 async 化拆除。
pub async fn fetch_stock_quote_production(route: StockRoute, code: &str) -> Result<Quote> {
    let client = build_client()?;
    let mut pacer = Pacer::default();
    fetch_stock_quote(&client, &mut pacer, TENCENT_QUOTE_HOSTS, route, code).await
}
