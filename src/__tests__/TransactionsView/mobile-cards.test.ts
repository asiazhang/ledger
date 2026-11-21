import {
  mountView,
  mountMobile,
  mountPhone,
  cards,
  shownModal,
  closeShownModal,
  makeTxn,
  setTxnDb,
  rowMenu,
  rowMenuKeys,
  openCreateFab,
} from "./common";
import { describe, it, expect, afterEach } from "vitest";
import { flushPromises } from "@vue/test-utils";
import { NDataTable } from "naive-ui";
import { setFakeMedia } from "@ledger/test-support/media-mock";
import { probeColor } from "@ledger/test-support/dom";
import { formatAmount } from "@ledger/money";
import { kindSemanticColor } from "@ledger/theme/semantic-colors";
import { useAppStore } from "@/stores/app";
import { useReferenceStore } from "@/stores/reference";
import { refCurrencies } from "@ledger/test-support/reference-stubs";
import AccountLink from "@/accounts/AccountLink.vue";
import TransactionCardList from "@/transaction/TransactionCardList.vue";
import type { Transaction } from "@ledger/types";
import { visibleModalText } from "@ledger/test-support/dom";
import { makePurchase } from "../factories";

describe("卡片内购买项清单（issue #1884 / ADR-0138 决策 15）", () => {
  function orderTxn(): Transaction {
    return makeTxn(1, "acc-1", {
      kind: "expense",
      amount_cents: 9990,
      amount_native_cents: 9990,
      purchases: [
        makePurchase({
          name: "进口无谷深海鱼油配方成猫粮专用 10kg 装大袋全新升级",
          quantity: 1,
          category_id: "cat-1",
        }),
        makePurchase({ name: "洗衣液", quantity: 2, category_id: "cat-2" }),
        makePurchase({ name: "纸巾" }),
      ],
    });
  }

  it("卡内逐行列出购买项（商品名 + 共 N 件），金额只出现一次、卡头仍是订单级", async () => {
    setTxnDb([orderTxn()]);
    const wrapper = await mountMobile();
    const card = cards(wrapper)[0];
    const text = card.text();
    expect(text).toContain("进口无谷深海鱼油配方成猫粮专用 10kg 装大袋全新升级");
    expect(text).toContain("洗衣液");
    expect(text).toContain("纸巾");
    expect(text).toContain("共 2 件");
    expect(text.split("2026-01-01").length - 1).toBe(1);
    expect(card.findAll(".amount-cell").length).toBe(1);
  });

  it("无购买项卡片零变化：不出现件数标注", async () => {
    setTxnDb([makeTxn(2, "acc-1", { note: "手工备注" })]);
    const wrapper = await mountMobile();
    const text = cards(wrapper)[0].text();
    expect(text).not.toContain("共 1 件");
    expect(cards(wrapper)[0].find(".transaction-card-purchase").exists()).toBe(false);
  });

  it("整卡点击 = 只读详情：购买项清单呈现长商品名全文", async () => {
    setTxnDb([orderTxn()]);
    const wrapper = await mountMobile();
    await cards(wrapper)[0].trigger("click");
    const modal = shownModal(wrapper);
    expect(modal, "期望详情弹窗打开").toBeTruthy();
    const text = visibleModalText();
    expect(text).toContain("购买项清单");
    expect(text).toContain("进口无谷深海鱼油配方成猫粮专用 10kg 装大袋全新升级");
    expect(text).toContain("共 2 件");
    await closeShownModal(wrapper);
  });
});

const cny = refCurrencies[0];

afterEach(() => {
  useAppStore().setAmountPrivacyEnabled(false);
});

