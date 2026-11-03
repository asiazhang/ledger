// 翻页渲染性能守门——计数型 Vitest 门禁（issue #2012，关联 #2010 渲染优化主体）：
// 渲染性能在 Linux(WebKitGTK) 是体验敏感点（AGENTS.md 体验标尺），后续迭代
// （新列 / 新组件 / 新格式化逻辑）容易无声劣化；计时断言在 CI 共享 runner 上必然
// flaky、不可作 gate，本文件以**确定性计数指标**守「渲染期不做重复功」的形态：
//   ① 渲染期格式化调用（t / formatAmount / resolveLendingDirection）——同一数据
//      全表重渲染窗口内的调用次数 ≤ 阈值；#2010 文本预计算落地后本项应收紧到 0
//      （预计算在数据到达时完成、不在 patch 期逐格执行，即 #2010 的核心形态）。
//   ② 行内交互组件实例数（AccountLink / MerchantLink / SourceLink /
//      NoteCopyButton / AmountCell / NTag / NEllipsis）≤ 阈值。
//   ③ 表格容器内 DOM 节点总数 ≤ 阈值。
// 计数窗口 = 「同一行集 + 重建列数组」的全表重渲染（列对象引用更新 → 每格
// DataTableCell 组件收到新列对象而重渲染）：不含数据到达路径——预计算若在数据
// 到达时完成，不计入窗口；若格式化被改回渲染期逐格执行（#2010 接线被移除的
// 形态），窗口内计数回到建票基线、阈值收紧后即红（删除即红，ADR-0087 断言强度）。
// 阈值纪律：初始阈值 = 建票时现状基线（下方常量，fixture 推导式见各常量注）；
// #2010 落地时收紧到优化后基线，前后差即 #2010 的量化收益（更新进 #2010）；
// 上调不允许静默进行——须在 PR 中说明原因并关联 #2012。
// 计时趋势不在此文件（CI 计时 flaky）：每日基准走 perf-bench.yml 的
// render.bench.ts（同 fixture，mount p95 只记录不判定），量「计数之外的成本增长」
// （依赖升级、框架行为变化）。本测试在默认 Vitest 套件内运行（CI frontend-test
// job / 本地 pnpm test，check.sh 链路的测试执行面），无独立跳过开关。

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

// 行内交互组件（AccountLink / SourceLink）经 useRouter 下钻：守门只量呈现，
// 不触发导航，router 以最小替身满足注入（vue-router mock 须住测试文件，
// 随 import 提升先于组件装载注册）。
vi.mock("vue-router", () => ({
  useRoute: () => ({ query: {} }),
  useRouter: () => ({ push: () => {} }),
}));

// 渲染期格式化调用计数哨兵（计数窗口见文件头注）。
const tSpy = vi.spyOn(i18n, "t");
const formatAmountSpy = vi.spyOn(money, "formatAmount");
const resolveLendingSpy = vi.spyOn(lending, "resolveLendingDirection");

// —— ① 渲染期格式化调用基线（2026-11 建票时现状，#2010 前形态）——
// t = 134：全表重渲染一次的取词总量——类型标签 50（每笔交易）+ 行内组件取词
//          （AccountLink 悬停提示 60、SourceLink 类型/状态文案、NoteCopyButton
//          无障碍标签 20×2、购买项件数标注 25、行菜单 aria-label 50）。
// formatAmount = 50：每笔交易一个订单级合并金额格。
// resolveLendingDirection = 10：fixture 全部转账行（借贷四方向轮转 + 普通转账）。
const RENDER_FORMAT_T_BASELINE = 134;
const RENDER_FORMAT_AMOUNT_BASELINE = 50;
const RENDER_RESOLVE_LENDING_BASELINE = 10;

