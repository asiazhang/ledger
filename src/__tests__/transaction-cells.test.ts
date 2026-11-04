import { beforeEach, describe, expect, it } from "vitest";
import { amountPrivacyEnabled, formatAmount } from "@ledger/money";
import { kindSemanticColor } from "@ledger/theme/semantic-colors";
import { currentLocale } from "@ledger/i18n";
import type { Account, Category, Merchant, Transaction } from "@ledger/types";
import {
  KIND_TAG_TYPE,
  accountCellView,
  displayAmountText,
  kindLabel,
  kindTagStyle,
  purchaseQtyText,
  sourceCellView,
  statusTagStyle,
  withTransactionCells,
  type AccountAtom,
  type ReferenceStore,
} from "@/transaction/transaction-cells";
import { resolveLendingDirection } from "@/transaction/lending";
import {
  makeAccount,
  makeCategory,
  makeFunding,
  makeMerchant,
  makePurchase,
  makeTransaction,
} from "./factories";

/** 单元格预计算单测（issue #2010 验收①）：与现渲染口径逐字段一致——类型标签文案
 * （含借贷派生）、金额（含隐私模式与数字分组）、分类路径、借贷派生文案、双账户 /
 * 分解行标注、购买项行。表格列渲染只消费本模型的纯数据（render-gate ①项收紧到 0
 * 的形态基础），本文件钉住「预计算产物 == 既有单源函数产物」。 */

// —— 参考数据替身：最小 ReferenceStore 投影（与 renderAccountCell/categoryPath 同源读面） ——
const ACCOUNTS: Account[] = [
  makeAccount({ id: "acc-1", name: "现金", type: "cash" }),
  makeAccount({ id: "acc-2", name: "工资卡", type: "bank" }),
  makeAccount({ id: "acc-recv", name: "借出·张三", type: "receivable" }),
  makeAccount({ id: "acc-debt", name: "借入·李四", type: "debt" }),
];
const MERCHANTS: Merchant[] = [
  makeMerchant({ id: "mch-1", name: "京东" }),
  makeMerchant({ id: "mch-del", name: "已闭店商城", is_deleted: true }),
];
const CATEGORIES: Category[] = [
  makeCategory({ id: "cat-root", name: "生活" }),
  makeCategory({ id: "cat-child", name: "宠物", parent_id: "cat-root" }),
];

function buildReference(): ReferenceStore {
  const accountMap = new Map(ACCOUNTS.map((a) => [a.id, a]));
  const merchantMap = new Map(MERCHANTS.map((m) => [m.id, m]));
  const reference = {
    accountMap,
    merchantMap,
    categoryPath: (id: string | null | undefined) => {
      if (!id) return "";
      const child = CATEGORIES.find((c) => c.id === id);
      if (!child) return "";
      const parent = child.parent_id ? CATEGORIES.find((c) => c.id === child.parent_id) : null;
      return parent ? `${parent.name} > ${child.name}` : child.name;
    },
    getCurrency: (code: string) =>
      code === "CNY" ? { code: "CNY", symbol: "¥", decimal_places: 2 } : undefined,
  };
  return reference as unknown as ReferenceStore;
}

const reference = buildReference();
const link = (id: string, name: string): AccountAtom => ({ link: true, accountId: id, name });
const placeholder: AccountAtom = { link: false, placeholder: "-" };

