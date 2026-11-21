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

function latestValuation(assetId: string): MockAssetValuation {
  let latest: MockAssetValuation | null = null;
  for (const v of db().physical_asset_valuations) {
    if (v.asset_id !== assetId) continue;
    if (latest === null || v.valuation_date >= latest.valuation_date) latest = v;
  }
  if (latest === null) throw new Error(`替身后端：实物资产缺估值历史行 ${assetId}`);
  return latest;
}

function projectAsset(row: MockPhysicalAsset, holdingTotal: { sum: number }): PhysicalAsset {
  const valuation = latestValuation(row.id);
  const native =
    row.status === "holding"
      ? (() => {
          holdingTotal.sum += valuation.amount_cents;
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

function parseStatus(raw: unknown): PhysicalAssetStatus {
  if (raw === null || raw === undefined || raw === "") return "holding";
  if (raw === "holding" || raw === "disposed") return raw;
  throw new Error(`替身后端：未知实物资产状态 ${String(raw)}`);
}

function activeAsset(id: string): MockPhysicalAsset {
  return findActive(db().physical_assets, id, "实物资产");
}

export function listPhysicalAssets(args: Record<string, unknown>): PhysicalAssetList {
  const filter = parseStatus(args["status"]);
  const rows = db()
    .physical_assets.filter((a) => !a.is_deleted)
    .sort((a, b) => a.created_at.localeCompare(b.created_at) || a.id.localeCompare(b.id));
  const holdingTotal = { sum: 0 };
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

function reqNum(source: Record<string, unknown>, key: string): number {
  const v = source[key];
  if (typeof v !== "number") throw new Error(`替身后端：命令参数 ${key} 缺失`);
  return v;
}

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
