<script setup lang="ts">
import { NDataTable, NEmpty, NSpace, type DataTableColumn, type PaginationProps } from "naive-ui";
import { computed, h, ref, watch } from "vue";
import { api } from "@ledger/api";
import { useLoadable } from "@ledger/loadable";
import { t } from "@ledger/i18n";
import { formatAmount, formatPrice, formatQuantity } from "@ledger/money";
import { sumFixedColumnWidths } from "@ledger/utils/table";
import AppSelect from "@ledger/ui-kit/AppSelect.vue";
import {
  LEDGER_TAB_PAGE_SIZE_OPTIONS,
  useInvestmentsSessionStore,
} from "@/investment/investments-session";
import { useReferenceStore } from "@/stores/reference";
import type {
  InvestmentTransactionListFilter,
  InvestmentTransactionRow,
  TransactionKind,
} from "@ledger/types";

/**
 * 投资明细页签（Investment Ledger Tab，ADR-0135 决策 3 / issue #1779 基座）：五种投资
 * kind 交易行的投资投影列表——消费后端投资明细命令（issue #1778），按 kind 分
 * 形态渲染（buy/sell 标的/数量/单价/手续费/出资账户、convert「A → B」双腿、
 * split 带符号份额增量 Δ、dividend 现金腿与到账账户）；服务端 offset 分页
 * （ADR-0008，「共 N 条」+ 页大小档位与主列表同构）+ 类型多选筛选（投资 kind
 * 子集）。
 *
 * 状态归宿（ADR-0094）：类型筛选与页码/页大小住投资页会话 store（会话内保留、
 * 冷启动回默认），本组件只读消费 + 经意图入口写入；请求发起、loading 与行数据
 * 归视图（主列表同构，ADR-0030 决策 6）。筛选维度实际变化翻页归零由 store 内化。
 *
 * 金额列走统一展示格式化（formatAmount：数字分组随界面语言、金额隐私掩码自动
 * 生效）；行投影不携带币种（混合币种列表不暗示同币种），金额按数字呈现。
 */

/** 明细页签类型筛选闭集 = 行集闭包：五种投资 kind（与后端 LEDGER_TAB_KINDS 同一闭集）。 */
const LEDGER_TAB_KINDS: TransactionKind[] = ["buy", "sell", "convert", "split", "dividend"];

const session = useInvestmentsSessionStore();
const reference = useReferenceStore();

const rows = ref<InvestmentTransactionRow[]>([]);
const total = ref(0);

/**
 * 列表请求（ADR-0030 决策 6：请求发起、loading、行数据归视图；错误反馈走
 * Loadable 统一通道——error 置位 + 统一 toast 单点，#1008 异步守门基线）：以
 * 会话 store 当前状态装配请求参数并发起查询。页码钳制（issue #893 同款自愈）：
 * 空页 + 尚有数据 + 非第一页 → 回退一页重拉，本响应不落数据（不渲染空页）。
 */
const { loading, run } = useLoadable(async () => {
  const filter: InvestmentTransactionListFilter = {
    page: session.detailPage,
    page_size: session.detailPageSize,
  };
  // 类型维度：非空集合才携带（空集合 ≡ 不过滤 ≡ 默认态，浅拷贝脱只读）
  if (session.detailKinds?.length) filter.kinds = [...session.detailKinds];
  return api.listInvestmentTransactions(filter);
});

async function load() {
  // 失败回空（error 已置位 + 统一 toast）：rows/total 保持原值不清空成空态
  const res = await run();
  if (res === null) return;
  if (res.items.length === 0 && res.total > 0 && session.detailPage > 1) {
    session.setDetailPage(session.detailPage - 1);
    return;
  }
  rows.value = res.items;
  total.value = res.total;
}

// 首拉与重拉唯一触发点：明细状态（页码/页大小/类型筛选）任一变化即以当前状态
// 重拉；首拉 immediate 承担——默认态以默认态拉取，恢复访次以保留态拉取（会话
// 内保留语义，页签 display-directive 'if' 重挂后由 store 恢复）。
watch(
  [() => session.detailPage, () => session.detailPageSize, () => session.detailKinds],
  () => {
    void load();
  },
  { immediate: true },
);