describe("withTransactionCells 类型标签（借贷派生经 kindLabel 单源）", () => {
  it("基础 kind 按自身标签；语义色型随 KIND_TAG_TYPE", () => {
    const rows = withTransactionCells(
      [
        makeTransaction({ id: "t1", kind: "expense" }),
        makeTransaction({ id: "t2", kind: "income" }),
        makeTransaction({ id: "t3", kind: "refund" }),
      ],
      reference,
      "dark",
    );
    expect(rows.map((r) => r.cells.kindText)).toEqual(["支出", "收入", "退款"]);
    expect(rows.map((r) => r.cells.kindTagType)).toEqual(["warning", "success", "info"]);
  });

  it("借贷四方向派生（两端账户类型构成借贷）与普通转账", () => {
    const rows = withTransactionCells(
      [
        makeTransaction({
          id: "lend",
          kind: "transfer",
          account_id: "acc-1",
          to_account_id: "acc-recv",
        }),
        makeTransaction({
          id: "collect",
          kind: "transfer",
          account_id: "acc-recv",
          to_account_id: "acc-1",
        }),
        makeTransaction({
          id: "borrow",
          kind: "transfer",
          account_id: "acc-debt",
          to_account_id: "acc-1",
        }),
        makeTransaction({
          id: "repay",
          kind: "transfer",
          account_id: "acc-1",
          to_account_id: "acc-debt",
        }),
        makeTransaction({
          id: "plain",
          kind: "transfer",
          account_id: "acc-1",
          to_account_id: "acc-2",
        }),
      ],
      reference,
      "dark",
    );
    expect(rows.map((r) => r.cells.kindText)).toEqual(["借出", "收回", "借入", "还款", "转账"]);
    // 派生口径与 lending 单源一致（kindLabel 内部同函数）
    expect(resolveLendingDirection(rows[0], (id) => reference.accountMap.get(id)?.type)).toBe(
      "lend",
    );
  });

  it("kindTagStyle 为 kindTagStyle(type, theme) 的共享对象（主题/色型单点）", () => {
    const rows = withTransactionCells(
      [makeTransaction({ id: "t", kind: "expense" })],
      reference,
      "light",
    );
    expect(rows[0].cells.kindTagStyle).toBe(kindTagStyle("warning", "light"));
  });
});

describe("withTransactionCells 金额（displayAmountText 单源：隐私模式 / 数字分组 / 特殊 kind）", () => {
  it("基础行金额 = formatAmount(amount_native_cents)，携带语义色", () => {
    const row = makeTransaction({ id: "t", amount_native_cents: 123456 });
    const cells = withTransactionCells([row], reference, "dark")[0].cells;
    expect(cells.amountText).toBe(formatAmount(123456, reference.getCurrency("CNY")));
    expect(cells.amountText).toBe("¥1234.56");
    expect(cells.amountColor).toBe(kindSemanticColor("expense", "dark"));
  });

  it("转换行显示转出金额（ADR-0099 确认单口径）、份额调整行空值 '-'（ADR-0106）", () => {
    const convert = makeTransaction({
      id: "tc",
      kind: "convert",
      amount_native_cents: 359062,
      convert: {
        to_instrument_id: "inst",
        to_symbol: "519700",
        to_quantity: 10,
        out_amount_cents: 361561,
        in_amount_cents: 361561,
      },
    });
    const split = makeTransaction({ id: "ts", kind: "split", amount_native_cents: 0 });
    const cells = withTransactionCells([convert, split], reference, "dark").map((r) => r.cells);
    expect(cells[0].amountText).toBe("¥3615.61");
    expect(cells[1].amountText).toBe("-");
  });

  it("隐私模式掩码即时生效（••••，spec #564 掩码恒等）", () => {
    amountPrivacyEnabled.value = true;
    try {
      const cells = withTransactionCells(
        [makeTransaction({ id: "t", amount_native_cents: 123456 })],
        reference,
        "dark",
      )[0].cells;
      expect(cells.amountText).toBe("••••");
    } finally {
      amountPrivacyEnabled.value = false;
    }
  });

  it("数字分组随界面语言（中文 4 位 / 英文 3 位，词汇表「数字分组」）", () => {
    const row = makeTransaction({ id: "t", amount_native_cents: 123456789 });
    expect(withTransactionCells([row], reference, "dark")[0].cells.amountText).toBe("¥123,4567.89");
    const prev = currentLocale.value;
    currentLocale.value = "en-US";
    try {
      expect(withTransactionCells([row], reference, "dark")[0].cells.amountText).toBe(
        "¥1,234,567.89",
      );
    } finally {
      currentLocale.value = prev;
    }
  });
});

