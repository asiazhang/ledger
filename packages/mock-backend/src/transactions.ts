import type {
  CreateTransactionResult,
  OrderAccountContribution,
  Transaction,
  TransactionInput,
  TransactionOrderSummary,
  TransactionSearchResult,
} from "@ledger/types";
import { insertTransaction } from "./ledger";
import { argsOf, db, emitLedgerChanged, obj, str } from "./state";

/**
 * 交易读扩展替身（issue #1968 / spec #1964 / ADR-0143）：搜索、批量创建与订单
 * 汇总。写入复用 ledger.ts 的 insertTransaction 内核（含商户即建），信号由本域
 * 命令收口。从简口径（不追行为等价，与真后端的差异点，发现误导 UI 开发时以真
 * 后端为准修替身）：
 *
 * - **搜索匹配面只做备注 / 商户名**（词条间 AND、字段间 OR、大小写不敏感子串）：
 *   真后端另含转出账户名与购买项名称命中面，替身不模拟；软删商户名仍可搜
 *   （历史引用口径）、软删交易恒不可搜。
 * - **金额筛选取 amount_native_cents**（替身即原币直记，不折算）。
 * - **排序 date DESC + created_at DESC + id DESC**（同秒批量写入翻页不漂移，
 *   与真后端同形）；分页 page ≥ 1、pageSize 鉗 1..200。
 * - **批量创建不去重**（IPC 路径 dedup=false，逐条直落）、ledger:changed 整批
 *   只发一次；逐条结果恒 success（单行校验失败不影响他行的真后端语义，
 *   替身校验只做存在性必需，失败直接整批报错）。
 * - **订单汇总是只读投影**：行集/合计/出资构成（分解行逐出资项、单出资行按主
 *   账户整额、同账户合并）、币种唯一才携带；空单零行汇总不报错。
 */

/** 搜索每页条数上限（防呆，与真后端同值）。 */
const MAX_PAGE_SIZE = 200;

/** 可选字符串 / 数字参数窄化（扁平 camelCase 载荷的四键 + page/pageSize）。 */
function optStr(args: Record<string, unknown>, key: string): string | null {
  const v = args[key];
  return typeof v === "string" ? v : null;
}

function optNum(args: Record<string, unknown>, key: string): number | null {
  const v = args[key];
  return typeof v === "number" ? v : null;
}

export function searchTransactions(args: Record<string, unknown>): TransactionSearchResult {
  const a = argsOf(args);
  const terms = (typeof a.query === "string" ? a.query : "")
    .trim()
    .split(/\s+/)
    .filter(Boolean)
    .map((term) => term.toLowerCase());
  const min = optNum(a, "amountMinCents");
  const max = optNum(a, "amountMaxCents");
  const from = optStr(a, "dateFrom");
  const to = optStr(a, "dateTo");
  // 空关键字 + 无筛选维持空结果（既有语义）；空关键字 + 有筛选走仅筛选查询
  if (terms.length === 0 && min === null && max === null && from === null && to === null) {
    return { items: [], total: 0 };
  }

  // 商户名字典（含软删——历史交易仍可搜，与真后端 LEFT JOIN 同形）
  const merchantNames = new Map(db().merchants.map((m) => [m.id, m.name.toLowerCase()]));
  const rows = db()
    .transactions.filter((t) => {
      if (t.is_deleted) return false;
      if (min !== null && t.amount_native_cents < min) return false;
      if (max !== null && t.amount_native_cents > max) return false;
      if (from !== null && t.date < from) return false;
      if (to !== null && t.date > to) return false;
      for (const term of terms) {
        const inNote = t.note !== null && t.note.toLowerCase().includes(term);
        const inMerchant =
          t.merchant_id !== null && (merchantNames.get(t.merchant_id) ?? "").includes(term);
        if (!inNote && !inMerchant) return false; // 词条间 AND、字段间 OR
      }
      return true;
    })
    .sort(
      (x, y) =>
        y.date.localeCompare(x.date) ||
        y.created_at.localeCompare(x.created_at) ||
        y.id.localeCompare(x.id),
    );

  const page = Math.max(1, optNum(a, "page") ?? 1);
  const pageSize = Math.min(MAX_PAGE_SIZE, Math.max(1, optNum(a, "pageSize") ?? 20));
  const start = (page - 1) * pageSize;
  return { items: rows.slice(start, start + pageSize), total: rows.length };
}

export function createTransactions(args: Record<string, unknown>): CreateTransactionResult[] {
  const inputs = obj<TransactionInput[]>(args, "inputs");
  if (!Array.isArray(inputs)) throw new Error("替身后端：命令参数 inputs 缺失");
  const results = inputs.map((input) => {
    const id = insertTransaction(input); // 逐笔记内存（含商户即建）
    return { success: true, duplicate: false, id, error: null };
  });
  emitLedgerChanged(); // 整批只发一次（票面契约）
  return results;
}

export function getTransactionOrderSummary(args: Record<string, unknown>): TransactionOrderSummary {
  // 载荷键 camelCase（sourceOrderNo）：Tauri IPC 按 Rust 参数名 lowerCamelCase 绑定
  const sourceOrderNo = str(args, "sourceOrderNo");
  const items: Transaction[] = db()
    .transactions.filter((t) => !t.is_deleted && t.source_order_no === sourceOrderNo)
    .sort(
      (x, y) =>
        x.date.localeCompare(y.date) ||
        x.created_at.localeCompare(y.created_at) ||
        x.id.localeCompare(y.id),
    );

  const row_count = items.length;
  const total_amount_cents = items.reduce((sum, t) => sum + t.amount_cents, 0);
  // 币种唯一才携带：单币种订单常态；混合币种 null，不静默混算
  const currency_code =
    row_count === 0 || items.some((t) => t.currency_code !== items[0]?.currency_code)
      ? null
      : (items[0]?.currency_code ?? null);

  // 出资构成按账户聚合：分解行逐出资项、单出资行按主账户整额，同账户合并、序随行序
  const accounts: OrderAccountContribution[] = [];
  for (const t of items) {
    const contributions =
      t.fundings.length > 0
        ? t.fundings.map((f) => ({ account_id: f.account_id, amount_cents: f.amount_cents }))
        : t.account_id !== null
          ? [{ account_id: t.account_id, amount_cents: t.amount_cents }]
          : [];
    for (const c of contributions) {
      const hit = accounts.find((x) => x.account_id === c.account_id);
      if (hit) hit.amount_cents += c.amount_cents;
      else accounts.push({ ...c });
    }
  }

  return {
    source_order_no: sourceOrderNo,
    row_count,
    total_amount_cents,
    currency_code,
    accounts,
    items,
  };
}
