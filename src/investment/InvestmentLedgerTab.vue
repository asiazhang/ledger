<script setup lang="ts">
import {
  NButton,
  NButtonGroup,
  NDataTable,
  NEmpty,
  NTag,
  NSpace,
  useMessage,
  useThemeVars,
  type DataTableColumn,
  type DropdownOption,
  type PaginationProps,
} from "naive-ui";
import { computed, h, ref, watch } from "vue";
import { storeToRefs } from "pinia";
import { api } from "@ledger/api";
import { useLoadable } from "@ledger/loadable";
import { t } from "@ledger/i18n";
import { formatAmount, formatPrice, formatQuantity } from "@ledger/money";
import { sumFixedColumnWidths } from "@ledger/utils/table";
import { PAGE_SIZE_OPTIONS } from "@ledger/utils/pagination";
import { kindSemanticColor } from "@ledger/theme/semantic-colors";
import type { NullableDateRange } from "@ledger/utils/time-period";
import AppModal from "@ledger/ui-kit/AppModal.vue";
import AppSelect from "@ledger/ui-kit/AppSelect.vue";
import AppDropdown from "@ledger/ui-kit/AppDropdown.vue";
import TransactionForm from "@/transaction/TransactionForm.vue";
import AmountCell from "@/transaction/AmountCell.vue";
import ConvertDetail from "@/investment/ConvertDetail.vue";
import SplitDetail from "@/investment/SplitDetail.vue";
import DividendDetail from "@/investment/DividendDetail.vue";
import { useAppDialog } from "@/composables/useAppDialog";
import { useRowContextMenu } from "@ledger/row-context-menu";
import { useTransactionModalState } from "@ledger/transaction-modal-state";
import { buildRowMenuOptions } from "@/transaction/transaction-row-menu";
import { KIND_TAG_TYPE, rowActionsColumn } from "@/transaction/transaction-columns";
import { ledgerRowToModalRow } from "@/investment/ledger-row-modal";
import { instrumentDisplayLabel } from "@/investment/instrument-display-label";
import PinyinSelect from "@ledger/ui-kit/PinyinSelect.vue";
import QuickTimeRange from "@/components/QuickTimeRange.vue";
import { useInvestmentsSessionStore } from "@/investment/investments-session";
import { useReferenceStore } from "@/stores/reference";
import { useAppStore } from "@/stores/app";
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
 * （ADR-0008，「共 N 条」+ 页大小档位与主列表同构）+ 手动筛选三维（issue #1780：类型多选
 * （投资 kind 子集）/ 账户（涉及账户语义）/ 日期（时间范围快捷选择，ADR-0135 修订注记）；
 * 标的维度仅作 URL 下钻只读入口（?instrument=，无手动控件，比照主列表分类维度形态）。
 *
 * 行操作与主列表同权（ADR-0135 决策 4 / issue #1781）：编辑 buy/sell 与 convert/
 * split/dividend 只读详情复用交易弹窗族——弹窗编排 TransactionModalState（先取
 * 扩展明细再开窗、失败不开窗、慢取竞态守卫内化其中）新实例消费，弹窗组件
 * （TransactionForm / 三只读详情组件）原样复用，行经弹窗行适配投影
 * （ledger-row-modal 单点）进编排；软删走既有 delete_transaction（级联与码化
 * 守卫照常，不新增错误码）；行菜单选项组装复用 transaction-row-menu 单点，行
 * 激活闭集（buy/sell = 编辑、convert/split/dividend = 只读详情、refund = 无）
 * 单源 transactionKindActivation 经其判定，本页签无第二套判定。行右键与「⋯」
 * 常显列是同一 RowContextMenu 编排的两个入口（账户行先例）；触控轴无卡片双渲染
 * （ADR-0135 决策 8），「⋯」列即触控轴行菜单入口。
 *
 * 状态归宿（ADR-0094）：筛选全维与页码/页大小住投资页会话 store（会话内保留、
 * 冷启动回默认），本组件只读消费 + 经意图入口写入；请求发起、loading 与行数据
 * 归视图（主列表同构，ADR-0030 决策 6）。筛选维度实际变化翻页归零由 store 内化。
 *
 * 金额列走统一展示格式化（formatAmount：数字分组随界面语言、金额隐私掩码自动
 * 生效）并按 kind 语义色着色（kindSemanticColor，与主列表同源同件，ADR-0135 修订注记）；
 * 行投影不携带币种呈现（混合币种列表不暗示同币种），金额按数字呈现。
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
  // 其余三维（issue #1780）：非默认才携带；日期为双端成对（picker 形态即闭包）
  if (session.detailAccountId) filter.account_id = session.detailAccountId;
  if (session.detailInstrumentId) filter.instrument_id = session.detailInstrumentId;
  if (session.detailDateFrom) filter.from = session.detailDateFrom;
  if (session.detailDateTo) filter.to = session.detailDateTo;
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
  [
    () => session.detailPage,
    () => session.detailPageSize,
    () => session.detailKinds,
    () => session.detailAccountId,
    () => session.detailInstrumentId,
    () => session.detailDateFrom,
    () => session.detailDateTo,
  ],
  () => {
    void load();
  },
  { immediate: true },
);