describe("withTransactionCells 分类路径与商户", () => {
  it("分类路径按层级（父 > 子）；无分类与未知 id 回退 '-'", () => {
    const rows = withTransactionCells(
      [
        makeTransaction({ id: "t1", category_id: "cat-child" }),
        makeTransaction({ id: "t2", category_id: null }),
        makeTransaction({ id: "t3", category_id: "cat-unknown" }),
      ],
      reference,
      "dark",
    );
    expect(rows.map((r) => r.cells.categoryText)).toEqual(["生活 > 宠物", "-", "-"]);
  });

  it("展开视图购买项行取商品自己的分类（ADR-0138 决策 13）；商户含软删可解析", () => {
    const expanded = {
      ...makeTransaction({
        id: "t",
        merchant_id: "mch-del",
        purchases: [
          makePurchase({ name: "猫粮", quantity: 2, category_id: "cat-root" }),
          makePurchase({ name: "洗衣液", quantity: 1, category_id: null }),
        ],
      }),
      purchaseIndex: 0,
    } as Transaction;
    const cells = withTransactionCells([expanded], reference, "dark")[0].cells;
    expect(cells.categoryText).toBe("生活");
    expect(cells.merchant).toEqual({ merchantId: "mch-del", name: "已闭店商城" });
    // 第二购买项行：商品分类为空 → '-'
    const second = withTransactionCells(
      [{ ...expanded, purchaseIndex: 1 } as Transaction],
      reference,
      "dark",
    )[0].cells;
    expect(second.categoryText).toBe("-");
  });

  it("无商户 / 未知商户 id → null（渲染 '-'）", () => {
    const rows = withTransactionCells(
      [makeTransaction({ id: "t1" }), makeTransaction({ id: "t2", merchant_id: "mch-x" })],
      reference,
      "dark",
    );
    expect(rows.map((r) => r.cells.merchant)).toEqual([null, null]);
  });
});

describe("withTransactionCells 账户格三形态（renderAccountCell 同一模型单源）", () => {
  it("单账户：可见账户可下钻；黑洞/隐藏账户占位 '-'", () => {
    expect(accountCellView(makeTransaction({ id: "t1", account_id: "acc-1" }), reference)).toEqual({
      form: "single",
      account: link("acc-1", "现金"),
    });
    expect(
      accountCellView(makeTransaction({ id: "t2", account_id: "acc-hidden" }), reference),
    ).toEqual({
      form: "single",
      account: placeholder,
    });
  });

  it("转账双向「转出 → 转入」；buy/sell 出资账户按资金流出方在前（ADR-0096 决策 6）", () => {
    expect(
      accountCellView(
        makeTransaction({ id: "t", kind: "transfer", account_id: "acc-2", to_account_id: "acc-1" }),
        reference,
      ),
    ).toEqual({ form: "two", from: link("acc-2", "工资卡"), to: link("acc-1", "现金") });
    expect(
      accountCellView(
        makeTransaction({
          id: "tb",
          kind: "buy",
          account_id: "acc-1",
          funding_account_id: "acc-2",
        }),
        reference,
      ),
    ).toEqual({ form: "two", from: link("acc-2", "工资卡"), to: link("acc-1", "现金") });
    expect(
      accountCellView(
        makeTransaction({
          id: "ts",
          kind: "sell",
          account_id: "acc-1",
          funding_account_id: "acc-2",
        }),
        reference,
      ),
    ).toEqual({ form: "two", from: link("acc-1", "现金"), to: link("acc-2", "工资卡") });
  });

  it("出资分解行：多账户「等 N 账户」/ 同账户多条「同账户 N 笔」/ 单条无标注；派生行另带标注", () => {
    const combo = makeTransaction({
      id: "t1",
      account_id: null,
      fundings: [makeFunding({ account_id: "acc-1" }), makeFunding({ account_id: "acc-2" })],
    });
    expect(accountCellView(combo, reference)).toEqual({
      form: "funding",
      first: link("acc-1", "现金"),
      marker: "等 2 账户",
      derivedMarkerText: null,
    });
    const same = makeTransaction({
      id: "t2",
      account_id: null,
      fundings: [makeFunding({ account_id: "acc-1" }), makeFunding({ account_id: "acc-1" })],
    });
    expect(accountCellView(same, reference)).toEqual({
      form: "funding",
      first: link("acc-1", "现金"),
      marker: "同账户 2 笔",
      derivedMarkerText: null,
    });
    const single = makeTransaction({
      id: "t3",
      account_id: null,
      fundings: [makeFunding({ account_id: "acc-1" })],
    });
    const singleView = accountCellView(single, reference);
    expect(singleView.form === "funding" ? singleView.marker : "非分解形态").toBeNull();
    const derived = makeTransaction({
      id: "t4",
      account_id: null,
      fundings: [
        makeFunding({ account_id: "acc-1", derived: true }),
        makeFunding({ account_id: "acc-2", derived: true }),
      ],
    });
    expect(accountCellView(derived, reference)).toMatchObject({
      marker: "等 2 账户",
      derivedMarkerText: "按比例自动分解",
    });
  });
});

