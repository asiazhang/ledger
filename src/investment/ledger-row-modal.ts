import type { InvestmentTransactionRow, TransactionModalRow } from "@ledger/types";
import { instrumentDisplayLabel } from "@/investment/instrument-display-label";

/**
 * 投资明细行 → 交易弹窗族行投影（ADR-0135 决策 4「操作同权」/ issue #1781）：
 * 明细页签复用交易弹窗族（编辑 / 只读详情）的行适配投影——弹窗编排
 * （TransactionModalState）与其消费组件读取的字段闭集是 TransactionModalRow，
 * 明细行的弹窗族消费列（note / currency_code / amount_native_cents，后端投影
 * 尾段注记）之外的字段由本适配单点补齐，消费组件不经手、不感知明细行形状。
 *
 * 三个补齐字段的事实来源（均为既有口径，不另立事实）：
 * - 关联字段恒 null：投资 kind 不携带分类 / 商户 / 保单 / 退款关联 / 转入账户
 *   （写入行为层准入闭集），convert 的两腿展示读 intent 的扩展明细载荷而非行上
 *   `convert` 扩展（主列表 ConvertFields 契约冻结，明细行不带该形状）；
 * - source：与主列表来源列「标的目标反查」同口径（read/source.rs ④：代码 +
 *   名称空格连接，无名称退化裸代码），明细行公共标的地即反查结果原料
 *   （symbol / instrument_name 由同一条 JOIN 带出）。
 *
 * 编辑（buy/sell）与只读详情（convert/split/dividend）共走本适配：编辑提交走
 * update_transaction 全字段替换，关联字段 null 即原值（投资 kind 本就不可携带）。
 */
export function ledgerRowToModalRow(row: InvestmentTransactionRow): TransactionModalRow {
  return {
    id: row.id,
    kind: row.kind,
    date: row.date,
    note: row.note,
    account_id: row.account_id,
    to_account_id: null,
    funding_account_id: row.funding_account_id,
    category_id: null,
    merchant_id: null,
    policy_id: null,
    refund_of_transaction_id: null,
    amount_cents: row.amount_cents,
    currency_code: row.currency_code,
    amount_native_cents: row.amount_native_cents,
    source: {
      kind: "instrument",
      entity_id: row.instrument_id,
      display_name: instrumentDisplayLabel(row.symbol, row.instrument_name),
      status: null,
    },
    convert: null,
  };
}