// —— ② 行内交互组件实例数基线 ——
// AccountLink 60（40 单账户行 + 10 转账行 × 2）、MerchantLink 10（含 1 软删）、
// SourceLink 15（订阅 5 + 分期 5 + 保单 5）、NoteCopyButton 20（有备注且非购买项行）、
// AmountCell 50（订单级合并）、NTag 52（类型标签 50 + 来源状态标注 2：已删保单 1 +
// 已取消订阅 1）、NEllipsis 162（分类列 65 显示行 + 商户列 50 交易 + 备注列 45
//（备注文本 20 + 购买项商品名 25）+ 表头省略 2：分类/商户列头）。
const ACCOUNT_LINK_BASELINE = 60;
const MERCHANT_LINK_BASELINE = 10;
const SOURCE_LINK_BASELINE = 15;
const NOTE_COPY_BUTTON_BASELINE = 20;
const AMOUNT_CELL_BASELINE = 50;
const N_TAG_BASELINE = 52;
const N_ELLIPSIS_BASELINE = 162;

// —— ③ 表格容器内 DOM 节点总数基线（.n-data-table 内全部元素节点）——
const TABLE_DOM_NODES_BASELINE = 1794;

beforeEach(async () => {
  await wireRenderFixture();
});

async function mountAndFlush(): Promise<VueWrapper> {
  const wrapper = mountDesktopRenderTable();
  await flushPromises();
  return wrapper;
}

/** 类型列标签清单（订单级合并后每笔交易一格；`data-col-key` 是单元格列键钩子）。 */
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
    // 3 件订单的首行承载 rowspan=3 合并格（每轮一个 3 件订单 × 5 轮；按类型列锚定，
    // 其余订单级列同构）
    expect(wrapper.findAll('td[data-col-key="kind"][rowspan="3"]')).toHaveLength(5);
  });

  it("类型列标签覆盖借贷派生四方向 + 普通转账 + 三基础 kind，转账双账户同格呈现", async () => {
    const wrapper = await mountAndFlush();
    expect(kindLabelCounts(wrapper)).toEqual(new Map(Object.entries(EXPECTED_KIND_LABELS)));
    // 转账双账户形态：同一行出现「转出 → 转入」两个账户链接
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
    // 先挂载再置位：app store 首次创建时从 localStorage 水合开关、会覆写渲染前置位
    // （chart-privacy-mask 同款先例）；先挂载后置位也顺带断言「开关即时生效」。
    const wrapper = await mountAndFlush();
    amountPrivacyEnabled.value = true;
    await nextTick();
    const amountCells = wrapper.findAll(".amount-cell");
    expect(amountCells.length).toBe(FIXTURE_TXN_COUNT);
    for (const cell of amountCells) expect(cell.text()).toBe("••••");
  });
});

describe("渲染期计数守门（初始阈值 = 建票时现状基线，收紧纪律见文件头注）", () => {
  it("① 同一数据全表重渲染：渲染期格式化调用 ≤ 基线", async () => {
    const wrapper = await mountAndFlush();
    // 计数窗口起点：清哨兵后以「重建列数组」触发同一行集的全表重渲染（纯 patch）。
    tSpy.mockClear();
    formatAmountSpy.mockClear();
    resolveLendingSpy.mockClear();
    await wrapper.setProps({ columns: buildFixtureColumns() });
    await flushPromises();
    expect(
      tSpy.mock.calls.length,
      `渲染期 t() 调用 ${tSpy.mock.calls.length} 次超出基线 ${RENDER_FORMAT_T_BASELINE}（渲染期逐格格式化回归？收紧纪律见文件头注 / #2012）`,
    ).toBeLessThanOrEqual(RENDER_FORMAT_T_BASELINE);
    expect(formatAmountSpy.mock.calls.length).toBeLessThanOrEqual(RENDER_FORMAT_AMOUNT_BASELINE);
    expect(resolveLendingSpy.mock.calls.length).toBeLessThanOrEqual(
      RENDER_RESOLVE_LENDING_BASELINE,
    );
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

  it("③ 表格容器内 DOM 节点总数 ≤ 基线", async () => {
    const wrapper = await mountAndFlush();
    const table = wrapper.find(".n-data-table");
    expect(table.exists()).toBe(true);
    expect(table.element.querySelectorAll("*").length).toBeLessThanOrEqual(
      TABLE_DOM_NODES_BASELINE,
    );
  });
});
