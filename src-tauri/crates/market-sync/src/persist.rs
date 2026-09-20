//! 行情同步持久化（issue #137）：汇率历史周采样 upsert 与 ECB 汇率落库单元
//!（issue #1543）。
//! 价格写入单点（现价缓存 / 价格历史周采样 upsert 与刻度换算）已随投资域归位
//! 迁入 [`ledger_investment::prices`]（#401 / ADR-0056），增量同步经域入口消费；
//! 标的字典应用（全量同步侧）已随 ADR-0081 决策 3 退役删除（issue #698）。

use rusqlite::Connection;
use rusqlite::params;

use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::db::{new_uuid, now_iso};
use ledger_infra::error::Result;
use ledger_investment::upsert_auto_exchange_rate;
use ledger_sync_protocol::device::device_id;

use super::ecb::FxPairWeeklySeries;

/// ECB 汇率来源标记（issue #1543 / ADR-0019 修订记录）：汇率历史与当期汇率表
/// 的自动写入共用同一来源词；人工录入行仍记 'manual'（写入协议既有词表），
/// 人工行保护以该词判定。
///
/// `allow(dead_code)`：落库单元的接线归触发票（#1544 回填 / #1545 手动入口 /
/// #1546 每日增量），接装时撤去。
#[allow(dead_code)]
pub(super) const ECB_FX_SOURCE: &str = "ecb";

/// ECB 汇率落库结果统计（issue #1543）：服务触发编排的提示拼装（覆盖区间 /
/// 条数，#1545 手动同步结果面 / #1546 每日增量日志）；库内可观察行为以两表
/// 内容为准，统计只是本次处理的记录。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FxPersistReport {
    /// 实际落库的币种对数（空序列的对不计）。
    pub pairs: usize,
    /// 处理的汇率历史周采样点总数（含同周覆盖既有行的重写）。
    pub points: usize,
    /// 周采样覆盖区间的最早采样日（"YYYY-MM-DD"）。
    pub earliest: Option<String>,
    /// 周采样覆盖区间的最晚采样日（当期汇率表 priced_at 的取值来源）。
    pub latest: Option<String>,
    /// 因人工录入保护跳过的当期汇率行数。
    pub manual_protected: usize,
}

/// ECB 汇率落库（issue #1543）：把取数层产出的周采样序列（[`FxPairWeeklySeries`]，
/// 1 base = ? quote，与两表既有方向口径一致；正反向兜底仍归查询层）写进库——
///
/// - **汇率历史**：按（币种对 × 周键）整周覆盖 upsert，幂等由
///   UNIQUE(base_code, quote_code, week_start) 保证，重复落库零重复行；
/// - **当期汇率表**：每对取序列最新一条 upsert（每对恒一行），经投资域
///   [`upsert_auto_exchange_rate`] 写入——既有行标记人工录入时不覆盖；
///
/// 全部落库在**单一事务**内完成（嵌套感知：调用方已在事务中则加入外层），
/// 任一行失败整体回滚、不留部分写入。汇率历史按**可重建缓存**对待（ADR-0019
/// 修订记录）：本单元不产同步 op（清空后重同步零损失），人工录入仍走既有域
/// 写入口并记 op。
///
/// 空序列与全空点集是无害 no-op。返回 [`FxPersistReport`] 统计。
///
/// `allow(dead_code)`：同 [`ECB_FX_SOURCE`]，接线票接装时撤去。
#[allow(dead_code)]
pub(super) fn persist_ecb_fx_series(
    conn: &Connection,
    series: &[FxPairWeeklySeries],
) -> Result<FxPersistReport> {
    ensure_transaction(conn, || {
        let mut report = FxPersistReport::default();
        for s in series {
            if s.points.is_empty() {
                continue;
            }
            report.pairs += 1;
            for (trade_date, rate) in &s.points {
                upsert_fx_rate_history(conn, &s.base, &s.quote, trade_date, *rate, ECB_FX_SOURCE)?;
                if report
                    .earliest
                    .as_ref()
                    .is_none_or(|e| trade_date.as_str() < e.as_str())
                {
                    report.earliest = Some(trade_date.clone());
                }
                if report
                    .latest
                    .as_ref()
                    .is_none_or(|l| trade_date.as_str() > l.as_str())
                {
                    report.latest = Some(trade_date.clone());
                }
            }
            report.points += s.points.len();
            // 序列按日期升序（取数层周采样契约），末点即最新一条；当期值 = 它。
            let Some((latest_date, latest_rate)) = s.points.last() else {
                continue;
            };
            let written = upsert_auto_exchange_rate(
                conn,
                &s.base,
                &s.quote,
                *latest_rate,
                latest_date,
                ECB_FX_SOURCE,
            )?;
            if !written {
                report.manual_protected += 1;
            }
        }
        Ok(report)
    })
}

/// 按 (币种对, ISO 周) 插入或覆盖一条周采样汇率历史，规则与投资域价格历史
/// 周采样 upsert（[`ledger_investment::prices::upsert_price_history`]）对齐
/// （同周整周覆盖、同期采集）。`rate` 口径与 exchange_rates 一致：1 base = ? quote。
/// `source` 为数据来源标记：东财 FX 通道 'eastmoney'（#1551 退役前）、ECB 落库
/// 单元 [`ECB_FX_SOURCE`]。
pub(super) fn upsert_fx_rate_history(
    conn: &Connection,
    base_code: &str,
    quote_code: &str,
    trade_date: &str,
    rate: f64,
    source: &str,
) -> Result<()> {
    let now = now_iso();
    conn.execute(
        "INSERT INTO fx_rate_history (id,base_code,quote_code,trade_date,rate,source,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?7,1,?8) \
         ON CONFLICT(base_code, quote_code, week_start) DO UPDATE SET \
         trade_date=excluded.trade_date, rate=excluded.rate, source=excluded.source, \
         updated_at=excluded.updated_at, version=version+1",
        params![new_uuid(), base_code, quote_code, trade_date, rate, source, now, device_id(conn)?],
    )?;
    Ok(())
}