// —— 行操作弹窗族（ADR-0135 决策 4 / issue #1781）：与主列表同权的复用接线 ——

const message = useMessage();
const dialog = useAppDialog();
// 主题 error 色：删除项经 DropdownOption props 着色（不硬编码色值，暗色模式自动适配）。
const themeVars = useThemeVars();

/**
 * 行操作弹窗编排：复用交易弹窗族深模块 TransactionModalState 新实例（意图/序号
 * 不与交易页串扰；弹窗组件复用、消费意图在本页签重接线，ADR-0135 决策 4 /
 * issue #1781）。编辑 buy/sell 的「先取买卖明细再开窗、失败不开窗」与 convert/
 * split/dividend 只读详情的「先取扩展明细再开窗」异步时序、慢取竞态守卫、
 * dividend 同步开窗全部内化在模块（弹窗组件明细取数不经视图）；非 detail/edit
 * 意图（create/refund/add-item）本页签不产生——明细行集闭包内 refund 不在场、
 * 记一笔入口随创建入口迁址票另接（issue #1782）。
 */
const { intent, seq, open: openModal, close: closeModal } = useTransactionModalState();

/** 只读详情意图（窄化）：非 detail 意图为 null；模板按 detail.kind 分派只读组件。 */
const detailIntent = computed(() => (intent.value?.type === "detail" ? intent.value : null));

/** 编辑弹窗：行经弹窗行适配投影进编排，取数时序内化在模块（取数不经视图）。 */
function openEditFromRow(row: InvestmentTransactionRow) {
  void openModal({ type: "edit", row: ledgerRowToModalRow(row) });
}

/** 只读详情弹窗（界面只读 kind 不体现写操作入口，ADR-0106 决策 10 / ADR-0109）。 */
function openDetailFromRow(row: InvestmentTransactionRow) {
  void openModal({ type: "detail", row: ledgerRowToModalRow(row) });
}

/** 编辑成功：关窗（编排内化关闭意图）并以当前页码重拉列表（保持当前页与筛选，
 * 不重置页码——与主列表 onEditSaved 同构，不经翻回第 1 页语义）。 */
function onEditSaved() {
  closeModal();
  void load();
}

/** 软删目标行 id（0 元闭包任务的自读参数，Loadable 纪律：发起动作进、终态出）。 */
let removingId = "";

/** 软删走既有 delete_transaction（issue #151 二次确认同款，issue #1781 照常）：
 * 取消不删、遮罩点击不构成关闭意图；确认后才删除。既有级联与码化守卫（部分卖出
 * 守卫、转换链守卫、在用占用判定）在后端随命令自然生效，不新增错误码。错误反馈
 * 走 Loadable 统一通道（error 置位 + 统一 toast 单点，#1008：删除请求与列表请求
 * 同一异步守门基线，不发 catch 直弹分支）。 */
const { run: runRemove } = useLoadable(async () => {
  await api.deleteTransaction(removingId);
});

/** 软删成功：成功提示 + 以当前状态重拉（本页删后剩 0 条且非第 1 页由 load 内的
 * 空页自愈回退一页（issue #893 同款），与主列表 afterRowDelete 同一可观察结果）。 */
async function remove(id: string) {
  removingId = id;
  const ok = await runRemove();
  if (ok === null) return; // 失败：error 置位 + 统一 toast 已弹（Loadable 单点）
  message.success(t("transactions.list.deleted"));
  await load();
}

/** 删除走 useAppDialog 二次确认（issue #151）：取消不删，确认后才删除。 */
function confirmDelete(row: InvestmentTransactionRow) {
  dialog.warning({
    title: t("transactions.deleteDialog.title"),
    content: t("transactions.deleteDialog.content"),
    positiveText: t("transactions.deleteDialog.confirm"),
    negativeText: t("transactions.deleteDialog.cancel"),
    maskClosable: false,
    onPositiveClick: () => remove(row.id),
  });
}

