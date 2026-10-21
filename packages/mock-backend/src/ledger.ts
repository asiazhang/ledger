import type {
  Account,
  AccountBalance,
  AccountInput,
  AccountUpdateInput,
  Category,
  CategoryInput,
  CategoryUpdateInput,
  Currency,
  Insurer,
  InsurerInput,
  InsurerUpdateInput,
  Merchant,
  MerchantInput,
  MerchantUpdateInput,
  ReorderItem,
  Transaction,
  TransactionFunding,
  TransactionInput,
  TransactionKind,
  TransactionListFilter,
  TransactionListResult,
  TransactionPurchase,
  UpdateTransactionInput,
} from "@ledger/types";
import {
  fixtureAccounts,
  fixtureCategories,
  fixtureCurrencies,
  fixtureInsurers,
  fixtureMerchants,
  fixtureTransactions,
} from "./fixtures";
import { emitMockEvent } from "./tauri/event";

/**
 * 替身后端的内存状态与首批主路径命令实现（issue #1962 / ADR-0143 决策 2/3/6）：
 * 写入真的改内存、读真的投影内存，「记一笔账→列表出现」成立；已实现的写路径
 * 完成后统一发射 `ledger:changed`（无 payload，ADR-0012 失效信号契约——事件驱动
 * 失效是参考数据 store 的唯一重拉触发源，不发信号写后刷新即断）。
 *
 * 刻意不追求与 Rust 后端行为等价（ADR-0143 决策 1）：金额折算细节、余额缓存
 * 维护、复杂读口径、码化错误体系都不模拟——余额 = 期初 + 流水直和，唯一余额
 * 口径即答案；校验只做存在性必需（找不到目标行报错），其余从简。
 */

/** 夹具种子快照形态（五字典 + 交易 + id 计数器） */
interface MockLedger {
  currencies: Currency[];
  accounts: Account[];
  categories: Category[];
  merchants: Merchant[];
  insurers: Insurer[];
  transactions: Transaction[];
  counters: Map<string, number>;
}

const DEVICE_ID = "mock";

let ledger: MockLedger = seed();

/** 重新播种：状态整体回到夹具种子（安装替身与页面刷新同语义，不持久）。 */
export function reseed(): void {
  ledger = seed();
}

function seed(): MockLedger {
  return {
    currencies: structuredClone(fixtureCurrencies),
    accounts: structuredClone(fixtureAccounts),
    categories: structuredClone(fixtureCategories),
    merchants: structuredClone(fixtureMerchants),
    insurers: structuredClone(fixtureInsurers),
    transactions: structuredClone(fixtureTransactions),
    counters: new Map(),
  };
}

/** 会话内递增 id（前缀按实体区分）；刷新重播种后从种子值重新起算。 */
function nextId(prefix: string): string {
  const n = (ledger.counters.get(prefix) ?? 0) + 1;
  ledger.counters.set(prefix, n);
  return `${prefix}-${n}`;
}

function nowIso(): string {
  return new Date().toISOString();
}

/** 触碰同步字段（写入侧统一口径：updated_at 前进、version 自增）。 */
function touch(row: { updated_at: string; version: number }): void {
  row.updated_at = nowIso();
  row.version += 1;
}

/** 参考/交易写入的统一失效信号出口（无 payload，ADR-0012）。 */
function emitLedgerChanged(): void {
  emitMockEvent("ledger:changed");
}

// —— 参数窄化助手：invoke args 是 Record<string, unknown>，命令实现按契约取用 ——

function argsOf(args: Record<string, unknown> | undefined): Record<string, unknown> {
  return args ?? {};
}

function str(args: Record<string, unknown>, key: string): string {
  const v = args[key];
  if (typeof v !== "string" || v === "") throw new Error(`替身后端：命令参数 ${key} 缺失`);
  return v;
}

function bool(args: Record<string, unknown>, key: string): boolean {
  return args[key] === true;
}

/** 按声明字段集捡拾对象参数（input / filter / items 等嵌套对象直通泛型）。 */
function obj<T>(args: Record<string, unknown>, key: string): T {
  return args[key] as T;
}

// —— 启动门（ADR-0143 决策 6）：替身答「已解锁」直接进主界面 ——

export function getBootStatus(): { phase: "ready"; error_code: null } {
  return { phase: "ready", error_code: null };
}

// —— 参考数据：读（含删全量按 includeDeleted 过滤） ——

export function listCurrencies(): Currency[] {
  return [...ledger.currencies];
}

export function listAccounts(): Account[] {
  return ledger.accounts.filter((a) => !a.is_deleted);
}

