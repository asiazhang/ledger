import type {
  PhysicalAsset,
  PhysicalAssetDisposeInput,
  PhysicalAssetInput,
  PhysicalAssetList,
  PhysicalAssetStatus,
  PhysicalAssetUpdateInput,
  PhysicalAssetValuationInput,
} from "@ledger/types";
import {
  DEVICE_ID,
  MOCK_TODAY,
  db,
  emitLedgerChanged,
  findActive,
  nextId,
  nowIso,
  obj,
  softDelete,
  str,
  touch,
} from "./state";
import type { MockAssetValuation, MockPhysicalAsset } from "./state";

/**
 * 实物资产域替身（issue #1975 / spec #1964 / ADR-0143）：估值档案内存态读写
 * 闭环，实物资产页「建档 → 更新估值 → 处置 → 列表反馈」可用。从简口径（不追
 * 行为等价，与真后端的差异点，发现误导 UI 开发时以真后端为准修替身）：
 *
 * - **估值只追加不改写**（真后端同语义）：每次更新估值追加一条历史行，当前
 *   估值 = 最新一条（日期最大，同日按插入序——后插者胜，对应真后端 UUID v7
 *   主键降序首条）；历史行不外发（API 无该读面），仅作当前估值的唯一事实源。
 * - **不折算**（ADR-0143 决策 1，budget.ts 同款）：夹具单币种 CNY，
 *   native 估值 = 原币金额直记；native_currency = 账本本位币（设置域内存态）。
 * - **在持合计恒指在持行**（与筛选无关，真后端同口径）：已处置行 native 估值
 *   投影缺席（null）且不计入合计；折算与累计先于筛选。
 * - **缺省日期锚定替身「今日」MOCK_TODAY（2026-01-15）**：估值日缺省 = 今天，
 *   替身不随真实时钟走（确定性纪律，见 state.ts）。
 * - **校验只做存在性必需**（资产行在用、名称/估值必填项缺失报错）：处置日期
 *   不得早于购买日、拒绝未来估值日、处置价与币种成对等真后端校验不模拟。
 *
 * 写路径（create / update / 估值追加 / dispose / delete）完成后统一发
 * `ledger:changed`（ADR-0143 决策 3）。估值追加只落历史行、不触碰资产行版本
 * （真后端同款：仅 INSERT 历史行）。
 */

/** 资产的最新估值历史行（create 必写首条，缺行 = 脏数据：显式报错不静默）。 */
function latestValuation(assetId: string): MockAssetValuation {
  let latest: MockAssetValuation | null = null;
  for (const v of db().physical_asset_valuations) {
    if (v.asset_id !== assetId) continue;
    if (latest === null || v.valuation_date >= latest.valuation_date) latest = v;
  }
  if (latest === null) throw new Error(`替身后端：实物资产缺估值历史行 ${assetId}`);
  return latest;
}

/** 存储行 → 实体投影：当前估值三件套来自最新历史行；折算/累计先于筛选。 */
function projectAsset(row: MockPhysicalAsset, holdingTotal: { sum: number }): PhysicalAsset {
  const valuation = latestValuation(row.id);
  const native =
    row.status === "holding"
      ? (() => {
          holdingTotal.sum += valuation.amount_cents; // 不折算直记（见头注）
          return valuation.amount_cents;
        })()
      : null;
  return {
    ...row,
    current_valuation_cents: valuation.amount_cents,
    current_valuation_currency_code: valuation.currency_code,
    current_valuation_date: valuation.valuation_date,
    current_valuation_native_cents: native,
    native_currency: db().settings.base_currency,
  };
}

/** 状态参数解析：缺省 = 在持（真后端同默认）；未知值报错。 */
function parseStatus(raw: unknown): PhysicalAssetStatus {
  if (raw === null || raw === undefined || raw === "") return "holding";
  if (raw === "holding" || raw === "disposed") return raw;
  throw new Error(`替身后端：未知实物资产状态 ${String(raw)}`);
}

/** 在用资产行（存在性校验：不存在或已软删报错）。 */
function activeAsset(id: string): MockPhysicalAsset {
  return findActive(db().physical_assets, id, "实物资产");
}