/** 类型多选选项：投资 kind 闭集（按闭集顺序渲染），标签复用交易域 kind 文案单点。 */
const kindOptions = computed<Array<{ label: string; value: TransactionKind }>>(() =>
  LEDGER_TAB_KINDS.map((value) => ({
    label: t(`transactions.kind.${value}`),
    value,
  })),
);

/** 类型多选值：脱只读投影 + 按闭集顺序渲染（手动选择序不作展示序，主列表同构）；
 * 空集合归一为 null（≡ 不过滤）。 */
const kindValue = computed<TransactionKind[] | null>(() => {
  if (!session.detailKinds?.length) return null;
  const rank = new Map(LEDGER_TAB_KINDS.map((k, i) => [k, i]));
  return [...session.detailKinds].sort((a, b) => (rank.get(a) ?? 0) - (rank.get(b) ?? 0));
});

/** 类型筛选意图：空数组归一为 null（空集合 ≡ 不过滤 ≡ 默认态）；翻页归零由
 * store 内化（实际变化才归零）。 */
function onKindFilterChange(values: TransactionKind[] | null) {
  session.setDetailKinds(values?.length ? values : null);
}

/** 是否有激活筛选（控制空态文案分型：无筛选空态 vs 筛选无匹配）。 */
const filtersActive = computed(() => session.detailKinds !== null);

// —— 行单元格（按 kind 分形态，ADR-0135 决策 3）——

/** 账户名：账户端（buy/sell/convert/split 投资账户；dividend 到账账户）。 */
function accountName(row: InvestmentTransactionRow): string {
  return reference.accountMap.get(row.account_id)?.name ?? "-";
}

/** 出资账户名（仅 buy/sell 可携带，ADR-0096；未携带渲染「-」）。 */
function fundingAccountName(row: InvestmentTransactionRow): string {
  return row.funding_account_id
    ? (reference.accountMap.get(row.funding_account_id)?.name ?? "-")
    : "-";
}

/** 标的单元格：convert 行「A → B」双腿（转出 → 转入代码）；其余行代码 + 名称。 */
function instrumentCell(row: InvestmentTransactionRow): string {
  if (row.convert) return `${row.symbol} → ${row.convert.to_symbol}`;
  return row.instrument_name ? `${row.symbol} ${row.instrument_name}` : row.symbol;
}

/** 数量单元格：buy/sell 成交数量；convert「转出 → 转入」份额双腿；split 带符号
 * 增量 Δ（+ 送股/折算、− 缩股，原样呈现不取绝对值）；dividend 无数量「-」。 */
function quantityCell(row: InvestmentTransactionRow): string {
  if (row.trade) return formatQuantity(row.trade.quantity);
  if (row.convert)
    return `${formatQuantity(row.convert.quantity)} → ${formatQuantity(row.convert.to_quantity)}`;
  if (row.split) {
    const delta = row.split.delta_quantity;
    return delta > 0 ? `+${formatQuantity(delta)}` : formatQuantity(delta);
  }
  return "-";
}

/** 金额单元格（分）：buy/sell/dividend 读行金额锚点；convert 展示金额读转换
 * 载荷转出金额（与主列表同口径，行金额锚点是结转成本）；split 无现金腿「-」。 */
function amountCell(row: InvestmentTransactionRow): string {
  if (row.convert) return formatAmount(row.convert.out_amount_cents);
  if (row.split) return "-";
  return formatAmount(row.amount_cents);
}

