//! 基金**历史回填**单元（issue #1388 自 `fund_nav` 拆出；ADR-0038 决策 6 /
//! ADR-0122 决策 2；issue #1377 起归价格历史后台补全专用）：首刷判据 = 磁盘上
//! 没有任何历史序列，首刷回填近两年；增量以现价缓存净值日期为水位、取水位次日
//! 之后。取数走**新浪单只全历史面**（issue #1566 / ADR-0130 决策 2：一次请求
//! 整只历史单位净值，含已终止基金），窗口语义由本地裁剪表达——首刷裁剪到近两年
//! 窗口、增量裁剪到水位次日之后，不依赖服务端窗口过滤行为。
//! 未打标标的回填前先经官方披露判定门（issue #1563 / ADR-0126 决策 3 换源）：
//! 确认即打标收尾、零抓取，万份收益不得经取数面冒充单位净值落库（#1342）。
//!
//! 水位窗口与水位读留守 `fund_nav`，本模块只收历史回填的编排。

use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::error::Result;
use ledger_investment::PriceChannel;
use ledger_investment::prices::{
    MarketPriceWrite, SINA_PRICE_SOURCE, price_value_to_cents, upsert_market_price,
};

use super::channels::FetchFuture;
use super::fund_nav::{
    NavPoint, mark_constant_price_on_confirm, nav_window, read_fund_watermark, trim_to_window,
};
use super::http::KlineBar;
use super::session::ScopedSession;

/// 单只基金历史回填的结局（issue #1377 走势空态三态的判据输入）：`written` =
/// 是否实际落库；`inconclusive` = 本轮结局不可信（披露源不可信）——整只不落库、
/// 结果不可信，后台补全据此把该只记为「待重试」而非「无数据」。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct BackfillOutcome {
    pub(super) written: bool,
    pub(super) inconclusive: bool,
}

