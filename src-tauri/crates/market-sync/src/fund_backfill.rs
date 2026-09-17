//! 基金**历史回填**单元（issue #1388 自 `fund_nav` 拆出；ADR-0038 决策 6 /
//! ADR-0122 决策 2；issue #1377 起归价格历史后台补全专用）：首刷判据 = 磁盘上
//! 没有任何历史序列，首刷回填近两年——优先单请求全量通道（详情页数据文件，
//! issue #1062），失败 fail-closed 回退 lsjz 分页；增量以现价缓存净值日期为水位。
//!
//! 共享件（lsjz 报文解析、水位窗口、分页器、水位读）留守 `fund_nav`，本模块
//! 只收历史回填的编排。

use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::error::Result;
use ledger_investment::prices::{
    EASTMONEY_PRICE_SOURCE, MarketPriceWrite, price_value_to_cents, upsert_market_price,
};

use super::channels::FetchFuture;
use super::fund_nav::{
    LsjzPage, NavPages, NavPoint, NavQuery, fetch_nav_pages, nav_window, read_fund_watermark,
};
use super::http::KlineBar;
use super::session::ScopedSession;

/// 单只基金单次同步的页数上限：近两年窗口约 25 页（≈500 个净值日 ÷ 20），
/// 上限兜底防异常 TotalCount 导致的失控翻页。触顶即窗口已知未采全，整只不落库
///（见 `NavPages::truncated`，ADR-0122 决策 8 / issue #1373）。
const MAX_NAV_PAGES: u64 = 40;

/// 单只基金历史回填的结局（issue #1377 走势空态三态的判据输入）：`written` =
/// 是否实际落库；`inconclusive` = 本轮窗口不完整（部分页空响应或页数触顶）——
/// 整只不落库、结果不可信，后台补全据此把该只记为「待重试」而非「无数据」
///（被拦截 ≠ 数据源没有可采序列）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct BackfillOutcome {
    pub(super) written: bool,
    pub(super) inconclusive: bool,
}