describe("withTransactionCells 来源格（SourceLink 语义单源化）", () => {
  it("展示名在场用展示名；类型全称供悬停/触控标注；状态标注经 i18n", () => {
    const view = sourceCellView(
      {
        kind: "subscription",
        entity_id: "sub-1",
        display_name: "流媒体会员",
        status: "cancelled",
      },
      "dark",
    );
    expect(view).toMatchObject({
      name: "流媒体会员",
      typeTitle: "订阅计划",
      hasDisplayName: true,
      statusLabel: "已取消",
      clickable: true,
    });
  });

  it("展示名空串按类型名兜底（hasDisplayName=false，触控小字不重复）；软删保单不可点击", () => {
    const fallback = sourceCellView(
      { kind: "policy", entity_id: "p-0", display_name: "", status: null },
      "dark",
    );
    expect(fallback.name).toBe("保单");
    expect(fallback.hasDisplayName).toBe(false);
    const deleted = sourceCellView(
      {
        kind: "policy",
        entity_id: "p-1",
        display_name: "重疾险",
        status: "deleted",
      },
      "dark",
    );
    expect(deleted.clickable).toBe(false);
    expect(deleted.statusLabel).toBe("已删除");
  });
});

describe("withTransactionCells 备注格三形态", () => {
  it("无备注 '-'（form empty）；有备注带文本；购买项行商品名 + 共 N 件", () => {
    const rows = withTransactionCells(
      [
        makeTransaction({ id: "t1", note: null }),
        makeTransaction({ id: "t2", note: "视频会员月费" }),
      ],
      reference,
      "dark",
    );
    expect(rows[0].cells.note).toEqual({ form: "empty" });
    expect(rows[1].cells.note).toEqual({ form: "note", text: "视频会员月费" });
    const expanded = {
      ...makeTransaction({
        id: "t3",
        purchases: [makePurchase({ name: "进口猫粮", quantity: 3 })],
      }),
      purchaseIndex: 0,
    } as Transaction;
    expect(withTransactionCells([expanded], reference, "dark")[0].cells.note).toEqual({
      form: "purchase",
      name: "进口猫粮",
      qtyText: "共 3 件",
    });
  });

  it("件数标注与 renderPurchaseLine 全文形态同拼法（单源 purchaseQtyText）", () => {
    expect(purchaseQtyText(makePurchase({ quantity: 2 }))).toBe("共 2 件");
  });
});

describe("NTag 平替色板（主题派生，守门见 transaction-columns.test.ts）", () => {
  it("语义色型暗/亮两套随主题；default 型取文本灰/边框灰", () => {
    const dark = kindTagStyle("success", "dark");
    const light = kindTagStyle("success", "light");
    expect(dark.color).toBe("#63e2b7");
    expect(dark.background).toBe("#0000");
    expect(dark.borderColor).toBe("rgba(99, 226, 183, 0.3)");
    expect(light.color).toBe("#18a058");
    expect(light.background).toBe("rgba(24, 160, 88, 0.1)");
    expect(light.borderColor).toBe("rgba(24, 160, 88, 0.3)");
    // 暗色圆角随应用 overrides（radius.small 6px），亮色 naive 出厂 2px
    expect(dark.borderRadius).toBe("6px");
    expect(light.borderRadius).toBe("2px");
    const def = kindTagStyle("default", "dark");
    expect(def.background).toBe("#0000");
    expect(statusTagStyle("dark").background).toBeTruthy();
  });
});

describe("displayAmountText / kindLabel 单源转出口不变", () => {
  it("直连调用照常（详情弹窗 / 移动卡片消费面零变化）", () => {
    expect(
      displayAmountText(reference, makeTransaction({ id: "t", amount_native_cents: 9900 })),
    ).toBe("¥99");
    expect(kindLabel(reference, makeTransaction({ id: "t", kind: "income" }))).toBe("收入");
    expect(KIND_TAG_TYPE.convert).toBe("info");
  });
});

beforeEach(() => {
  amountPrivacyEnabled.value = false;
});