function listSoftDeletable<T extends { is_deleted: boolean }>(
  rows: T[],
  includeDeleted: boolean,
): T[] {
  return rows.filter((r) => includeDeleted || !r.is_deleted);
}

export function listCategories(args?: Record<string, unknown>): Category[] {
  return listSoftDeletable(ledger.categories, bool(argsOf(args), "includeDeleted"));
}

export function listMerchants(args?: Record<string, unknown>): Merchant[] {
  return listSoftDeletable(ledger.merchants, bool(argsOf(args), "includeDeleted"));
}

export function listInsurers(args?: Record<string, unknown>): Insurer[] {
  return listSoftDeletable(ledger.insurers, bool(argsOf(args), "includeDeleted"));
}

// —— 参考数据：写（软删语义与后端一致，写入后发 ledger:changed） ——

export function createCategory(args: Record<string, unknown>): string {
  const input = obj<CategoryInput>(args, "input");
  const row: Category = {
    id: nextId("mock-cat"),
    name: input.name,
    kind: input.kind,
    parent_id: input.parent_id ?? null,
    icon: input.icon ?? null,
    sort_order: ledger.categories.length,
    created_at: nowIso(),
    updated_at: nowIso(),
    version: 1,
    device_id: DEVICE_ID,
    is_deleted: false,
  };
  ledger.categories.push(row);
  emitLedgerChanged();
  return row.id;
}

export function updateCategory(args: Record<string, unknown>): void {
  const row = findActive(ledger.categories, str(args, "id"), "分类");
  const input = obj<CategoryUpdateInput>(args, "input");
  if (input.name !== undefined) row.name = input.name;
  if (input.icon !== undefined) row.icon = input.icon;
  if (input.parent_id !== undefined) row.parent_id = input.parent_id;
  touch(row);
  emitLedgerChanged();
}

export function reorderCategories(args: Record<string, unknown>): void {
  const items = obj<ReorderItem[]>(args, "items");
  for (const item of items) {
    const row = ledger.categories.find((c) => c.id === item.id);
    if (row) row.sort_order = item.sort_order;
  }
  emitLedgerChanged();
}

export function deleteCategory(args: Record<string, unknown>): void {
  softDelete(ledger.categories, str(args, "id"), "分类");
  emitLedgerChanged();
}

export function createMerchant(args: Record<string, unknown>): string {
  const input = obj<MerchantInput>(args, "input");
  const row = newMerchant(input.name);
  ledger.merchants.push(row);
  emitLedgerChanged();
  return row.id;
}

export function updateMerchant(args: Record<string, unknown>): void {
  const row = findActive(ledger.merchants, str(args, "id"), "商户");
  const input = obj<MerchantUpdateInput>(args, "input");
  if (input.name !== undefined) row.name = input.name;
  touch(row);
  emitLedgerChanged();
}

export function deleteMerchant(args: Record<string, unknown>): void {
  softDelete(ledger.merchants, str(args, "id"), "商户");
  emitLedgerChanged();
}

export function createInsurer(args: Record<string, unknown>): string {
  const input = obj<InsurerInput>(args, "input");
  const row: Insurer = {
    id: nextId("mock-ins"),
    name: input.name,
    updated_at: nowIso(),
    version: 1,
    device_id: DEVICE_ID,
    is_deleted: false,
  };
  ledger.insurers.push(row);
  emitLedgerChanged();
  return row.id;
}

export function updateInsurer(args: Record<string, unknown>): void {
  const row = findActive(ledger.insurers, str(args, "id"), "保司");
  const input = obj<InsurerUpdateInput>(args, "input");
  if (input.name !== undefined) row.name = input.name;
  touch(row);
  emitLedgerChanged();
}

export function deleteInsurer(args: Record<string, unknown>): void {
  softDelete(ledger.insurers, str(args, "id"), "保司");
  emitLedgerChanged();
}

// —— 账户：读写（余额 = 期初 + 流水直和，唯一余额口径，不做缓存维护） ——

export function createAccount(args: Record<string, unknown>): string {
  const input = obj<AccountInput>(args, "input");
  const row: Account = {
    id: nextId("mock-acc"),
    name: input.name,
    type: input.type,
    currency_code: input.currency_code,
    initial_balance_cents: input.initial_balance_cents ?? 0,
    credit_limit_cents: input.credit_limit_cents ?? null,
    statement_day: input.statement_day ?? null,
    due_day: input.due_day ?? null,
    created_at: nowIso(),
    is_hidden: false,
    is_archived: false,
    updated_at: nowIso(),
    version: 1,
    device_id: DEVICE_ID,
    is_deleted: false,
  };
  ledger.accounts.push(row);
  emitLedgerChanged();
  return row.id;
}

