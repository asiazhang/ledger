// 翻页渲染性能夹具与桌面表格装配（issue #2012 计数守门 / #2010 渲染优化共用）：
// 固定 50 行交易 fixture（确定性、无随机源）覆盖主列表全部呈现形态——四通用 kind、
// 转账双账户、借贷派生四方向、购买项展开（订单块）、软删商户、来源三态、长备注、
// 无备注行；计数守门（render-gate.test.ts，默认 Vitest 套件）与每日计时基准
// （render.bench.ts，perf-bench.yml）消费同一 fixture，形态面与量测面对齐。
//
// 装配形态镜像 TransactionsView 的桌面表格接线（buildTransactionColumns +
// expandPurchaseRows + displayRowKey + purchaseRowClass + 固定列宽 scroll-x +
// size=small + remote + 无内建分页），不带视图壳（筛选行 / 弹窗 / 分页条）——
// 计数与计时只量表格本体，视图壳的常数噪声不进阈值。行内交互组件
// （AccountLink 等）需要的 router / 消息替身由消费方测试文件提供（vi.mock
// 声明须留在测试文件，随 import 提升先行注册）。

import { defineComponent, h, type PropType } from "vue";
import { NDataTable, type DataTableColumn } from "naive-ui";
import {
  buildTransactionColumns,
  displayRowKey,
  expandPurchaseRows,
  purchaseRowClass,
} from "@/transaction/transaction-columns";
import { sumFixedColumnWidths } from "@ledger/utils/table";
import { useReferenceStore } from "@/stores/reference";
import { mount, type VueWrapper } from "@vue/test-utils";
import { wireInvokeSeam, type InvokeSeamOverride } from "@ledger/test-support/invoke-mock";
import { makeAccount, makeMerchant, makePurchase, makeTransaction } from "../factories";
import type { Account, Merchant, Transaction, TransactionSource } from "@ledger/types";

/** fixture 交易笔数（主列表页大小上限档，与 #2010 诊断的「50 行卡顿」同量级）。 */
export const FIXTURE_TXN_COUNT = 50;

/** 长备注形态：超长文本（购买项/备注列省略路径的触发形态）。 */
export const LONG_NOTE =
  "退货重下单：进口无谷深海鱼油成猫粮专用 10kg 装大袋，订单尾号 8827，与客服确认后按原路退回，预计 3-5 个工作日到账，备注留存以核对运费差额与优惠券分摊金额";

/** 账户集：资金三账户（现金 / 工资卡 / 支付宝）+ 借贷两侧（借出·张三 / 借入·李四），
 * 借贷派生四方向的识别依据是两端账户类型（receivable / debt）。 */
const FIXTURE_ACCOUNTS: Account[] = [
  makeAccount({ id: "acc-cash", name: "现金", type: "cash" }),
  makeAccount({ id: "acc-bank", name: "工资卡", type: "bank" }),
  makeAccount({ id: "acc-alipay", name: "支付宝", type: "ewallet" }),
  makeAccount({ id: "acc-recv", name: "借出·张三", type: "receivable" }),
  makeAccount({ id: "acc-debt", name: "借入·李四", type: "debt" }),
];

/** 商户集：在用两户 + 软删一户（软删商户仍有历史交易，MerchantLink 含软删解析）。 */
const FIXTURE_MERCHANTS: Merchant[] = [
  makeMerchant({ id: "mch-1", name: "京东" }),
  makeMerchant({ id: "mch-2", name: "山姆会员店" }),
  makeMerchant({ id: "mch-del", name: "已闭店商城", is_deleted: true }),
];

/** 来源三态 fixture：订阅（在用）、分期（在用）、保单（末轮软删）。 */
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

/** 第 8 型转账行的借贷方向轮转：五轮覆盖借出 / 收回 / 借入 / 还款 + 普通转账兜底。 */
const LENDING_ROTATION: Array<{ from: string; to: string }> = [
  { from: "acc-cash", to: "acc-recv" }, // 借出：资金 → receivable
  { from: "acc-recv", to: "acc-cash" }, // 收回：receivable → 资金
  { from: "acc-debt", to: "acc-cash" }, // 借入：debt → 资金
  { from: "acc-cash", to: "acc-debt" }, // 还款：资金 → debt
  { from: "acc-cash", to: "acc-alipay" }, // 普通转账：资金互转
];

/** fixture 交易集（模块级常量：纯数据、确定性，两消费方同源）。 */
export const FIXTURE_TXNS: Transaction[] = Array.from(
  { length: FIXTURE_TXN_COUNT },
  (_, i): Transaction => {
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
        const dir = LENDING_ROTATION[repeat];
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
  },
);

/** 展开后的显示行数：每轮两笔购买项订单（3 项 + 2 项）各多 2 / 1 行 → 50 + 15 = 65。 */
export const FIXTURE_DISPLAY_ROW_COUNT = 65;

/** 类型列期望标签清单（每笔交易一个订单级合并格）：借贷派生四方向 + 普通转账 + 三基础 kind。 */
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

/** fixture 展开显示行集（消费方以本出口取表格 data，与视图接线同源）。 */
export function fixtureDisplayRows(): Transaction[] {
  return expandPurchaseRows(FIXTURE_TXNS);
}

/** 布线 fixture 参考数据并预热参考 store（账户/商户覆写，分类/币种走接缝规范夹具）。 */
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

/** 桌面表格测试装配：镜像 TransactionsView 的 NDataTable 接线（见文件头注）。 */
export const DesktopRenderTable = defineComponent({
  name: "DesktopRenderTable",
  props: {
    columns: {
      type: Array as PropType<DataTableColumn<Transaction>[]>,
      required: true,
    },
    rows: { type: Array as PropType<Transaction[]>, required: true },
  },
  setup(props) {
    return () =>
      h(NDataTable, {
        columns: props.columns,
        data: props.rows,
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

/** 构建一份新的列数组（与视图同参：展开装配 + 行菜单入口）；重复调用产出新列对象，
 * 是「同一数据全表重渲染」计数窗口的触发形态（列对象引用更新 → 全部单元格重渲染）。 */
export function buildFixtureColumns(): DataTableColumn<Transaction>[] {
  return buildTransactionColumns(useReferenceStore(), {
    onRowMenuOpen: () => {},
    expandPurchases: true,
  });
}

/** 挂载桌面表格（fixture 行集 + 生产列构造），返回 wrapper 供断言与计数。 */
export function mountDesktopRenderTable(): VueWrapper {
  return mount(DesktopRenderTable, {
    props: { columns: buildFixtureColumns(), rows: fixtureDisplayRows() },
  });
}