/** 行右键菜单 + 「⋯」常显列（与主列表同构，issue #151 / #550 / #843）：除 refund 外
 * 可编辑行首项「编辑」、界面只读 kind 仅「详情」、其余行含「删除」——选项组装
 * 复用 transaction-row-menu 单点（行激活闭集单源 transactionKindActivation 经其
 * 判定，convert/split/dividend 行不出现编辑/软删入口），业务动作分派留本页签。 */
const rowMenu = useRowContextMenu<InvestmentTransactionRow>((key, row) => {
  if (key === "detail") openDetailFromRow(row);
  else if (key === "edit") openEditFromRow(row);
  else if (key === "delete") confirmDelete(row);
});

// 可见性由单判别状态派生（非空即显示）；定位坐标取工厂保留值（issue #798 同款，
// 离场动画期间仍按坐标重定位）。
const menuShow = computed(() => rowMenu.state.value !== null);
const menuX = computed(() => rowMenu.position.value.x);
const menuY = computed(() => rowMenu.position.value.y);

/** 菜单选项：选项组装单点复用（hasItem 维度仅 expense 行消费，投资 kind 行集不在场）。 */
const menuOptions = computed<DropdownOption[]>(() => {
  const row = rowMenu.state.value?.row;
  return row ? buildRowMenuOptions(row, { errorColor: themeVars.value.errorColor }) : [];
});

/** 表格行属性：绑定行右键菜单（open 内化重定位舞步；原生菜单拦截单点归窗口行为守卫）。 */
const rowProps = (row: InvestmentTransactionRow) => ({
  onContextmenu: (e: MouseEvent) => rowMenu.open(e, row),
});

/** 弹窗经 ✕ / ESC 显式关闭：走编排内化关闭（意图清回空终态）。 */
function onModalShowUpdate(show: boolean) {
  if (!show) closeModal();
}

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

/**
 * 账户筛选（issue #1780，涉及账户语义——账户端 ∪ 出资端）：候选 = 投资类账户，
 * 下拉候选面取参考 store 单点投影（investmentAccountOptions，#1830，同源消费面
 * 见 store 注记）；后端 account_id 命中账户端或出资端（dividend 到账账户、
 * buy/sell 出资账户），语义单点在后端——投资账户不在出资闭集，下拉候选只会
 * 经账户端命中；非投资类出资/到账账户的筛选入口在交易页账户筛选（全量候选）。
 */
const { investmentAccountOptions: accountOptions } = storeToRefs(reference);

function onAccountFilterChange(id: string | null) {
  session.setDetailAccount(id);
}

/**
 * 日期筛选（ADR-0135 修订注记）：时间范围快捷选择（QuickTimeRange）受控承载——
 * 快照区间 v-model 进出，组件不持状态源，唯一事实源是会话 store 明细日期维度
 * （from/to 成对桥接，与主列表日期维度同构）；快照语义与数据期间边界钳制由
 * 组件继承，本层零日期数学。
 */
const quickRange = computed<NullableDateRange>({
  get: () => ({ from: session.detailDateFrom, to: session.detailDateTo }),
  set: (range) =>
    session.setDetailDateRange(
      range.from !== null && range.to !== null ? [range.from, range.to] : null,
    ),
});

/** 是否有激活筛选（控制空态文案分型：无筛选空态 vs 筛选无匹配）。 */
const filtersActive = computed(
  () =>
    session.detailKinds !== null ||
    session.detailAccountId !== null ||
    session.detailInstrumentId !== null ||
    session.detailDateFrom !== null ||
    session.detailDateTo !== null,
);

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
  return instrumentDisplayLabel(row.symbol, row.instrument_name);
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
    // 类型标签色与主列表同源（KIND_TAG_TYPE 单点，ADR-0135 修订注记）
    render: (row) =>
      h(NTag, { type: KIND_TAG_TYPE[row.kind] }, () => t(`transactions.kind.${row.kind}`)),
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
    // 金额语义色与主列表同源（kindSemanticColor 单点，随主题响应式；ADR-0135 修订注记）
    render: (row) =>
      h(AmountCell, {
        text: amountCell(row),
        color: kindSemanticColor(row.kind, useAppStore().theme),
      }),
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
  // 交易行「⋯」常显操作列（与主列表同构，ADR-0088 决策 6 / issue #843 / #1781）：
  // 与行右键共用同一行菜单编排 open 入口、以点击坐标弹出；明细页签单表格渲染
  //（无卡片双渲染），本列即触控轴的行菜单入口（列形态单点 rowActionsColumn）。
  rowActionsColumn<InvestmentTransactionRow>((e, row) => rowMenu.open(e, row)),
]);