export function updateAccount(args: Record<string, unknown>): void {
  const row = findActive(ledger.accounts, str(args, "id"), "账户");
  const input = obj<AccountUpdateInput>(args, "input");
  if (input.name !== undefined) row.name = input.name;
  if (input.currency_code !== undefined) row.currency_code = input.currency_code;
  if (input.credit_limit_cents !== undefined) row.credit_limit_cents = input.credit_limit_cents;
  if (input.statement_day !== undefined) row.statement_day = input.statement_day;
  if (input.due_day !== undefined) row.due_day = input.due_day;
  if (input.is_archived !== undefined && input.is_archived !== null) {
    row.is_archived = input.is_archived;
  }
  touch(row);
  emitLedgerChanged();
}

export function archiveAccount(args: Record<string, unknown>): void {
  const row = findActive(ledger.accounts, str(args, "id"), "账户");
  row.is_archived = true;
  touch(row);
  emitLedgerChanged();
}

export function unarchiveAccount(args: Record<string, unknown>): void {
  const row = findActive(ledger.accounts, str(args, "id"), "账户");
  row.is_archived = false;
  touch(row);
  emitLedgerChanged();
}

export function deleteAccount(args: Record<string, unknown>): void {
  softDelete(ledger.accounts, str(args, "id"), "账户");
  emitLedgerChanged();
}

export function listAccountBalances(): AccountBalance[] {
  return ledger.accounts
    .filter((a) => !a.is_deleted)
    .map((account) => ({ account, balance_cents: balanceCents(account.id) }));
}

/** 余额 = 期初 + Σ 流水（账户币种直和；折算/缓存维护不模拟，ADR-0143 决策 1）。 */
function balanceCents(accountId: string): number {
  const account = ledger.accounts.find((a) => a.id === accountId);
  let sum = account?.initial_balance_cents ?? 0;
  for (const t of ledger.transactions) {
    if (t.is_deleted) continue;
    switch (t.kind) {
      case "expense":
        if (t.account_id === accountId) sum -= t.amount_cents;
        break;
      case "income":
        if (t.account_id === accountId) sum += t.amount_cents;
        break;
      case "transfer":
        if (t.account_id === accountId) sum -= t.amount_cents;
        if (t.to_account_id === accountId) sum += t.amount_cents;
        break;
      case "refund":
        if (t.account_id === accountId) sum += t.amount_cents;
        break;
      case "buy":
        if ((t.funding_account_id ?? t.account_id) === accountId) sum -= t.amount_cents;
        break;
      case "sell":
      case "dividend":
        if ((t.funding_account_id ?? t.account_id) === accountId) sum += t.amount_cents;
        break;
      case "convert":
      case "split":
        break; // 无现金腿
    }
  }
  return sum;
}

// —— 交易：读写闭环 ——

export function listTransactions(args?: Record<string, unknown>): TransactionListResult {
  const filter = obj<TransactionListFilter | null>(argsOf(args), "filter") ?? {};
  let rows = ledger.transactions.filter((t) => !t.is_deleted);

  if (filter.from) rows = rows.filter((t) => t.date >= filter.from!);
  if (filter.to) rows = rows.filter((t) => t.date <= filter.to!);
  if (filter.account_id) rows = rows.filter((t) => t.account_id === filter.account_id);
  if (filter.involving_account_id) {
    const id = filter.involving_account_id;
    rows = rows.filter(
      (t) => t.account_id === id || t.to_account_id === id || t.funding_account_id === id,
    );
  }
  if (filter.merchant_id) rows = rows.filter((t) => t.merchant_id === filter.merchant_id);
  if (filter.category_id) rows = rows.filter((t) => t.category_id === filter.category_id);
  if (filter.uncategorized_only) rows = rows.filter((t) => t.category_id === null);
  if (filter.instrument_id) rows = rows.filter(() => false); // 夹具无标的行
  if (filter.kinds && filter.kinds.length > 0) {
    const kinds = new Set<TransactionKind>(filter.kinds);
    rows = rows.filter((t) => kinds.has(t.kind));
  }
  if (filter.hide_investment_related) {
    const investmentIds = new Set(
      ledger.accounts.filter((a) => a.type === "investment").map((a) => a.id),
    );
    rows = rows.filter(
      (t) =>
        !investmentIds.has(t.account_id ?? "") &&
        !investmentIds.has(t.to_account_id ?? "") &&
        !investmentIds.has(t.funding_account_id ?? ""),
    );
  }

  // 列表同构排序：date 倒序（后端契约的投影，ADR-0135）
  rows = [...rows].sort(
    (a, b) => b.date.localeCompare(a.date) || b.created_at.localeCompare(a.created_at),
  );

  const total = rows.length;
  if (filter.limit != null) {
    return { items: rows.slice(0, filter.limit), total };
  }
  const page = filter.page ?? 1;
  if (filter.page_size != null) {
    const start = (page - 1) * filter.page_size;
    return { items: rows.slice(start, start + filter.page_size), total };
  }
  return { items: rows, total };
}

