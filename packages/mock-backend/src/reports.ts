import type {
  CategoryShare,
  MerchantSharesReport,
  MonthlySummary,
  ReportDateRange,
  Transaction,
} from "@ledger/types";
import { argsOf, db, str } from "./state";

/**
 * 报表域替身（issue #1968 / spec #1964 / ADR-0143）：报表页四条聚合命令按交易
 * 夹具直聚合，「报表页在浏览器可出图出表」成立。从简口径（不追行为等价，与
 * 真后端的差异点，发现误导 UI 开发时以真后端为准修替身）：
 *
 * - **不折算（ADR-0143 决策 1）**：聚合一律取 `amount_native_cents`，替身里该列
 *   即原币直记（夹具单币种 CNY），多币种混合直加不作本位币折算。
 * - **金额口径与真后端同源**（kind→度量矩阵，ledger_transaction::amount）：
 *   月度汇总毛值三列 income = 收入+分红、expense = 毛支出、refund = 退款毛额；
 *   分类份额净值 expense = 毛支出−退款、income = 收入+分红；商户排行
 *   expense_net = 毛支出−退款 + transaction_count 同口径计数（支出/退款笔数）。
 * - **子类直归本类，不模拟上卷**：按 category_id 分组，父分类预算覆盖/上卷
 *   不做（与真后端 GROUP BY category_id 同形）。
 * - **期间过滤（ADR-0057）**：from/to 任一存在即期间口径（YYYY-MM-DD 含边界
 *   闭区间、字典序比较、各边独立），遗留参数不参与；双端皆缺省回退遗留参数
 *   口径（year 按日期年前缀、month 按 YYYY-MM 前缀；遗留形态前端已不再使用，
 *   缺失时从简为全时段）。
 * - 软删商户历史引用照常统计（商户名字典不滤 is_deleted，与真后端 JOIN 同形）；
 *   排序 net DESC、同值按名字典序稳定。
 *
 * 全部为只读命令，不发 `ledger:changed`。
 */

/** 在用交易（聚合面统一软删过滤）。 */
function activeTxns(): Transaction[] {
  return db().transactions.filter((t) => !t.is_deleted);
}

/** 可选字符串参数窄化（from/to/month 等期间键，缺失或非字符串计 null）。 */
function optStr(args: Record<string, unknown>, key: string): string | null {
  const v = args[key];
  return typeof v === "string" ? v : null;
}

/** 可选数字参数窄化（遗留 year / topN，缺失或非数字计 null）。 */
function optNum(args: Record<string, unknown>, key: string): number | null {
  const v = args[key];
  return typeof v === "number" ? v : null;
}

/** 期间过滤谓词：from/to 任一存在即期间口径（闭区间）；双缺省返回 null（回退遗留口径）。 */
function periodPredicate(
  from: string | null,
  to: string | null,
): ((date: string) => boolean) | null {
  if (from === null && to === null) return null;
  return (date: string) => (from === null || date >= from) && (to === null || date <= to);
}

/** 谓词合成：期间优先，遗留 month（YYYY-MM 前缀）+ year（日期年前缀）兜底。 */
function txnFilter(
  period: ((date: string) => boolean) | null,
  legacyMonth: string | null,
  legacyYear: number | null,
): (t: Transaction) => boolean {
  return (t) => {
    if (period) return period(t.date);
    if (legacyMonth !== null && !t.date.startsWith(legacyMonth)) return false;
    if (legacyYear !== null && !t.date.startsWith(String(legacyYear))) return false;
    return true;
  };
}

// —— 金额口径（kind→度量，与真后端矩阵同源；不折算，直记原币） ——

function incomeNetCents(t: Transaction): number {
  return t.kind === "income" || t.kind === "dividend" ? t.amount_native_cents : 0;
}

function expenseGrossCents(t: Transaction): number {
  return t.kind === "expense" ? t.amount_native_cents : 0;
}

function refundGrossCents(t: Transaction): number {
  return t.kind === "refund" ? t.amount_native_cents : 0;
}

function expenseNetCents(t: Transaction): number {
  if (t.kind === "expense") return t.amount_native_cents;
  if (t.kind === "refund") return -t.amount_native_cents;
  return 0;
}

/** 报表日期极值：未删交易日期极值对，空库双 null。 */
export function reportDateRange(): ReportDateRange {
  const dates = activeTxns().map((t) => t.date);
  if (dates.length === 0) return { min_date: null, max_date: null };
  const sorted = [...dates].sort();
  return { min_date: sorted[0] ?? null, max_date: sorted[sorted.length - 1] ?? null };
}