// 横向滚动下限 = 各固定列宽总和（全仓单一收口）：标的列为唯一弹性列（minWidth
// 不计入），窄窗口/移动档由横向滚动吸收、列不压碎（不做卡片双渲染，ADR-0135 决策 8）。
const scrollX = computed(() => sumFixedColumnWidths(columns.value));

/** 服务端分页（ADR-0008）：页码/页大小/总数归 store 与响应，表格 remote 不切片；
 * 「共 N 条」前缀与主列表同构；页大小档位与主列表同源（PAGE_SIZE_OPTIONS 单点，#1792）。 */
const pagination = computed<PaginationProps>(() => ({
  page: session.detailPage,
  pageSize: session.detailPageSize,
  itemCount: total.value,
  showSizePicker: true,
  pageSizes: PAGE_SIZE_OPTIONS,
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

// —— 创建入口（页签头部记买入/卖出，ADR-0135 决策 5 / issue #1782）——

/**
 * 创建弹窗编排：复用交易弹窗族深模块 TransactionModalState 的记一笔意图（type=create
 * 携带 kind）——全仓弹窗开启编排的唯一形态上，投资页重接线（ADR-0135 决策 4）。类型由
 * 入口单点表达（头部两个按钮），弹窗内不提供切换，中途换类型 = 关闭重开。
 * 功能开关关闭投资时投资页整页不可达（ADR-0116 决策 4 修订注记），入口语义由整页覆盖，
 * 无需逐 kind 闸门。
 * 与行操作弹窗族（issue #1781）各持 TransactionModalState 新实例，意图/序号互不串扰。
 */
const {
  intent: rawCreateIntent,
  seq: createSeq,
  open: openCreateModal,
  close: closeCreateModal,
} = useTransactionModalState();

/** 创建意图（窄化）：非 create 意图为 null；模板按意图派生显示开关与标题。 */
const createIntent = computed(() =>
  rawCreateIntent.value?.type === "create" ? rawCreateIntent.value : null,
);

/**
 * 明细页签头部入口可创建类型闭集 = buy/sell（ADR-0135 决策 5；convert/split/dividend
 * 无现金腿 kind 无手工录入入口，ADR-0106 决策 10 / #1048）。类型由入口单点表达，
 * openCreate 直发记一笔意图。
 */
function openCreate(kind: "buy" | "sell") {
  void openCreateModal({ type: "create", kind });
}

/** 弹窗标题：标明入口选定类型（交易弹窗族同一文案单点）。 */
const createTitle = computed(() =>
  createIntent.value
    ? t("transactions.create.titleWithKind", {
        kind: t(`transactions.kind.${createIntent.value.kind}`),
      })
    : t("transactions.create.title"),
);

/**
 * 提交成功：关窗（模块意图清回空终态）并刷新明细列表——新记录按 date 倒序最可能落在
 * 第 1 页，翻回第 1 页重拉（主列表 onFormCreated 的 refresh 同语义）：页码变化经 store
 * 意图入口触发既有重拉出口；已在第 1 页时直接重拉（单一请求，不走翻页语义）。
 */
function onCreated() {
  closeCreateModal();
  if (session.detailPage !== 1) {
    session.setDetailPage(1);
  } else {
    void load();
  }
}
</script>

<template>
  <NSpace vertical :size="12">
    <!-- 手动筛选三维 + 标的下钻只读入口（ADR-0135 决策 3 及其修订注记 / issue #1780 / #1807）：
         类型多选（投资 kind 子集）+ 账户（涉及账户语义）+ 日期（时间范围快捷选择）；
         标的维度无手动控件，仅 ?instrument= 深链落账。立即生效，翻页归零由 store 内化。 -->
    <NSpace :size="8" align="center" :wrap="true" justify="space-between">
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
      <PinyinSelect
        :value="session.detailAccountId"
        :options="accountOptions"
        :placeholder="t('investments.ledger.filterAccount')"
        clearable
        style="width: 180px"
        data-testid="ledger-account-filter"
        @update:value="onAccountFilterChange"
      />
      <!-- 时间范围快捷选择（ADR-0135 修订注记）：五芯片（含「全部」）+ 期间步进器 +
           期间直达面板，受控桥接会话 store 明细日期维度；弹层上报由面板内
           AppDatePicker 承担（Overlay Suppression）。 -->
      <QuickTimeRange v-model="quickRange" data-testid="ledger-date-filter" />
      <!-- 清除筛选（主列表同款判定，#1807）：任一明细维度激活（含深链带入的标的
           维度）即可用；清明细筛选 + 翻页归零，不切页签、不动页大小。 -->
      <NButton
        size="tiny"
        quaternary
        type="primary"
        :disabled="!filtersActive"
        data-testid="ledger-clear-filters"
        @click="session.resetDetailFilters()"
      >
        {{ t("transactions.filter.clear") }}
      </NButton>
      <!-- 创建入口（ADR-0135 决策 5 / issue #1782）：买入/卖出的记一笔入口落页签头部，
           类型由入口单点表达；提交走既有创建编排与 TransactionInput 装配接缝 -->
      <NButtonGroup>
        <NButton type="primary" @click="openCreate('buy')">
          {{ t("investments.ledger.createBuy") }}
        </NButton>
        <NButton type="primary" @click="openCreate('sell')">
          {{ t("investments.ledger.createSell") }}
        </NButton>
      </NButtonGroup>
    </NSpace>
    <NDataTable
      :columns="columns"
      :data="rows"
      :loading="loading"
      :bordered="false"
      size="small"
      remote
      :row-key="(r: InvestmentTransactionRow) => r.id"
      :row-props="rowProps"
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
    <!-- 创建弹窗（记买入/记卖出共用一枚）：标题标明入口选定类型，内嵌 TransactionForm
         （按 kind 分派 InvestmentForm，基金金额权威/非基金单价权威，零表单内部改造）；
         提交成功关窗并刷新明细列表（翻回第 1 页）；显示开关由模块意图派生，序号作表单 key
         强制重建（ADR-0045 既有编排，投资页重接线） -->
    <AppModal
      :show="createIntent !== null"
      :title="createTitle"
      preset="card"
      display-directive="if"
      card-size="md"
      @update:show="
        (show: boolean) => {
          if (!show) closeCreateModal();
        }
      "
    >
      <TransactionForm
        v-if="createIntent"
        :key="createSeq"
        :kind="createIntent.kind"
        @created="onCreated"
      />
    </AppModal>
  </NSpace>
  <!-- 行右键菜单（与主列表同构，issue #151 / #550）：手动定位弹出；开合上报经薄封装
       attrs watch 自动生效（`:show` 绑定照旧）。 -->
  <AppDropdown
    trigger="manual"
    placement="bottom-start"
    :show="menuShow"
    :x="menuX"
    :y="menuY"
    :options="menuOptions"
    style="max-width: 140px"
    @select="rowMenu.select"
    @clickoutside="rowMenu.close"
  />
  <!-- 编辑弹窗（ADR-0135 决策 4 / issue #1781）：buy/sell 行经弹窗行适配进编排，
       回填既有交易全部业务字段（明细回填经 TransactionTrade 投影），kind 锁死；提交
       走全字段更新命令，成功关窗并保持当前页重拉。开启/关闭经 TransactionModalState
       编排（目标行与买卖明细由意图携带，序号作表单 key 强制重建）。 -->
  <AppModal
    :show="intent?.type === 'edit'"
    :title="t('transactions.edit.title')"
    preset="card"
    display-directive="if"
    card-size="md"
    @update:show="onModalShowUpdate"
  >
    <TransactionForm
      :key="seq"
      v-if="intent?.type === 'edit'"
      :editing="intent.row"
      :trade="intent.trade"
      @saved="onEditSaved"
    />
  </AppModal>
  <!-- 只读详情弹窗（ADR-0106 决策 10 / ADR-0109）：界面只读 kind（convert / split /
       dividend）不体现任何写操作；convert 只读呈现「A → B」两侧标的、份额、金额、
       手续费与结转成本，split 只读呈现标的、带符号份额变动、调整日与账户，dividend
       只读呈现归属标的、金额、到账账户与日期。扩展明细取数时序内化在编排模块 -->
  <AppModal
    :show="intent?.type === 'detail'"
    :title="t('transactions.detail.title')"
    preset="card"
    display-directive="if"
    card-size="md"
    @update:show="onModalShowUpdate"
  >
    <ConvertDetail
      :key="seq"
      v-if="detailIntent?.detail.kind === 'convert'"
      :transaction="detailIntent.row"
      :convert="detailIntent.detail.convert"
    />
    <SplitDetail
      :key="seq"
      v-else-if="detailIntent?.detail.kind === 'split'"
      :transaction="detailIntent.row"
      :split="detailIntent.detail.split"
    />
    <DividendDetail
      :key="seq"
      v-else-if="detailIntent?.detail.kind === 'dividend'"
      :transaction="detailIntent.row"
    />
  </AppModal>
</template>