export function listPhysicalAssets(args: Record<string, unknown>): PhysicalAssetList {
  const filter = parseStatus(args["status"]);
  const rows = db()
    .physical_assets.filter((a) => !a.is_deleted)
    .sort((a, b) => a.created_at.localeCompare(b.created_at) || a.id.localeCompare(b.id));
  const holdingTotal = { sum: 0 }; // 折算/累计先于筛选：回看已处置时合计不变
  const assets = rows
    .map((row) => projectAsset(row, holdingTotal))
    .filter((asset) => asset.status === filter);
  return {
    assets,
    holding_total_native_cents: holdingTotal.sum,
    native_currency: db().settings.base_currency,
  };
}

export function getPhysicalAsset(args: Record<string, unknown>): PhysicalAsset {
  const row = activeAsset(str(args, "id"));
  return projectAsset(row, { sum: 0 });
}

/** 必需数字字段窄化（缺失报错，与 str() 同语义）：嵌套 input 内的金额字段用。 */
function reqNum(source: Record<string, unknown>, key: string): number {
  const v = source[key];
  if (typeof v !== "number") throw new Error(`替身后端：命令参数 ${key} 缺失`);
  return v;
}

/** 可选日期字段窄化：缺省 = 替身今日（真后端同缺省语义，锚见头注）。 */
function optDate(source: Record<string, unknown>, key: string): string {
  const v = source[key];
  return typeof v === "string" && v !== "" ? v : MOCK_TODAY;
}

export function createPhysicalAsset(args: Record<string, unknown>): string {
  const input = obj<PhysicalAssetInput>(args, "input");
  const fields = input as unknown as Record<string, unknown>;
  const name = str(fields, "name");
  const amount = reqNum(fields, "initial_valuation_cents");
  const currency = str(fields, "initial_valuation_currency_code");
  const row: MockPhysicalAsset = {
    id: nextId("mock-pa"),
    name,
    purchase_date: input.purchase_date ?? null,
    purchase_price_cents: input.purchase_price_cents ?? null,
    purchase_currency_code: input.purchase_currency_code ?? null,
    status: "holding",
    disposal_date: null,
    disposal_price_cents: null,
    disposal_currency_code: null,
    created_at: nowIso(),
    updated_at: nowIso(),
    version: 1,
    device_id: DEVICE_ID,
    is_deleted: false,
  };
  db().physical_assets.push(row);
  // 建档即首条估值历史行（两行写入同「事务」，真后端同语义）
  db().physical_asset_valuations.push({
    id: nextId("mock-pav"),
    asset_id: row.id,
    valuation_date: optDate(fields, "initial_valuation_date"),
    amount_cents: amount,
    currency_code: currency,
  });
  emitLedgerChanged();
  return row.id;
}

export function updatePhysicalAsset(args: Record<string, unknown>): void {
  const row = activeAsset(str(args, "id"));
  const input = obj<PhysicalAssetUpdateInput>(args, "input");
  if (input.name !== undefined) row.name = input.name;
  if (input.purchase_date !== undefined) row.purchase_date = input.purchase_date;
  if (input.purchase_price_cents !== undefined)
    row.purchase_price_cents = input.purchase_price_cents;
  if (input.purchase_currency_code !== undefined) {
    row.purchase_currency_code = input.purchase_currency_code;
  }
  touch(row);
  emitLedgerChanged();
}

export function updatePhysicalAssetValuation(args: Record<string, unknown>): void {
  const row = activeAsset(str(args, "id"));
  const input = obj<PhysicalAssetValuationInput>(args, "input");
  const fields = input as unknown as Record<string, unknown>;
  // 只追加历史行不改写（真后端同语义）；当前估值由读投影取最新一条，
  // 资产行版本不触碰（真后端仅 INSERT 历史行）
  db().physical_asset_valuations.push({
    id: nextId("mock-pav"),
    asset_id: row.id,
    valuation_date: optDate(fields, "valuation_date"),
    amount_cents: reqNum(fields, "amount_cents"),
    currency_code: str(fields, "currency_code"),
  });
  emitLedgerChanged();
}

export function disposePhysicalAsset(args: Record<string, unknown>): void {
  const row = activeAsset(str(args, "id"));
  const input = obj<PhysicalAssetDisposeInput>(args, "input");
  const fields = input as unknown as Record<string, unknown>;
  row.status = "disposed";
  row.disposal_date = str(fields, "disposal_date");
  row.disposal_price_cents = input.disposal_price_cents ?? null;
  row.disposal_currency_code = input.disposal_currency_code ?? null;
  touch(row);
  emitLedgerChanged();
}

export function deletePhysicalAsset(args: Record<string, unknown>): void {
  softDelete(db().physical_assets, str(args, "id"), "实物资产");
  emitLedgerChanged();
}
