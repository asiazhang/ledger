// 翻页渲染性能守门——计数型 Vitest 门禁（issue #2012，关联 #2010 渲染优化主体）：
// 渲染性能在 Linux(WebKitGTK) 是体验敏感点（AGENTS.md 体验标尺），后续迭代
// （新列 / 新组件 / 新格式化逻辑）容易无声劣化；计时断言在 CI 共享 runner 上必然
// flaky、不可作 gate，本文件以**确定性计数指标**守「渲染期不做重复功」的形态：
//   ① 渲染期格式化调用（t / formatAmount / resolveLendingDirection）——同一数据
//      全表重渲染窗口内的调用次数 ≤ 阈值；#2010 文本预计算落地后本项应收紧到 0
//      （预计算在数据到达时完成、不在 patch 期逐格执行，即 #2010 的核心形态）。
//   ② 行内交互组件实例数（AccountLink / MerchantLink / SourceLink /
//      AmountCell / NTag / NEllipsis）≤ 阈值（NoteCopyButton 组件随 #2010 退役）。
//   ③ 表格容器内 DOM 节点总数 ≤ 阈值。
// 计数窗口 = 「同一行集 + 重建列数组」的全表重渲染（列对象引用更新 → 每格
// DataTableCell 组件收到新列对象而重渲染；列数组本身在窗口外重建，见①用例）：
// 不含数据到达路径——预计算在数据到达时完成（withTransactionCells），不计入窗口；
// 若格式化被改回渲染期逐格执行（#2010 接线被移除的形态），窗口内计数立即越过
// 收紧后的 0 基线即红（删除即红，ADR-0087 断言强度）。
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

// —— ① 渲染期格式化调用基线（#2010 落地后收紧到 0）——
// 建票基线（#2010 前）134/50/10：逐格渲染期格式化 + 行内组件取词。#2010 文本预计算
// 把全部取词/格式化前移到数据到达时（withTransactionCells，计数窗口外）与列构建期
//（列名/悬停提示/无障碍标签，列数组重建在窗口外执行）；重渲染窗口内格式化调用为 0
// 是 #2010 的核心形态，任何回归（>0）即红。
const RENDER_FORMAT_T_BASELINE = 0;
const RENDER_FORMAT_AMOUNT_BASELINE = 0;
const RENDER_RESOLVE_LENDING_BASELINE = 0;

// —— ② 行内交互组件实例数基线（#2010 落地后收紧到 0）——
// 建票基线（#2010 前）：AccountLink 60、MerchantLink 10、SourceLink 15、
// NoteCopyButton 20、AmountCell 50、NTag 52、NEllipsis 162。#2010 平替渲染后桌面
// 表格（指针轴）行内零组件实例：链接/标签/省略/复制按钮全部纯元素化，AmountCell
// 仅触控轴渲染（本 fixture 面向指针轴；触控轴形态归 TransactionsView 两轴测试）。
// NoteCopyButton 组件随 #2010 整体退役（复制语义迁 transaction-table-actions，
// 行为等价测试在彼处），不再列入计数面。
const ACCOUNT_LINK_BASELINE = 0;
const MERCHANT_LINK_BASELINE = 0;
const SOURCE_LINK_BASELINE = 0;
const AMOUNT_CELL_BASELINE = 0;
const N_TAG_BASELINE = 0;
const N_ELLIPSIS_BASELINE = 0;

// —— ③ 表格容器内 DOM 节点总数基线（.n-data-table 内全部元素节点）——
// 建票基线（#2010 前）1794；#2010 平替渲染（链接/标签/省略组件纯元素化）后实测
// 1259（组件包裹层与 NEllipsis/Tooltip 结构消失），按收紧纪律直接落新基线。
const TABLE_DOM_NODES_BASELINE = 1259;

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
    // 计数窗口 = 纯 patch：列数组在窗口**外**重建（列构建期的取词——列名/标签——是
    // 数据到达/语言切换的合法格式化时机，不属渲染期逐格执行），清哨兵后仅以列对象
    // 引用更新触发同一行集的全表重渲染。
    const rebuilt = buildFixtureColumns();
    tSpy.mockClear();
    formatAmountSpy.mockClear();
    resolveLendingSpy.mockClear();
    await wrapper.setProps({ columns: rebuilt });
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
