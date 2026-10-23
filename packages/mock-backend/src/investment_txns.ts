import type {
  InvestmentConvertFields,
  InvestmentTransactionListFilter,
  InvestmentTransactionListResult,
  InvestmentTransactionRow,
  InvestmentTradeFields,
  InvestmentSplitFields,
  Transaction,
  TransactionConvert,
  TransactionKind,
  TransactionSplit,
  TransactionTrade,
} from "@ledger/types";
import { argsOf, db, findActive, obj, str } from "./state";
import type { MockSecurityTxn } from "./state";

/**
 * 投资明细与交易扩展投影域替身（issue #1970 / spec #1964 / ADR-0143）：投资页
 * 「明细」页签的读投影（五种投资 kind 交易行 JOIN 证券扩展行）与编辑回填三
 * 投影。纯读命令，不发失效信号。刻意不追行为等价（决策 1）：
 *
 * - 扩展行数据面 = 夹具 `fixtureSecurityTxns` 直投影（真后端由投资写路径落
 *   `security_transactions`；替身投资写入面未实现，无第二数据源）；
 * - convert 的结转成本（行金额锚点）为夹具字面量，不追 FIFO 消耗；
 * - JOIN 展示字段（标的名/代码、类型）按标的字典现行取值，软删标的照常带出。
 */

/** 行集闭包（ADR-0135 决策 3）：五种投资 kind 整体构成明细行集。 */
const INVESTMENT_KINDS: ReadonlySet<TransactionKind> = new Set([
  "buy",
  "sell",
  "convert",
  "split",
  "dividend",
]);

/** 按交易 id 取证券扩展行（`transaction_id` 主键一对一，镜像真后端 JOIN）。 */
function securityTxnOf(transactionId: string): MockSecurityTxn | undefined {
  return db().securityTransactions.find((s) => s.transaction_id === transactionId);
}

/** 按 id 取标的行（JOIN instruments；夹具引用完整性保证存在）。 */
function instrumentOf(id: string) {
  return db().instruments.find((i) => i.id === id);
}

/** 行投影装配：公共字段恒在场，kind 专属载荷按形态携带（对齐真后端 FromRow 分派）。 */
function projectRow(t: Transaction, st: MockSecurityTxn): InvestmentTransactionRow {
  const instrument = instrumentOf(st.instrument_id);
  const symbol = instrument?.symbol ?? "";
  const instrumentName = instrument?.name ?? null;
  const instrumentType = instrument?.type ?? "other";

  let trade: InvestmentTradeFields | null = null;
  let convert: InvestmentConvertFields | null = null;
  let split: InvestmentSplitFields | null = null;
  if (t.kind === "buy" || t.kind === "sell") {
    trade = {
      quantity: st.quantity ?? 0,
      price_cents: st.price_cents ?? 0,
      fee_cents: st.fee_cents,
    };
  } else if (t.kind === "convert" && st.to_instrument_id) {
    convert = {
      quantity: st.quantity ?? 0,
      to_instrument_id: st.to_instrument_id,
      to_symbol: instrumentOf(st.to_instrument_id)?.symbol ?? "",
      to_quantity: st.to_quantity ?? 0,
      out_amount_cents: st.out_amount_cents ?? 0,
      in_amount_cents: st.in_amount_cents ?? 0,
    };
  } else if (t.kind === "split") {
    split = { delta_quantity: st.quantity ?? 0 };
  }

  return {
    id: t.id,
    date: t.date,
    kind: t.kind,
    amount_cents: t.amount_cents,
    account_id: t.account_id ?? "",
    funding_account_id: t.funding_account_id,
    instrument_id: st.instrument_id,
    symbol,
    instrument_name: instrumentName,
    instrument_type: instrumentType,
    trade,
    convert,
    split,
    note: t.note,
    currency_code: t.currency_code,
    amount_native_cents: t.amount_native_cents,
    source_order_no: t.source_order_no,
  };
}

// —— list_investment_transactions：行集闭包 + 四维筛选 + date 倒序 + offset 分页 ——