describe("TransactionsView 断点双渲染（issue #846）", () => {
  it("桌面档：表格在、卡片列表与记一笔悬浮按钮不在（回归红线）", async () => {
    const wrapper = await mountView();
    expect(wrapper.findComponent(NDataTable).exists()).toBe(true);
    expect(wrapper.findComponent(TransactionCardList).exists()).toBe(false);
    expect(wrapper.find(".transaction-card").exists()).toBe(false);
    expect(wrapper.find(".create-fab").exists()).toBe(false);
  });

  it("移动档：卡片列表在、表格与工具栏记一笔按钮不在；过滤控件与快捷时间选择保留", async () => {
    const wrapper = await mountMobile();
    expect(wrapper.findComponent(NDataTable).exists()).toBe(false);
    expect(wrapper.findComponent(TransactionCardList).exists()).toBe(true);
    expect(cards(wrapper).length).toBe(20);
    expect(wrapper.text()).not.toContain("记一笔");
    expect(wrapper.find(".create-fab").exists()).toBe(true);
    for (const chip of ["全部", "当月", "当季", "当年", "去年"]) {
      expect(
        wrapper.findAll("button").some((b) => b.text() === chip),
        chip,
      ).toBe(true);
    }
  });

  it("跨断点实时换档：839 ↔ 1280 缩放时卡片与表格互斥切换", async () => {
    setFakeMedia({ width: 1280 });
    const wrapper = await mountView();
    expect(wrapper.findComponent(NDataTable).exists()).toBe(true);
    setFakeMedia({ width: 839 });
    await flushPromises();
    expect(wrapper.findComponent(NDataTable).exists()).toBe(false);
    expect(cards(wrapper).length).toBe(20);
    setFakeMedia({ width: 1280 });
    await flushPromises();
    expect(wrapper.findComponent(NDataTable).exists()).toBe(true);
    expect(wrapper.find(".transaction-card").exists()).toBe(false);
  });
});

describe("移动档卡片字段（同一段列表状态）", () => {
  it("卡片呈现日期/类型/分类/商户/账户/金额，金额着收支语义色", async () => {
    setTxnDb([
      makeTxn(1, "acc-1", {
        kind: "expense",
        category_id: "cat-1",
        merchant_id: "mch-1",
        date: "2026-03-15",
        amount_native_cents: 12345,
      }),
      makeTxn(2, "acc-2", { kind: "income", date: "2026-03-16", amount_native_cents: -6789 }),
    ]);
    const wrapper = await mountMobile();
    const list = cards(wrapper);
    expect(list.length).toBe(2);
    const first = list[0];
    expect(first.text()).toContain("2026-03-15");
    expect(first.text()).toContain("支出");
    const reference = useReferenceStore();
    expect(first.text()).toContain(reference.categoryPath("cat-1"));
    expect(first.text()).toContain("京东");
    expect(first.findComponent(AccountLink).exists()).toBe(true);
    expect(first.text()).toContain("现金");
    const amountEl = first.find(".amount-cell").element as HTMLElement;
    expect(amountEl.textContent).toBe(formatAmount(12345, cny));
    expect(amountEl.style.color).toBe(
      probeColor(kindSemanticColor("expense", useAppStore().theme)),
    );
    const incomeEl = list[1].find(".amount-cell").element as HTMLElement;
    expect(incomeEl.style.color).toBe(probeColor(kindSemanticColor("income", useAppStore().theme)));
  });

  it("转账行账户呈现「转出 → 转入」双向链接（与表格同构）", async () => {
    setTxnDb([makeTxn(1, "acc-1", { kind: "transfer", to_account_id: "acc-2" })]);
    const wrapper = await mountMobile();
    const first = cards(wrapper)[0];
    const links = first.findAllComponents(AccountLink);
    expect(links.map((l: { text(): string }) => l.text())).toEqual(["现金", "银行"]);
    expect(first.text()).toContain("→");
  });

  it("金额隐私模式：隐藏数字不隐藏形状与方向——掩码恒形、语义色保留", async () => {
    setTxnDb([makeTxn(1, "acc-1", { kind: "expense", amount_native_cents: 12345 })]);
    const wrapper = await mountMobile();
    useAppStore().setAmountPrivacyEnabled(true);
    await flushPromises();
    const amountEl = cards(wrapper)[0].find(".amount-cell").element as HTMLElement;
    expect(amountEl.textContent).toBe("••••");
    expect(amountEl.style.color).toBe(
      probeColor(kindSemanticColor("expense", useAppStore().theme)),
    );
  });

  it("来源行保留链接语义：SourceLink 渲染于卡片", async () => {
    setTxnDb([
      makeTxn(1, "acc-1", {
        source: {
          kind: "subscription",
          entity_id: "sub-1",
          display_name: "视频会员",
          status: null,
        },
      }),
    ]);
    const wrapper = await mountMobile();
    expect(cards(wrapper)[0].text()).toContain("视频会员");
  });

  it("金额触控轴点按查看全文（悬停一击可达在卡片上同规）", async () => {
    setTxnDb([makeTxn(1, "acc-1", { amount_native_cents: 1234567 })]);
    const wrapper = await mountPhone();
    await cards(wrapper)[0].find(".amount-cell").trigger("click");
    await flushPromises();
    const popover = document.body.querySelector(".n-popover");
    expect(popover).not.toBeNull();
    expect(popover!.textContent).toContain(formatAmount(1234567, cny));
  });
});

