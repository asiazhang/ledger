//! 手动报价（ManualPrice，issue #291 / ADR-0036）：无行情数据源标的的唯一价格
//! 写入通道——与东方财富同步并列的第二条价格写入通道。用户按「日期 + 价格」
//! 单点录入，一条通道两个落点：
//!
//! 1. **价格历史周采样幂等覆盖**：复用 `price_history` 既有周键（week_start
//!    生成列）与整周覆盖语义（同周后写覆盖先写，与同步同规则），经
//!    `sync::persist::upsert_price_history` 单点落库，不立第二承载；
//! 2. **现价缓存 upsert**：现价是 MarketPrice 既有定义「PriceHistory 最新一条
//!    的即时映像」——报价不早于该标的最新价格点时 upsert 现价；回填早于最新
//!    点的旧价只沉淀历史、不改变现价（该规则是既有定义的推论，非新发明）。
//!
//! 实际写入任一落点即发价格失效信号（生产者清单再添一处，ADR-0031 模式扩展；
//! 证据经 `ManualPriceResult::any_written` 归一化、「是否发」判定单点在 signals
//! 映射，ADR-0044 / issue #333）；零写入不广播。录价 UI 入口只对同步
//! 覆盖不到的标的开放（自建标的与名称充代码的基金行），判定口径与净值可拉
//! 分区同源——守卫收在 UI 侧，后端命令不设（ADR-0036 决策 1 修订）。

use chrono::NaiveDate;
use rusqlite::Connection;

use crate::commands::sync::persist::{upsert_market_price, upsert_price_history};
use crate::error::{AppError, Result};
use crate::models::{ManualPriceInput, ManualPriceResult};

/// 手动报价来源标记：价格数据来源「手动」（与字典侧 source 同词表，ADR-0036）。
pub(crate) const MANUAL_PRICE_SOURCE: &str = "manual";

/// 手动报价核心接缝：校验 → 价格历史周采样落库 → 按最新点映像规则决定现价
/// upsert。与 IPC 命令同一实现（先例 `create_instrument_manual_internal`）。
pub(crate) fn record_manual_price(
    conn: &Connection,
    input: &ManualPriceInput,
) -> Result<ManualPriceResult> {
    if input.price_cents <= 0 {
        return Err(AppError::coded(
            "instrument.price-positive",
            "价格必须大于 0",
        ));
    }
    // 日期先解析再规范化为 canonical ISO（如 2026-8-8 → 2026-08-08）：week_start
    // 生成列（date(trade_date,…)) 对非 canonical 串返回 NULL，必须保证落库形状。
    let trade_date = NaiveDate::parse_from_str(input.date.trim(), "%Y-%m-%d")
        .map_err(|_| AppError::coded("instrument.price-date-format", "日期格式须为 YYYY-MM-DD"))?
        .format("%Y-%m-%d")
        .to_string();
    // 标的必须存在；价格币种随标的字典（自建标的币种在创建时已定）。
    let (instrument_id, currency): (String, String) = conn
        .query_row(
            "SELECT id, currency_code FROM instruments WHERE id=?1",
            rusqlite::params![input.instrument_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|_| AppError::coded("instrument.not-found", "标的不存在"))?;

    // 落点前置事实：写入前的最新价格点（现价 = 最新一条历史的即时映像，
    // 写入前的最新点决定本次报价是否成为新的最新点）。ISO 日期字典序即时间序。
    let latest_before: Option<String> = conn.query_row(
        "SELECT MAX(trade_date) FROM price_history WHERE instrument_id=?1",
        rusqlite::params![instrument_id],
        |r| r.get(0),
    )?;

    // 落点一：价格历史周采样幂等覆盖（整周覆盖，同周后写覆盖先写）。
    upsert_price_history(
        conn,
        &instrument_id,
        &trade_date,
        input.price_cents,
        &currency,
        MANUAL_PRICE_SOURCE,
    )?;

    // 落点二：现价缓存 upsert——报价成为（或保持为）最新价格点时写入。
    // 无历史（首价）或报价不早于最新点 → 现价随之更新；回填旧价 → 只沉淀历史。
    let current_price_written = latest_before
        .as_ref()
        .is_none_or(|latest| trade_date.as_str() >= latest.as_str());
    if current_price_written {
        // 手动落价无净值日期语义，nav_date 覆盖为 None（与 sync::persist 同规则）。
        upsert_market_price(
            conn,
            &instrument_id,
            input.price_cents,
            &currency,
            &trade_date,
            None,
            Some(MANUAL_PRICE_SOURCE),
        )?;
    }

    Ok(ManualPriceResult {
        // 历史落点 upsert 在 Ok 路径必写一行（同值重复报价也整周覆盖 version+1），
        // 「零写入」仅在 Err 即失败路径出现，失败本就不广播——结果仍保留两落点
        // 形状：与增量同步证据同构，发射语义「任一落点实际写入即发」（经
        // `any_written` 归一化）不被实现细节预先折迭，测试可演练全部组合。
        history_written: true,
        current_price_written,
    })
}