export function listInvestmentTransactions(
  args: Record<string, unknown>,
): InvestmentTransactionListResult {
  const filter = (obj<InvestmentTransactionListFilter | null>(argsOf(args), "filter") ??
    {}) as Partial<InvestmentTransactionListFilter>;

  // 行集闭包 = 五种投资 kind 且有证券扩展行（JOIN 一对一），通用 kind 不入集
  let rows = db()
    .transactions.filter((t) => !t.is_deleted && INVESTMENT_KINDS.has(t.kind))
    .map((t) => ({ t, st: securityTxnOf(t.id) }))
    .filter((pair): pair is { t: Transaction; st: MockSecurityTxn } => pair.st !== undefined);

  // 涉及账户语义（ADR-0135 决策 3）：账户端 ∪ 出资端；dividend 的到账账户即账户端
  if (filter.account_id) {
    const id = filter.account_id;
    rows = rows.filter(({ t }) => t.account_id === id || t.funding_account_id === id);
  }
  // 标的筛选认 convert 两腿任一命中（转出腿 instrument_id / 转入腿 to_instrument_id）
  if (filter.instrument_id) {
    const id = filter.instrument_id;
    rows = rows.filter(({ st }) => st.instrument_id === id || st.to_instrument_id === id);
  }
  // 类型子集多选：维度内取或；空集合视为未携带
  if (filter.kinds && filter.kinds.length > 0) {
    const kinds = new Set<TransactionKind>(filter.kinds);
    rows = rows.filter(({ t }) => kinds.has(t.kind));
  }
  if (filter.from) rows = rows.filter(({ t }) => t.date >= filter.from!);
  if (filter.to) rows = rows.filter(({ t }) => t.date <= filter.to!);

  // 排序与主列表同构：date 倒序 + created_at 倒序（ADR-0135）
  rows = [...rows].sort(
    (a, b) => b.t.date.localeCompare(a.t.date) || b.t.created_at.localeCompare(a.t.created_at),
  );

  const total = rows.length;
  if (filter.page_size != null) {
    const page = filter.page ?? 1;
    const start = (page - 1) * filter.page_size;
    return {
      items: rows.slice(start, start + filter.page_size).map((p) => projectRow(p.t, p.st)),
      total,
    };
  }
  return { items: rows.map((p) => projectRow(p.t, p.st)), total };
}

// —— 编辑回填三投影：非对应 kind 答错（存在性校验形态，真后端 NotFound 同语义） ——

/** 取未删除交易 + 对应 action 的扩展行；任一缺失按投影语义报错
 *（buy/sell 皆可回填买卖投影，action 传集合）。 */
function requireProjection(
  id: string,
  actions: ReadonlySet<MockSecurityTxn["action"]>,
  label: string,
): { t: Transaction; st: MockSecurityTxn } {
  const t = findActive(db().transactions, id, "交易");
  const st = securityTxnOf(id);
  if (!st || !actions.has(st.action)) {
    throw new Error(`替身后端：交易不存在或无${label}明细 ${id}`);
  }
  return { t, st };
}

export function getTransactionTrade(args: Record<string, unknown>): TransactionTrade {
  const id = str(args, "id");
  const { st } = requireProjection(id, new Set(["buy", "sell"]), "买卖");
  const instrument = instrumentOf(st.instrument_id);
  return {
    instrument_id: st.instrument_id,
    symbol: instrument?.symbol ?? "",
    instrument_name: instrument?.name ?? null,
    instrument_type: instrument?.type ?? "other",
    quantity: st.quantity ?? 0,
    price_cents: st.price_cents ?? 0,
    fee_cents: st.fee_cents,
  };
}

export function getTransactionConvert(args: Record<string, unknown>): TransactionConvert {
  const id = str(args, "id");
  const { t, st } = requireProjection(id, new Set(["convert"]), "转换");
  const out = instrumentOf(st.instrument_id);
  const inInstrument = st.to_instrument_id ? instrumentOf(st.to_instrument_id) : undefined;
  return {
    out_instrument_id: st.instrument_id,
    out_symbol: out?.symbol ?? "",
    out_instrument_name: out?.name ?? null,
    out_quantity: st.quantity ?? 0,
    out_amount_cents: st.out_amount_cents ?? 0,
    in_instrument_id: st.to_instrument_id ?? "",
    in_symbol: inInstrument?.symbol ?? "",
    in_instrument_name: inInstrument?.name ?? null,
    in_quantity: st.to_quantity ?? 0,
    in_amount_cents: st.in_amount_cents ?? 0,
    fee_cents: st.fee_cents,
    carried_cost_cents: t.amount_cents, // 行金额锚点 = 结转成本（真后端 JOIN transactions 带出）
    currency_code: t.currency_code,
  };
}

export function getTransactionSplit(args: Record<string, unknown>): TransactionSplit {
  const id = str(args, "id");
  const { st } = requireProjection(id, new Set(["split"]), "份额调整");
  const instrument = instrumentOf(st.instrument_id);
  return {
    instrument_id: st.instrument_id,
    symbol: instrument?.symbol ?? "",
    instrument_name: instrument?.name ?? null,
    quantity: st.quantity ?? 0, // 带符号增量 Δ 原样投影
  };
}