// 列形态遵循词汇表「表格列形态」约定：数值列右对齐 + 等宽数字（全局工具类
// tabular-nums 单点挂载），标的/账户长文本列弹性 + 单行 ellipsis 悬停全文，
// 短内容列按内容定宽（日期列与主列表同宽 105）。
const columns = computed<DataTableColumn<InvestmentTransactionRow>[]>(() => [
  {
    title: t("investments.ledger.columns.date"),
    key: "date",
    width: 105,
  },
  {
    title: t("investments.ledger.columns.kind"),
    key: "kind",
    width: 90,
    render: (r) => t(`transactions.kind.${r.kind}`),
  },
  {
    title: t("investments.ledger.columns.instrument"),
    key: "instrument",
    // 弹性列：不设固定宽，独吃窗口剩余宽度；minWidth 保窄窗口下限
    minWidth: 160,
    ellipsis: { tooltip: true },
    render: instrumentCell,
  },
  {
    title: t("investments.ledger.columns.quantity"),
    key: "quantity",
    width: 130,
    align: "right",
    className: "tabular-nums",
    render: quantityCell,
  },
  {
    title: t("investments.ledger.columns.amount"),
    key: "amount",
    width: 120,
    align: "right",
    className: "tabular-nums",
    render: amountCell,
  },
  {
    title: t("investments.ledger.columns.price"),
    key: "price",
    width: 110,
    align: "right",
    className: "tabular-nums",
    // 成交单价（万分之一元刻度，ADR-0038），价格列专用 formatPrice
    render: (r) => (r.trade ? formatPrice(r.trade.price_cents) : "-"),
  },
  {
    title: t("investments.ledger.columns.fee"),
    key: "fee",
    width: 100,
    align: "right",
    className: "tabular-nums",
    render: (r) => (r.trade ? formatAmount(r.trade.fee_cents) : "-"),
  },
  {
    // 账户端（dividend 行即到账账户）
    title: t("investments.ledger.columns.account"),
    key: "account",
    width: 110,
    ellipsis: { tooltip: true },
    render: accountName,
  },
  {
    // 出资账户（仅 buy/sell 可携带，ADR-0096）
    title: t("investments.ledger.columns.fundingAccount"),
    key: "funding_account",
    width: 110,
    ellipsis: { tooltip: true },
    render: fundingAccountName,
  },
]);

// 横向滚动下限 = 各固定列宽总和（全仓单一收口）：标的列为唯一弹性列（minWidth
// 不计入），窄窗口/移动档由横向滚动吸收、列不压碎（不做卡片双渲染，ADR-0135 决策 8）。
const scrollX = computed(() => sumFixedColumnWidths(columns.value));

/** 服务端分页（ADR-0008）：页码/页大小/总数归 store 与响应，表格 remote 不切片；
 * 「共 N 条」前缀与页大小档位（[10, 20, 50, 100]）与主列表同构。 */
const pagination = computed<PaginationProps>(() => ({
  page: session.detailPage,
  pageSize: session.detailPageSize,
  itemCount: total.value,
  showSizePicker: true,
  pageSizes: LEDGER_TAB_PAGE_SIZE_OPTIONS,
  prefix: ({ itemCount }) =>
    h(
      "span",
      { "data-testid": "ledger-total" },
      t("investments.ledger.total", { n: itemCount ?? 0 }),
    ),
  onChange: (page: number) => session.setDetailPage(page),
  onUpdatePageSize: (pageSize: number) => session.setDetailPageSize(pageSize),
}));

/** 空态文案（两型同源）：筛选无结果提示 / 默认暂无数据（主列表同构）。 */
const emptyDescription = computed(() =>
  filtersActive.value ? t("investments.ledger.emptyFiltered") : t("investments.ledger.empty"),
);
</script>

<template>
  <NSpace vertical :size="12">
    <!-- 类型多选筛选（投资 kind 子集，ADR-0135 决策 3）：立即生效，翻页归零由
         store 内化；本基座票仅类型一维（账户/标的/日期随后续票接入）。 -->
    <NSpace :size="8" align="center" :wrap="true">
      <AppSelect
        :value="kindValue"
        :options="kindOptions"
        :placeholder="t('investments.ledger.filterKind')"
        multiple
        clearable
        :max-tag-count="1"
        style="width: 160px"
        data-testid="ledger-kind-filter"
        @update:value="onKindFilterChange"
      />
    </NSpace>
    <NDataTable
      :columns="columns"
      :data="rows"
      :loading="loading"
      :bordered="false"
      size="small"
      remote
      :row-key="(r: InvestmentTransactionRow) => r.id"
      :scroll-x="scrollX"
      :pagination="pagination"
    >
      <!-- 空态：筛选无结果与暂无数据分型文案；加载期间不渲染空态节点（与加载态
           区分，主列表移动档同款 v-if 形态） -->
      <template #empty>
        <NEmpty
          v-if="!loading"
          :description="emptyDescription"
          size="small"
          data-testid="ledger-empty"
        />
      </template>
    </NDataTable>
  </NSpace>
</template>