/// 单只 fund 标的的**历史回填**单元（ADR-0038 决策 6 / ADR-0122 决策 2；issue
/// #1377 起归价格历史后台补全专用——现价刷新走
/// [`super::fund_price_refresh::refresh_one_fund_price`]，「同步标的信息」不再承担首刷）：
/// **首刷判据 = 磁盘上没有任何历史序列**（issue
/// #1059）。首刷回填近两年，已有历史序列者以现价缓存的净值日期为水位增量（水位
/// 当日不重拉、只取水位次日之后）。取数一次请求拿整只历史单位净值（新浪全历史
/// 面，含已终止基金），本地裁剪到窗口（首刷近两年 / 增量水位次日；取数失败上抛
/// 交下一窗口重试，不把不可信结果当「无净值」——fail-closed，issue #1566）。
/// 净值点降采样落 PriceHistory（同周整周覆盖幂等），窗口内最新公布净值落现价缓存
///（现价 = 单位净值、priced_at = nav_date = 净值日期，与 #301 添加基金同形）。
/// 周采样与现价落库在**一只一个事务**里整只一次提交（ADR-0122 决策 8 / issue
/// #1373）：第 N 个周点写入失败或中途中断整体回滚，磁盘上不留半根历史——「有
/// 历史序列」与「历史完整」由此等价，首刷判据（ADR-0038 决策 6）依赖的正是这个
/// 等价。
///
/// **判定门前置（issue #1563 / ADR-0126 决策 3 换源）**：未打标标的抓取前先查
/// 官方披露自报形态——确认即打标收尾（单向，幂等；现价缓存保留建档一条、不落
/// 历史周点），**零净值抓取请求**；缺信号照常回填（不清空既有标记）；披露源
/// 不可信则本轮整只不落，按 `inconclusive` 待重试。已标记行的竞态窗（排队后才
/// 被并行刷新打标）沿用既有短路，不再触碰网络。
///
/// **前置条件**：`fund` 为 6 位真实代码的有通道基金行。跳过语义：首刷查无净值
///（查无此码 / 新基金未公布首期 / 已终止基金的末点在窗口外）不报错不中断，以
/// [`BackfillOutcome`] 表达结局；单只取数失败上抛（后台补全编排单只失败不中断
/// 本轮）。
pub(super) async fn backfill_one_fund_history<Q, H, C>(
    session: &Q,
    fund: &super::incremental::SyncInstrument,
    fetch_nav_history: &mut H,
    confirm_money_fund: &mut C,
) -> Result<BackfillOutcome>
where
    // 作用域会话接缝（issue #1275）：本函数读写库的唯一通道，签名层面取不到连接。
    Q: ScopedSession,
    // 新浪单只全历史通道（issue #1566）：6 位代码 → 整只历史单位净值。
    H: FnMut(&str) -> FetchFuture<Vec<NavPoint>> + Send,
    // 货基判定确认通道（issue #1563）：6 位代码 → 官方披露自报形态三态。
    C: FnMut(&str) -> FetchFuture<bool> + Send,
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

    // 判定门前置（issue #1563 / ADR-0126 决策 3 换源）：官方披露自报形态确认即
    // 打标收尾——不发起净值抓取，万份收益不得经取数面冒充单位净值落库（#1342）。
    // 确认单向幂等：已标记行零触碰、缺信号（Ok(false)）不清空、照常回填；披露
    // 源不可信（Err）则本轮整只不落，按不可信结局待重试。已标记行（竞态窗：
    // 排队后才被并行刷新打标）在下方短路，不重复确认。
    if fund.channel != PriceChannel::Constant {
        match confirm_money_fund(&fund.symbol).await {
            Ok(true) => {
                let written = mark_constant_price_on_confirm(
                    session,
                    &fund.instrument_id,
                    &fund.currency,
                    today.format("%Y-%m-%d").to_string(),
                )
                .await?;
                return Ok(BackfillOutcome {
                    written,
                    inconclusive: false,
                });
            }
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(
                    code = %fund.symbol, %error,
                    "官方披露判定源不可信，本轮整只不落（信号缺席时落取值位即万份收益冒充净值）"
                );
                return Ok(BackfillOutcome {
                    written: false,
                    inconclusive: true,
                });
            }
        }
    }

    // 恒定价格标的（ADR-0126 决策 3/5/6）：队列判据已在收集侧排除恒定标的，能到
    // 这里的恒定通道行只剩「排队后才被并行刷新打标」的竞态窗——打标收尾，零网络。
    if fund.channel == PriceChannel::Constant {
        let written = mark_constant_price_on_confirm(
            session,
            &fund.instrument_id,
            &fund.currency,
            today.format("%Y-%m-%d").to_string(),
        )
        .await?;
        return Ok(BackfillOutcome {
            written,
            inconclusive: false,
        });
    }

    // 单只全历史通道（issue #1566 / ADR-0130 决策 2）：一次请求拿整只基金的
    // 历史单位净值（含已终止基金），本地裁剪到窗口。失败语义 fail-closed：取数
    // 失败（网络 / 报文不可信 / 窗口截断）一律上抛，下一窗口按派生事实重试，
    // 不把不可信结果当「无净值」——半根历史冒充完整数据比慢更糟（ADR-0122
    // 决策 8 的「整只不落」由「要么全序列可信、要么零落库」承接）。
    let points: Vec<NavPoint> = fetch_nav_history(&fund.symbol).await?;
    let collected: Vec<NavPoint> = trim_to_window(points, &start, &end);

    if collected.is_empty() {
        // 首刷查无净值（查无此码 / 新基金未公布首期 / 已终止基金末点在窗口外）
        // 与增量窗口内确实无新净值（T+1 正常空窗）：结局确定——无可采数据 /
        // 已是最新。（全历史面的可信空与报错在取数层已区分开，issue #1566。）
        return Ok(BackfillOutcome {
            written: false,
            inconclusive: false,
        });
    }

    // 现价 = 窗口内最新公布单位净值；priced_at = nav_date = 净值日期
    // （与 #301 添加基金同形；nav_date 兼任下次同步的水位）。
    // let-else 显式防线（#434，ADR-0060 A 类临时豁免已摘）：points 非空由
    // 前文判空保证，此臂理论不可达；一旦前置防线被移除，此处记警告并跳过
    // 该只、不中断同步。
    let Some(latest) = collected.iter().max_by_key(|p| p.date.as_str()) else {
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
    let bars: Vec<KlineBar> = collected
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
                    SINA_PRICE_SOURCE,
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
                        source: Some(SINA_PRICE_SOURCE),
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