/// 单只 fund 标的的**历史回填**单元（ADR-0038 决策 6 / ADR-0122 决策 2；issue
/// #1377 起归价格历史后台补全专用——现价刷新走
/// [`super::fund_price_refresh::refresh_one_fund_price`]，「同步标的信息」不再承担首刷）：
/// **首刷判据 = 磁盘上没有任何历史序列**（issue
/// #1059）。首刷回填近两年——优先走单请求全量通道（详情页数据文件，issue
/// #1062），抓取/解析失败或窗口内无点则 fail-closed 回退 lsjz 分页通道；已有
/// 历史序列的基金走 lsjz 分页，以现价缓存的净值日期为水位增量（水位当日不重拉、
/// 只取水位次日之后）。净值点降采样落 PriceHistory（同周整周覆盖幂等），窗口内
/// 最新公布净值落现价缓存（现价 = 单位净值、priced_at = nav_date = 净值日期，
/// 与 #301 添加基金同形）。周采样与现价落库在**一只一个事务**里整只一次提交
///（ADR-0122 决策 8 / issue #1373）：第 N 个周点写入失败或中途中断整体回滚，
/// 磁盘上不留半根历史——「有历史序列」与「历史完整」由此等价，首刷判据（ADR-0038
/// 决策 6）依赖的正是这个等价。页级推进经注入的 `on_page` 回调透传（issue
/// #1061——每页抓取返回后报告「已完成页/总页数」，抓取内部的退避/重试等待不
/// 产生推进）。
///
/// **前置条件**：`fund` 为 6 位真实代码的有通道基金行。跳过语义：首刷查无净值
/// 与**空响应/被拦截**（issue #1059）不报错不中断，以 [`BackfillOutcome`] 表达
/// 结局；其中**本轮窗口不完整**（部分页空响应或页数触顶，ADR-0122 决策 8 /
/// issue #1373）整只不落库并记 `inconclusive`，不留半根历史；单只网络失败上抛
///（后台补全编排单只失败不中断本轮）。
pub(super) async fn backfill_one_fund_history<Q, N, S, P>(
    session: &Q,
    fund: &super::incremental::SyncInstrument,
    fetch_nav: &mut N,
    fetch_nav_full_series: &mut S,
    on_page: &mut P,
) -> Result<BackfillOutcome>
where
    // 作用域会话接缝（issue #1275）：本函数读写库的唯一通道，签名层面取不到连接。
    Q: ScopedSession,
    N: FnMut(&NavQuery) -> FetchFuture<LsjzPage> + Send,
    S: FnMut(&str) -> FetchFuture<Vec<NavPoint>> + Send,
    P: FnMut(u64, u64) + Send,
{
    let today = super::incremental::beijing_today();
    // 水位 = 现价缓存的净值日期（股票行恒 NULL，基金行由 #301/本通道写入）；首刷
    // 判据 = 磁盘上没有任何历史序列（issue #1059）：添加基金 / AI 导入在
    // 「按代码即拉」时已把最新净值落现价缓存（水位有值）但 PriceHistory 为空，
    // 若以水位作增量起点，增量窗口只剩「水位次日」而近两年回填静默落空
    //（#303 验收在真实账本上未成立的根因）。水位只服务已有历史序列的增量。
    // 两条读经作用域会话短暂取一次连接完成（issue #1275）；抓取前不再触碰连接。
    let (watermark, has_history) = read_fund_watermark(session, fund).await?;
    let first_fill = !has_history;
    let window_watermark = if first_fill {
        None
    } else {
        watermark.as_deref()
    };
    let (start, end) = nav_window(window_watermark, today);

    // 首刷优先走单请求全量通道（ADR-0038 决策 6 修订，issue #1062）：一次请求拿
    // 整只基金的历史单位净值，本地裁剪到与分页通道相同的近两年窗口后再用。抓取
    // 失败 / 解析不出序列 / 窗口内无点都 fail-closed 回退分页通道，不静默丢数据；
    // 已有历史序列的增量不触碰本通道（日常增量仍走 lsjz）。
    let full_points = if first_fill {
        match fetch_nav_full_series(&fund.symbol).await {
            Ok(points) => {
                let clipped: Vec<NavPoint> = points
                    .into_iter()
                    .filter(|p| {
                        p.date.as_str() >= start.as_str() && p.date.as_str() <= end.as_str()
                    })
                    .collect();
                if clipped.is_empty() {
                    tracing::warn!(
                        code = %fund.symbol,
                        "单请求全量净值通道无窗口内净值，回退分页通道"
                    );
                    None
                } else {
                    Some(clipped)
                }
            }
            Err(error) => {
                tracing::warn!(
                    code = %fund.symbol, %error,
                    "单请求全量净值通道失败，回退分页通道"
                );
                None
            }
        }
    } else {
        None
    };

    // 单请求通道命中即免去分页；否则回退既有分页通道（首刷近两年 / 增量水位次日）。
    let collected = match full_points {
        Some(points) => NavPages {
            points,
            blocked: false,
            truncated: false,
        },
        None => {
            fetch_nav_pages(
                fetch_nav,
                &fund.symbol,
                &start,
                &end,
                MAX_NAV_PAGES,
                on_page,
            )
            .await?
        }
    };

    if collected.points.is_empty() {
        if collected.blocked {
            // 空响应（Data 缺省 / 非对象）= 疑似被拦截或异常：结果不可信，不
            // 得计入成功（issue #1059）。此路记 `inconclusive`——被拦截 ≠
            // 数据源没有可采序列，空态按「待重试」而非「无数据」（issue #1377）。
            tracing::warn!(
                code = %fund.symbol, first_fill, watermark = ?watermark,
                "历史净值返回空响应（疑似被拦截/异常），本轮无法判定是否有新净值"
            );
            return Ok(BackfillOutcome {
                written: false,
                inconclusive: true,
            });
        }
        // 首刷查无净值（查无此码 / 新基金未公布首期）与增量窗口内确实无新净值
        //（T+1 正常空窗）：结局确定——无可采数据 / 已是最新。
        return Ok(BackfillOutcome {
            written: false,
            inconclusive: false,
        });
    }
    if !collected.is_complete() {
        // 本轮窗口不完整（部分页空响应或页数触顶，ADR-0122 决策 8 / issue #1373）：
        // 整只不落库——宁可整只留空重试，不留半根历史。半根历史会让「有历史序列」
        // 冒充「历史完整」，首刷判据（ADR-0038 决策 6）据此失效。结局按待重试。
        tracing::warn!(
            code = %fund.symbol, first_fill,
            blocked = collected.blocked, truncated = collected.truncated,
            "历史净值窗口不完整（部分页空响应或页数触顶），整只不落库待下次重试"
        );
        return Ok(BackfillOutcome {
            written: false,
            inconclusive: true,
        });
    }
    let points = collected.points;

    // 现价 = 窗口内最新公布单位净值；priced_at = nav_date = 净值日期
    // （与 #301 添加基金同形；nav_date 兼任下次同步的水位）。
    // let-else 显式防线（#434，ADR-0060 A 类临时豁免已摘）：points 非空由
    // 前文判空保证，此臂理论不可达；一旦前置防线被移除，此处记警告并跳过
    // 该只、不中断同步。
    let Some(latest) = points.iter().max_by_key(|p| p.date.as_str()) else {
        tracing::warn!(code = %fund.symbol, "净值点意外为空，跳过现价更新");
        return Ok(BackfillOutcome {
            written: false,
            inconclusive: false,
        });
    };

    // 周采样与现价落库同在一只一个事务里（ADR-0122 决策 8 / issue #1373）：单只
    // 标的的历史回填整只一次提交，第 N 个周点写入失败或中途中断整体回滚，磁盘上
    // 不留半根历史——「有历史序列」与「历史完整」由此等价，首刷判据（ADR-0038
    // 决策 6）依赖的正是这个等价。单位净值即价格（ADR-0038 决策 3），与日线共用
    // 降采样与「整周覆盖」幂等（同周重复获取零重复行）。落库经作用域会话短暂取
    // 一次连接（issue #1275）；抓取已在会话之外完成。事务经 [`ensure_transaction`]
    // （ADR-0033 嵌套感知）：连接 autocommit 则自持事务、已在事务中则加入外层。
    // 「一只一事务」的前提是写错误**不被吞**——本函数的写失败一律经 `?` 上抛，
    // 加入外层时由外层持有者回滚，同样整只不留；任一层吞掉写错误才会破坏前提。
    let bars: Vec<KlineBar> = points
        .iter()
        .map(|p| KlineBar {
            date: p.date.clone(),
            close: p.nav,
        })
        .collect();
    let instrument_id = fund.instrument_id.clone();
    let currency = fund.currency.clone();
    let latest_date = latest.date.clone();
    let latest_nav = latest.nav;
    session
        .with_connection(move |conn| {
            ensure_transaction(conn, || {
                // 周采样落库（与日 K 回填共用单点），与现价写在同一事务里整只一次提交。
                super::incremental::write_weekly_price_history(
                    conn,
                    &instrument_id,
                    &currency,
                    &bars,
                )?;
                upsert_market_price(
                    conn,
                    &MarketPriceWrite {
                        instrument_id: &instrument_id,
                        price_cents: price_value_to_cents(latest_nav),
                        currency_code: &currency,
                        // 基金现价时点 = 净值日期（现价的行情日期就是净值本身对应的日期）；
                        // nav_date 兼任下次同步的水位。
                        priced_at: &latest_date,
                        nav_date: Some(&latest_date),
                        source: Some(EASTMONEY_PRICE_SOURCE),
                    },
                )?;
                Ok(())
            })
        })
        .await?;
    Ok(BackfillOutcome {
        written: true,
        inconclusive: false,
    })
}