describe("卡片「⋯」与整卡编辑", () => {
  it("卡片「⋯」点开与桌面右键同一菜单（集合一致）", async () => {
    const wrapper = await mountMobile();
    await cards(wrapper)[0].find(".row-actions-btn").trigger("click");
    await flushPromises();
    expect(rowMenu(wrapper).props("show")).toBe(true);
    expect(rowMenuKeys(wrapper)).toEqual(["edit", "refund", "add-item", "menu-divider", "delete"]);
    expect(shownModal(wrapper)).toBeUndefined();
  });

  it("非支出行「⋯」集合同桌面右键（refund 行仅删除）", async () => {
    setTxnDb([
      makeTxn(1, "acc-1", { kind: "transfer", to_account_id: "acc-2" }),
      makeTxn(2, "acc-1", { kind: "refund", refund_of_transaction_id: "txn-x" }),
    ]);
    const wrapper = await mountMobile();
    const list = cards(wrapper);
    await list[0].find(".row-actions-btn").trigger("click");
    await flushPromises();
    expect(rowMenuKeys(wrapper)).toEqual(["edit", "menu-divider", "delete"]);
    await list[1].find(".row-actions-btn").trigger("click");
    await flushPromises();
    expect(rowMenuKeys(wrapper)).toEqual(["delete"]);
  });

  it("整卡点击 = 编辑（refund 行不开放编辑、点击无动作）", async () => {
    setTxnDb([
      makeTxn(1, "acc-1", { kind: "expense" }),
      makeTxn(2, "acc-1", { kind: "refund", refund_of_transaction_id: "txn-x" }),
    ]);
    const wrapper = await mountMobile();
    const list = cards(wrapper);
    await list[0].trigger("click");
    await flushPromises();
    expect(shownModal(wrapper)?.props("title")).toBe("编辑交易");
    await closeShownModal(wrapper);
    await list[1].trigger("click");
    await flushPromises();
    expect(shownModal(wrapper)).toBeUndefined();
  });

  it("卡内链接点击不触发整卡编辑（账户链接照常下钻）", async () => {
    setTxnDb([makeTxn(1, "acc-1", { kind: "expense" })]);
    const wrapper = await mountMobile();
    await cards(wrapper)[0].findComponent(AccountLink).find("button").trigger("click");
    await flushPromises();
    expect(shownModal(wrapper)).toBeUndefined();
  });
});

describe("记一笔悬浮按钮（移动档交易页右下，ADR-0088 决策 5）", () => {
  it("点开大号类型选择（创建闭集收窄后 = 支出/收入/转账，不含借贷、退款与转换，ADR-0135 决策 5 / issue #1782）", async () => {
    const wrapper = await mountPhone();
    expect(await openCreateFab(wrapper)).toEqual(["支出", "收入", "转账"]);
  });
  it("类型意图矩阵：类型选择 → 记一笔意图（携带类型）→ 对应表单（convert 无手工录入入口；ADR-0135 决策 5 / issue #1782 收窄后只剩三通用类型）", async () => {
    const cases = [
      ["支出", "记一笔 · 支出"],
      ["收入", "记一笔 · 收入"],
      ["转账", "记一笔 · 转账"],
    ] as const;
    const wrapper = await mountPhone();
    for (const [optionLabel, expectedTitle] of cases) {
      await wrapper.find(".create-fab").trigger("click");
      await flushPromises();
      const option = [...document.body.querySelectorAll(".create-fab-option")].find(
        (o) => o.textContent === optionLabel,
      );
      expect(option, optionLabel).toBeTruthy();
      option!.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      await flushPromises();
      const modal = shownModal(wrapper);
      expect(modal, optionLabel).toBeTruthy();
      expect(modal!.props("title"), optionLabel).toBe(expectedTitle);
      await closeShownModal(wrapper);
    }
  });

  it("桌面档无记一笔悬浮按钮（顶栏分裂按钮是桌面档唯一入口）", async () => {
    const wrapper = await mountView();
    expect(wrapper.find(".create-fab").exists()).toBe(false);
    expect(wrapper.text()).toContain("记一笔");
  });
});