export function createTransaction(args: Record<string, unknown>): string {
  const input = obj<TransactionInput>(args, "input");
  const id = nextId("mock-txn");
  const { merchantId } = resolveMerchant(input);
  ledger.transactions.push(mapInputToTxn(input, id, merchantId));
  emitLedgerChanged(); // 交易写入；建商户随交易写一并发一次（ADR-0044 决策 4）
  return id;
}

export function updateTransaction(args: Record<string, unknown>): void {
  const id = str(args, "id");
  const row = ledger.transactions.find((t) => t.id === id && !t.is_deleted);
  if (!row) throw new Error(`替身后端：交易不存在 ${id}`);
  const input = obj<UpdateTransactionInput>(args, "input");
  const { merchantId } = resolveMerchant(input);
  const replaced = mapInputToTxn(input, id, merchantId);
  Object.assign(row, replaced, { created_at: row.created_at });
  touch(row); // updated_at 前进 + version 自增，与参考写入同一触碰口径
  emitLedgerChanged(); // 交易写入；建商户随交易写一并发一次（ADR-0044 决策 4）
}

export function deleteTransaction(args: Record<string, unknown>): void {
  softDelete(ledger.transactions, str(args, "id"), "交易");
  emitLedgerChanged();
}

function newMerchant(name: string): Merchant {
  return {
    id: nextId("mock-mch"),
    name,
    updated_at: nowIso(),
    version: 1,
    device_id: DEVICE_ID,
    is_deleted: false,
  };
}

/** 商户名字符串解析（AI 导入契约投影）：精确命中在用商户复用，未命中即建。 */
function resolveMerchant(input: TransactionInput): { merchantId: string | null } {
  if (input.merchant_name) {
    const hit = ledger.merchants.find((m) => !m.is_deleted && m.name === input.merchant_name);
    if (hit) return { merchantId: hit.id };
    const row = newMerchant(input.merchant_name);
    ledger.merchants.push(row);
    return { merchantId: row.id };
  }
  return { merchantId: input.merchant_id ?? null };
}

function mapInputToTxn(
  input: TransactionInput,
  id: string,
  merchantId: string | null,
): Transaction {
  const now = nowIso();
  const fundings: TransactionFunding[] = (input.funding ?? []).map((f) => ({
    account_id: f.account_id,
    amount_cents: f.amount_cents,
    label: f.label ?? null,
    derived: false,
  }));
  const purchases: TransactionPurchase[] = (input.purchases ?? []).map((p) => ({
    name: p.name,
    quantity: p.quantity,
    category_id: p.category_id ?? null,
    unit_price_cents: p.unit_price_cents ?? null,
  }));
  return {
    id,
    kind: input.kind,
    amount_cents: input.amount_cents,
    currency_code: input.currency_code,
    // 不折算（ADR-0143 决策 1）：本位币金额按原币直记，折算留痕恒空
    amount_native_cents: input.amount_cents,
    fx_rate_used: null,
    fx_rate_source: null,
    // 分解行读回 null（ADR-0138 契约）：携分解时 account_id 不投影
    account_id: fundings.length > 0 ? null : (input.account_id ?? null),
    to_account_id: input.to_account_id ?? null,
    funding_account_id: input.funding_account_id ?? null,
    category_id: input.category_id ?? null,
    merchant_id: merchantId,
    policy_id: input.policy_id ?? null,
    source: null,
    convert: null,
    refund_of_transaction_id: input.refund_of_transaction_id ?? null,
    fundings,
    purchases,
    source_order_no: input.source_order_no ?? null,
    note: input.note ?? null,
    date: input.date,
    created_at: now,
    updated_at: now,
    version: 1,
    device_id: DEVICE_ID,
    is_deleted: false,
  };
}

// —— 共享小件 ——

function findActive<T extends { id: string; is_deleted: boolean }>(
  rows: T[],
  id: string,
  label: string,
): T {
  const row = rows.find((r) => r.id === id && !r.is_deleted);
  if (!row) throw new Error(`替身后端：${label}不存在或已删除 ${id}`);
  return row;
}

function softDelete<
  T extends { id: string; is_deleted: boolean; updated_at: string; version: number },
>(rows: T[], id: string, label: string): void {
  const row = findActive(rows, id, label);
  row.is_deleted = true;
  touch(row);
}
