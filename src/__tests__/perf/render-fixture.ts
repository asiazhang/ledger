import { computed, defineComponent, h, type PropType } from "vue";
import { useInputMode } from "@/composables/useInputMode";
import { NDataTable, type DataTableColumn } from "naive-ui";
import {
  buildTransactionColumns,
  displayRowKey,
  precomputeTransactionDisplayRows,
  purchaseRowClass,
} from "@/transaction/transaction-columns";
import { sumFixedColumnWidths } from "@ledger/utils/table";
import { useReferenceStore } from "@/stores/reference";
import { mount, type VueWrapper } from "@vue/test-utils";
import { wireInvokeSeam, type InvokeSeamOverride } from "@ledger/test-support/invoke-mock";
import { makeAccount, makeMerchant, makePurchase, makeTransaction } from "../factories";
import type { Account, Merchant, Transaction, TransactionSource } from "@ledger/types";

export const FIXTURE_TXN_COUNT = 50;

export const LONG_NOTE =
  "退货重下单：进口无谷深海鱼油成猫粮专用 10kg 装大袋，订单尾号 8827，与客服确认后按原路退回，预计 3-5 个工作日到账，备注留存以核对运费差额与优惠券分摊金额";

const FIXTURE_ACCOUNTS: Account[] = [
  makeAccount({ id: "acc-cash", name: "现金", type: "cash" }),
  makeAccount({ id: "acc-bank", name: "工资卡", type: "bank" }),
  makeAccount({ id: "acc-alipay", name: "支付宝", type: "ewallet" }),
  makeAccount({ id: "acc-recv", name: "借出·张三", type: "receivable" }),
  makeAccount({ id: "acc-debt", name: "借入·李四", type: "debt" }),
];

const FIXTURE_MERCHANTS: Merchant[] = [
  makeMerchant({ id: "mch-1", name: "京东" }),
  makeMerchant({ id: "mch-2", name: "山姆会员店" }),
  makeMerchant({ id: "mch-del", name: "已闭店商城", is_deleted: true }),
];

function sourceFor(repeat: number, pattern: number): TransactionSource | null {
  if (pattern === 4)
    return {
      kind: "policy",
      entity_id: "pol-1",
      display_name: "重疾险",
      status: repeat === 4 ? "deleted" : null,
    };
  if (pattern === 5)
    return {
      kind: "subscription",
      entity_id: "sub-1",
      display_name: "流媒体会员",
      status: repeat === 0 ? "cancelled" : null,
    };
  if (pattern === 9)
    return { kind: "installmentPlan", entity_id: "plan-1", display_name: "手机分期", status: null };
  return null;
}

const LENDING_ROTATION: Array<{ from: string; to: string }> = [
  { from: "acc-cash", to: "acc-recv" },
  { from: "acc-recv", to: "acc-cash" },
  { from: "acc-debt", to: "acc-cash" },
  { from: "acc-cash", to: "acc-debt" },
  { from: "acc-cash", to: "acc-alipay" },
];

export const EXPECTED_KIND_LABELS = {
  借出: 1,
  收回: 1,
  借入: 1,
  还款: 1,
  转账: 6,
  支出: 25,
  收入: 10,
  退款: 5,
} as const;

