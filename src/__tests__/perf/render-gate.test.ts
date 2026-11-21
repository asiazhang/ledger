import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, type VueWrapper } from "@vue/test-utils";
import { nextTick } from "vue";
import { NTag, NEllipsis } from "naive-ui";
import * as i18n from "@ledger/i18n";
import * as money from "@ledger/money";
import { amountPrivacyEnabled } from "@ledger/money";
import AccountLink from "@/accounts/AccountLink.vue";
import MerchantLink from "@/merchants/MerchantLink.vue";
import SourceLink from "@/transaction/SourceLink.vue";
import AmountCell from "@/transaction/AmountCell.vue";
import NoteCopyButton from "@ledger/ui-kit/NoteCopyButton.vue";
import * as lending from "@/transaction/lending";
import {
  EXPECTED_KIND_LABELS,
  FIXTURE_DISPLAY_ROW_COUNT,
  FIXTURE_TXN_COUNT,
  LONG_NOTE,
  buildFixtureColumns,
  mountDesktopRenderTable,
  wireRenderFixture,
} from "./render-fixture";

vi.mock("vue-router", () => ({
  useRoute: () => ({ query: {} }),
  useRouter: () => ({ push: () => {} }),
}));

const tSpy = vi.spyOn(i18n, "t");
const formatAmountSpy = vi.spyOn(money, "formatAmount");
const resolveLendingSpy = vi.spyOn(lending, "resolveLendingDirection");

const RENDER_FORMAT_T_BASELINE = 0;
const RENDER_FORMAT_AMOUNT_BASELINE = 0;
const RENDER_RESOLVE_LENDING_BASELINE = 0;

const ACCOUNT_LINK_BASELINE = 0;
const MERCHANT_LINK_BASELINE = 0;
const SOURCE_LINK_BASELINE = 0;
const NOTE_COPY_BUTTON_BASELINE = 0;
const AMOUNT_CELL_BASELINE = 0;
const N_TAG_BASELINE = 0;
const N_ELLIPSIS_BASELINE = 2;

const TABLE_DOM_NODES_BASELINE = 1422;
const TABLE_DOM_NODES_100_BASELINE = 2787;

beforeEach(async () => {
  await wireRenderFixture();
});

async function mountAndFlush(): Promise<VueWrapper> {
  const wrapper = mountDesktopRenderTable();
  await flushPromises();
  return wrapper;
}

function kindLabelCounts(wrapper: VueWrapper): Map<string, number> {
  const counts = new Map<string, number>();
  for (const cell of wrapper.findAll('td[data-col-key="kind"]')) {
    const label = cell.text();
    if (label) counts.set(label, (counts.get(label) ?? 0) + 1);
  }
  return counts;
}

describe("fixture 形态在场（缺形态即红：计数阈值只对覆盖到的形态有意义）", () => {
  it("50 笔交易展开为 65 显示行，购买项订单块以 rowspan 合并", async () => {
    const wrapper = await mountAndFlush();
    const rows = wrapper.findAll(".n-data-table-tbody .n-data-table-tr");
    expect(rows).toHaveLength(FIXTURE_DISPLAY_ROW_COUNT);
    expect(wrapper.findAll('td[data-col-key="kind"][rowspan="3"]')).toHaveLength(5);
  });

  it("类型列标签覆盖借贷派生四方向 + 普通转账 + 三基础 kind，转账双账户同格呈现", async () => {
    const wrapper = await mountAndFlush();
    expect(kindLabelCounts(wrapper)).toEqual(new Map(Object.entries(EXPECTED_KIND_LABELS)));
    const texts = wrapper.findAll(".n-data-table-tbody .n-data-table-tr").map((r) => r.text());
    expect(texts.some((t) => t.includes("现金") && t.includes("工资卡") && t.includes("→"))).toBe(
      true,
    );
  });

  it("长备注与购买项商品名在场（省略路径的触发形态）", async () => {
    const wrapper = await mountAndFlush();
    const body = wrapper.find(".n-data-table-tbody").text();
    expect(body).toContain(LONG_NOTE.slice(0, 20));
    expect(body).toContain("进口猫粮 1");
  });

  it("金额隐私模式：掩码即时呈现于全部金额格（••••）", async () => {
    const wrapper = await mountAndFlush();
    try {
      amountPrivacyEnabled.value = true;
      await nextTick();
      const amountCells = wrapper.findAll(".amount-cell");
      expect(amountCells.length).toBe(FIXTURE_TXN_COUNT);
      for (const cell of amountCells) expect(cell.text()).toBe("••••");
    } finally {
      amountPrivacyEnabled.value = false;
    }
  });
});

