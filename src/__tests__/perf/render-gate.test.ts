// 翻页渲染性能守门——计数型 Vitest 门禁（issue #2012，关联 #2010 渲染优化主体）：
// 渲染性能在 Linux(WebKitGTK) 是体验敏感点（AGENTS.md 体验标尺），后续迭代
// （新列 / 新组件 / 新格式化逻辑）容易无声劣化；计时断言在 CI 共享 runner 上必然
// flaky、不可作 gate，本文件以**确定性计数指标**守「渲染期不做重复功」的形态：
//   ① 渲染期格式化调用（t / formatAmount / resolveLendingDirection）——同一数据
//      全表重渲染窗口内的调用次数 = 0（#2188 文本预计算落地后的核心形态）：
//      预计算在数据到达/上下文变化时完成、不在 patch 期逐格执行；格式化被改回
//      渲染期逐格执行（#2188 接线被移除的形态）即红（删除即红，ADR-0087 断言强度）。
//   ② 行内重型组件实例数（AccountLink / MerchantLink / SourceLink /
//      NoteCopyButton / AmountCell / NTag / NEllipsis）——指针轴下收紧到 0
//      （issue #2189 行内高频组件瘦身：类型/操作/复制/金额/账户/商户/来源格全部
//      轻量原生元素 + 表级事件委托；任一列回退逐格重型组件即红）。
//   ③ 表格容器内 DOM 节点总数 ≤ 阈值（50 行与 100 行两档）。
// 计数窗口 = 「同一行集 + 重建列数组」的纯 patch 阶段：列数组在窗口外先构建
//（列标题/无障碍标签取词属数据/上下文到达阶段，同视图 computed 重建列的时点），
// mockClear 后仅 setProps 触发列对象引用更新 → 全部单元格 patch。渲染函数若逐格
// 调用格式化（退回 #2188 前形态）即在窗口内留痕变红；列构建期的取词不在窗口内。
// 阈值纪律：优化落地时收紧到优化后基线（前后差即量化收益记录）；
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

// —— ① 渲染期格式化调用（#2188 预计算落地后收紧到 0）——
// 渲染函数只读预计算 display 字段；列构建期取词（列标题/操作列无障碍标签）在
// 窗口外（先构建后 clear）。删除预计算接线（渲染函数回落逐格格式化）即红。
const RENDER_FORMAT_T_BASELINE = 0;
const RENDER_FORMAT_AMOUNT_BASELINE = 0;
const RENDER_RESOLVE_LENDING_BASELINE = 0;

// —— ② 行内重型组件实例数基线（issue #2189 收紧到 0）——
// 指针轴下全部高频格为轻量原生元素：类型标签 = 语义样式元素（tag-colors）、
// 账户/商户/来源 = 原生按钮/占位文本（data-tx-action 委托）、备注复制 = 原生
// 按钮 + 共享静态图标、金额 = 轻量文本（AmountCell 仅触控轴）。删除任一列的
// 瘦身接线（回退逐格重型组件实例）即本组断言变红（删除即红）。
const ACCOUNT_LINK_BASELINE = 0;
const MERCHANT_LINK_BASELINE = 0;
const SOURCE_LINK_BASELINE = 0;
const NOTE_COPY_BUTTON_BASELINE = 0;
const AMOUNT_CELL_BASELINE = 0;
const N_TAG_BASELINE = 0;
// NEllipsis（同步省略，每格同步实例化完整提示弹层子树）#2188 后仅剩表头省略 2：
// 分类/商户列头（列头仍走同步 Ellipsis，naive-ui Header 不消费 performant 变体）。
// 表格分类/商户列已改列级懒挂载（performant-ellipsis）、备注/商品名单元格内已改
// NPerformantEllipsis（初始纯 span，悬停才挂弹层）——退回同步形态即红。
const N_ELLIPSIS_BASELINE = 2;

// —— ③ 表格容器内 DOM 节点总数基线（.n-data-table 内全部元素节点）——
// 50 行档：#2189 收紧（类型/账户/商户/来源/备注复制/金额格轻量化 + 事件委托去
// 逐格绑定，#2188 优化后基线 1794 → 1422）。
const TABLE_DOM_NODES_BASELINE = 1422;
// 100 行档（主列表页大小上限档，issue #2189 新增覆盖）：100 笔交易 + 购买项展开
// = 150 显示行，阈值按同形态线性外推并实测锚定。
const TABLE_DOM_NODES_100_BASELINE = 2787;

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
    // 开关是展示格式化层模块级单点，用例结尾复原避免污染同文件后续用例。
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
    // 预热性全表重渲染：吸收任何待结算的响应式状态（如前一用例遗留的上下文变化
    // 触发的预计算重派生），使计数窗口从已结算状态开始——预计算重派生属
    // 「数据到达/上下文变化」路径，不计入窗口；随后以「重建列数组」触发
    // 同一行集的纯 patch（窗口正式起点）。
    await wrapper.setProps({ columns: buildFixtureColumns() });
    await flushPromises();
    // 计数窗口起点：列数组在窗口外先构建（列标题/无障碍标签取词属数据/上下文
    // 到达阶段，同视图 computed 重建列时点），清哨兵后仅以「重建列数组」触发
    // 同一行集的全表重渲染（纯 patch）。
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
    // 行内重型组件实例（指针轴）：与 50 行档同收 0——翻页换档不引入组件回潮
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
    // DOM 节点总数 ≤ 100 行档基线
    const table = wrapper.find(".n-data-table");
    expect(table.exists()).toBe(true);
    const count = table.element.querySelectorAll("*").length;
    expect(count).toBeLessThanOrEqual(TABLE_DOM_NODES_100_BASELINE);
  });
});