export function fixtureTxns(count: number): Transaction[] {
  return Array.from({ length: count }, (_, i): Transaction => {
    const repeat = Math.floor(i / 10);
    const pattern = i % 10;
    const id = `perf-txn-${String(i).padStart(3, "0")}`;
    const date = `2026-${String(3 + Math.floor(i / 30)).padStart(2, "0")}-${String(30 - (i % 30)).padStart(2, "0")}`;
    const amount = ((i + 1) * 9973) % 99953;
    const base = {
      id,
      date,
      amount_cents: amount,
      amount_native_cents: amount,
      account_id: "acc-cash",
    };
    switch (pattern) {
      case 0:
        return makeTransaction({
          ...base,
          kind: "expense",
          merchant_id: "mch-1",
          category_id: "cat-1",
          note: `午餐·楼下食堂 ${i}`,
        });
      case 1:
        return makeTransaction({
          ...base,
          kind: "expense",
          purchases: [
            makePurchase({ name: `进口猫粮 ${i}`, quantity: 2 }),
            makePurchase({ name: `洗衣液 ${i}`, quantity: 1 }),
            makePurchase({ name: `抽纸 ${i}`, quantity: 6 }),
          ],
        });
      case 2:
        return makeTransaction({ ...base, kind: "income" });
      case 3:
        return makeTransaction({ ...base, kind: "transfer", to_account_id: "acc-bank" });
      case 4:
        return makeTransaction({
          ...base,
          kind: "expense",
          merchant_id: repeat === 4 ? "mch-del" : "mch-2",
          note: LONG_NOTE,
          source: sourceFor(repeat, pattern),
        });
      case 5:
        return makeTransaction({
          ...base,
          kind: "income",
          account_id: "acc-bank",
          note: "工资",
          source: sourceFor(repeat, pattern),
        });
      case 6:
        return makeTransaction({
          ...base,
          kind: "expense",
          purchases: [
            makePurchase({ name: `猫砂 ${i}`, quantity: 1 }),
            makePurchase({ name: `宠物湿巾 ${i}`, quantity: 3 }),
          ],
        });
      case 7:
        return makeTransaction({
          ...base,
          kind: "refund",
          account_id: "acc-alipay",
          note: `退货退款 ${i}`,
        });
      case 8: {
        const dir = LENDING_ROTATION[repeat % LENDING_ROTATION.length];
        return makeTransaction({
          ...base,
          kind: "transfer",
          account_id: dir.from,
          to_account_id: dir.to,
        });
      }
      default:
        return makeTransaction({
          ...base,
          kind: "expense",
          source: sourceFor(repeat, pattern),
        });
    }
  });
}

export const FIXTURE_TXNS: Transaction[] = fixtureTxns(FIXTURE_TXN_COUNT);

export const FIXTURE_DISPLAY_ROW_COUNT = 65;

export async function wireRenderFixture(): Promise<void> {
  const seam = wireInvokeSeam({
    overrides: {
      list_accounts: (() => FIXTURE_ACCOUNTS) satisfies InvokeSeamOverride,
      list_merchants: (() => FIXTURE_MERCHANTS) satisfies InvokeSeamOverride,
    },
    refreshReferenceStores: true,
  });
  await seam.ready;
}

export const DesktopRenderTable = defineComponent({
  name: "DesktopRenderTable",
  props: {
    columns: {
      type: Array as PropType<DataTableColumn<Transaction>[]>,
      required: true,
    },
    txnCount: {
      type: Number,
      default: FIXTURE_TXN_COUNT,
    },
  },
  setup(props) {
    useInputMode();
    const reference = useReferenceStore();
    const rows = computed(() =>
      precomputeTransactionDisplayRows(reference, fixtureTxns(props.txnCount), {
        expandPurchases: true,
      }),
    );
    return () =>
      h(NDataTable, {
        columns: props.columns,
        data: rows.value,
        rowKey: displayRowKey,
        rowProps: (row: Transaction) => ({ class: purchaseRowClass(row) }),
        loading: false,
        bordered: false,
        size: "small",
        remote: true,
        scrollX: sumFixedColumnWidths(props.columns),
        pagination: false as const,
      });
  },
});

export function buildFixtureColumns(): DataTableColumn<Transaction>[] {
  return buildTransactionColumns(useReferenceStore(), {
    rowActions: true,
    expandPurchases: true,
  });
}

export function mountDesktopRenderTable(options: { txnCount?: number } = {}): VueWrapper {
  return mount(DesktopRenderTable, {
    props: { columns: buildFixtureColumns(), txnCount: options.txnCount ?? FIXTURE_TXN_COUNT },
  });
}