/** 月度汇总（毛值三列）：按月分组，「期间内按月分布」，期间无流水的月份不成行。 */
export function monthlySummary(args: Record<string, unknown>): MonthlySummary[] {
  const a = argsOf(args);
  const predicate = txnFilter(
    periodPredicate(optStr(a, "from"), optStr(a, "to")),
    null, // 月度汇总无遗留 month 参数
    optNum(a, "year"),
  );
  const byMonth = new Map<string, MonthlySummary>();
  for (const t of activeTxns()) {
    if (!predicate(t)) continue;
    const month = t.date.slice(0, 7);
    const row = byMonth.get(month) ?? { month, income_cents: 0, expense_cents: 0, refund_cents: 0 };
    row.income_cents += incomeNetCents(t);
    row.expense_cents += expenseGrossCents(t);
    row.refund_cents += refundGrossCents(t);
    byMonth.set(month, row);
  }
  return [...byMonth.values()].sort((x, y) => x.month.localeCompare(y.month));
}

/** 分类份额（净值）：expense 退款冲减、income 含分红；未分类聚合到 null + 「未分类」。 */
export function categoryShares(args: Record<string, unknown>): CategoryShare[] {
  const kind = str(args, "kind");
  const a = argsOf(args);
  const predicate = txnFilter(
    periodPredicate(optStr(a, "from"), optStr(a, "to")),
    optStr(a, "month"),
    optNum(a, "year"),
  );
  const isExpense = kind === "expense";
  const byCategory = new Map<string | null, number>();
  for (const t of activeTxns()) {
    if (!predicate(t)) continue;
    const cents = isExpense ? expenseNetCents(t) : incomeNetCents(t);
    if (cents === 0) continue;
    byCategory.set(t.category_id, (byCategory.get(t.category_id) ?? 0) + cents);
  }
  const categories = db().categories;
  return [...byCategory.entries()]
    .map(([categoryId, amount_cents]) => ({
      // 未分类组 category_id 投影空串（与真后端 FromRow 的 unwrap_or_default 同形）
      category_id: categoryId ?? "",
      // 分类名取字典行现名（软删分类的历史引用照常显示，与真后端 JOIN 不滤删同形）
      category_name: categories.find((c) => c.id === categoryId)?.name ?? "未分类",
      amount_cents,
    }))
    .sort((x, y) => y.amount_cents - x.amount_cents || x.category_id.localeCompare(y.category_id));
}

/** 商户消费排行：expense_net 按商户聚合 + transaction_count 同口径计数 + 全量合计。 */
export function merchantShares(args: Record<string, unknown>): MerchantSharesReport {
  const a = argsOf(args);
  const predicate = txnFilter(
    periodPredicate(optStr(a, "from"), optStr(a, "to")),
    null, // 商户排行无遗留 month 参数
    optNum(a, "year"),
  );
  // 参与排行口径（expense_net 矩阵非零 kind）：支出 + 退款；无商户交易不进排行
  const byMerchant = new Map<string, { net: number; count: number }>();
  for (const t of activeTxns()) {
    if (t.merchant_id === null || !predicate(t)) continue;
    const cents = expenseNetCents(t);
    if (cents === 0) continue;
    const acc = byMerchant.get(t.merchant_id) ?? { net: 0, count: 0 };
    acc.net += cents;
    acc.count += 1; // 退款笔数计入（金额冲减、笔数计数）
    byMerchant.set(t.merchant_id, acc);
  }
  const merchants = db().merchants; // 软删商户照常（历史引用语义）
  const rows = [...byMerchant.entries()]
    .map(([merchant_id, { net, count }]) => ({
      merchant_id,
      merchant_name: merchants.find((m) => m.id === merchant_id)?.name ?? "",
      amount_cents: net,
      transaction_count: count,
    }))
    .sort(
      (x, y) => y.amount_cents - x.amount_cents || (x.merchant_name < y.merchant_name ? -1 : 1),
    );
  const total_cents = rows.reduce((sum, r) => sum + r.amount_cents, 0);
  // topN 键名契约：Tauri IPC 按 Rust 参数名 lowerCamelCase 绑定（topN），排序后截断
  const topN = optNum(a, "topN");
  return { rows: topN !== null ? rows.slice(0, Math.max(0, topN)) : rows, total_cents };
}