describe("渲染期计数守门（阈值 = 优化后基线，收紧纪律见文件头注）", () => {
  it("① 同一数据全表重渲染：渲染期格式化调用 = 0（预计算接线，删除即红）", async () => {
    const wrapper = await mountAndFlush();
    await wrapper.setProps({ columns: buildFixtureColumns() });
    await flushPromises();
    const nextColumns = buildFixtureColumns();
    tSpy.mockClear();
    formatAmountSpy.mockClear();
    resolveLendingSpy.mockClear();
    await wrapper.setProps({ columns: nextColumns });
    await flushPromises();
    expect(
      tSpy.mock.calls.length,
      `渲染期 t() 调用 ${tSpy.mock.calls.length} 次，应为 0（渲染期逐格格式化回归？预计算接线被移除？见文件头注 / #2188）`,
    ).toBe(RENDER_FORMAT_T_BASELINE);
    expect(
      formatAmountSpy.mock.calls.length,
      `渲染期 formatAmount 调用 ${formatAmountSpy.mock.calls.length} 次，应为 0`,
    ).toBe(RENDER_FORMAT_AMOUNT_BASELINE);
    expect(
      resolveLendingSpy.mock.calls.length,
      `渲染期 resolveLendingDirection 调用 ${resolveLendingSpy.mock.calls.length} 次，应为 0`,
    ).toBe(RENDER_RESOLVE_LENDING_BASELINE);
  });

  it("② 行内交互组件实例数 ≤ 基线", async () => {
    const wrapper = await mountAndFlush();
    expect(wrapper.findAllComponents(AccountLink).length).toBeLessThanOrEqual(
      ACCOUNT_LINK_BASELINE,
    );
    expect(wrapper.findAllComponents(MerchantLink).length).toBeLessThanOrEqual(
      MERCHANT_LINK_BASELINE,
    );
    expect(wrapper.findAllComponents(SourceLink).length).toBeLessThanOrEqual(SOURCE_LINK_BASELINE);
    expect(wrapper.findAllComponents(NoteCopyButton).length).toBeLessThanOrEqual(
      NOTE_COPY_BUTTON_BASELINE,
    );
    expect(wrapper.findAllComponents(AmountCell).length).toBeLessThanOrEqual(AMOUNT_CELL_BASELINE);
    expect(wrapper.findAllComponents(NTag).length).toBeLessThanOrEqual(N_TAG_BASELINE);
    expect(wrapper.findAllComponents(NEllipsis).length).toBeLessThanOrEqual(N_ELLIPSIS_BASELINE);
  });

  it("③ 表格容器内 DOM 节点总数 ≤ 基线（50 行档）", async () => {
    const wrapper = await mountAndFlush();
    const table = wrapper.find(".n-data-table");
    expect(table.exists()).toBe(true);
    const count = table.element.querySelectorAll("*").length;
    expect(count).toBeLessThanOrEqual(TABLE_DOM_NODES_BASELINE);
  });
});

describe("100 行/页档位覆盖（issue #2189：主列表页大小上限档守门）", () => {
  it("100 笔交易（150 显示行）挂载：行内重型组件实例仍为 0、DOM 节点 ≤ 100 行档基线", async () => {
    const wrapper = mountDesktopRenderTable({ txnCount: 100 });
    await flushPromises();
    expect(wrapper.findAll(".n-data-table-tbody .n-data-table-tr")).toHaveLength(
      FIXTURE_DISPLAY_ROW_COUNT * 2,
    );
    expect(wrapper.findAllComponents(AccountLink).length).toBeLessThanOrEqual(
      ACCOUNT_LINK_BASELINE,
    );
    expect(wrapper.findAllComponents(MerchantLink).length).toBeLessThanOrEqual(
      MERCHANT_LINK_BASELINE,
    );
    expect(wrapper.findAllComponents(SourceLink).length).toBeLessThanOrEqual(SOURCE_LINK_BASELINE);
    expect(wrapper.findAllComponents(NoteCopyButton).length).toBeLessThanOrEqual(
      NOTE_COPY_BUTTON_BASELINE,
    );
    expect(wrapper.findAllComponents(AmountCell).length).toBeLessThanOrEqual(AMOUNT_CELL_BASELINE);
    expect(wrapper.findAllComponents(NTag).length).toBeLessThanOrEqual(N_TAG_BASELINE);
    expect(wrapper.findAllComponents(NEllipsis).length).toBeLessThanOrEqual(N_ELLIPSIS_BASELINE);
    const table = wrapper.find(".n-data-table");
    expect(table.exists()).toBe(true);
    const count = table.element.querySelectorAll("*").length;
    expect(count).toBeLessThanOrEqual(TABLE_DOM_NODES_100_BASELINE);
  });
});
