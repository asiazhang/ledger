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
import OrderDetail from "@/transaction/OrderDetail.vue";
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
import { usePinnedAccountOptions } from "@/composables/useFormShared";
import { useReferenceStore } from "@/stores/reference";
import { useAppStore } from "@/stores/app";
import type {
  InvestmentTransactionListFilter,
  InvestmentTransactionRow,
  TransactionKind,
} from "@ledger/types";

const LEDGER_TAB_KINDS: TransactionKind[] = ["buy", "sell", "convert", "split", "dividend"];

const session = useInvestmentsSessionStore();
const reference = useReferenceStore();

const rows = ref<InvestmentTransactionRow[]>([]);
const total = ref(0);

const { loading, run } = useLoadable(async () => {
  const filter: InvestmentTransactionListFilter = {
    page: session.detailPage,
    page_size: session.detailPageSize,
  };
  if (session.detailKinds?.length) filter.kinds = [...session.detailKinds];
  if (session.detailAccountId) filter.account_id = session.detailAccountId;
  if (session.detailInstrumentId) filter.instrument_id = session.detailInstrumentId;
  if (session.detailDateFrom) filter.from = session.detailDateFrom;
  if (session.detailDateTo) filter.to = session.detailDateTo;
  return api.listInvestmentTransactions(filter);
});

async function load() {
  const res = await run();
  if (res === null) return;
  if (res.items.length === 0 && res.total > 0 && session.detailPage > 1) {
    session.setDetailPage(session.detailPage - 1);
    return;
  }
  rows.value = res.items;
  total.value = res.total;
}

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

const message = useMessage();
const dialog = useAppDialog();
const themeVars = useThemeVars();

const { intent, seq, open: openModal, close: closeModal } = useTransactionModalState();

const detailIntent = computed(() => (intent.value?.type === "detail" ? intent.value : null));

function openEditFromRow(row: InvestmentTransactionRow) {
  void openModal({ type: "edit", row: ledgerRowToModalRow(row) });
}

function openDetailFromRow(row: InvestmentTransactionRow) {
  void openModal({ type: "detail", row: ledgerRowToModalRow(row) });
}

function onEditSaved() {
  closeModal();
  void load();
}

let removingId = "";

const { run: runRemove } = useLoadable(async () => {
  await api.deleteTransaction(removingId);
});

async function remove(id: string) {
  removingId = id;
  const ok = await runRemove();
  if (ok === null) return;
  message.success(t("transactions.list.deleted"));
  await load();
}

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

const rowMenu = useRowContextMenu<InvestmentTransactionRow>((key, row) => {
  if (key === "detail") openDetailFromRow(row);
  else if (key === "edit") openEditFromRow(row);
  else if (key === "delete") confirmDelete(row);
});

const menuShow = computed(() => rowMenu.state.value !== null);
const menuX = computed(() => rowMenu.position.value.x);
const menuY = computed(() => rowMenu.position.value.y);

const menuOptions = computed<DropdownOption[]>(() => {
  const row = rowMenu.state.value?.row;
  return row
    ? buildRowMenuOptions(
        {
          kind: row.kind,
          fundings: [],
          purchases: [],
          source_order_no: row.source_order_no,
        },
        {
          errorColor: themeVars.value.errorColor,
        },
      )
    : [];
});

const rowProps = (row: InvestmentTransactionRow) => ({
  onContextmenu: (e: MouseEvent) => rowMenu.open(e, row),
});

function onModalShowUpdate(show: boolean) {
  if (!show) closeModal();
}

const kindOptions = computed<Array<{ label: string; value: TransactionKind }>>(() =>
  LEDGER_TAB_KINDS.map((value) => ({
    label: t(`transactions.kind.${value}`),
    value,
  })),
);

const kindValue = computed<TransactionKind[] | null>(() => {
  if (!session.detailKinds?.length) return null;
  const rank = new Map(LEDGER_TAB_KINDS.map((k, i) => [k, i]));
  return [...session.detailKinds].sort((a, b) => (rank.get(a) ?? 0) - (rank.get(b) ?? 0));
});

function onKindFilterChange(values: TransactionKind[] | null) {
  session.setDetailKinds(values?.length ? values : null);
}

const { selectableInvestmentAccountOptions } = storeToRefs(reference);
const accountOptions = usePinnedAccountOptions(
  computed(() => session.detailAccountId),
  selectableInvestmentAccountOptions,
);

function onAccountFilterChange(id: string | null) {
  session.setDetailAccount(id);
}

const quickRange = computed<NullableDateRange>({
  get: () => ({ from: session.detailDateFrom, to: session.detailDateTo }),
  set: (range) =>
    session.setDetailDateRange(
      range.from !== null && range.to !== null ? [range.from, range.to] : null,
    ),
});

const filtersActive = computed(
  () =>
    session.detailKinds !== null ||
    session.detailAccountId !== null ||
    session.detailInstrumentId !== null ||
    session.detailDateFrom !== null ||
    session.detailDateTo !== null,
);

function accountName(row: InvestmentTransactionRow): string {
  return reference.accountMap.get(row.account_id)?.name ?? "-";
}

function fundingAccountName(row: InvestmentTransactionRow): string {
  return row.funding_account_id
    ? (reference.accountMap.get(row.funding_account_id)?.name ?? "-")
    : "-";
}

function instrumentCell(row: InvestmentTransactionRow): string {
  if (row.convert) return `${row.symbol} → ${row.convert.to_symbol}`;
  return instrumentDisplayLabel(row.symbol, row.instrument_name);
}

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

function amountCell(row: InvestmentTransactionRow): string {
  if (row.convert) return formatAmount(row.convert.out_amount_cents);
  if (row.split) return "-";
  return formatAmount(row.amount_cents);
}

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
    render: (row) =>
      h(NTag, { type: KIND_TAG_TYPE[row.kind] }, () => t(`transactions.kind.${row.kind}`)),
  },
  {
    title: t("investments.ledger.columns.instrument"),
    key: "instrument",
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
    title: t("investments.ledger.columns.account"),
    key: "account",
    width: 110,
    ellipsis: { tooltip: true },
    render: accountName,
  },
  {
    title: t("investments.ledger.columns.fundingAccount"),
    key: "funding_account",
    width: 110,
    ellipsis: { tooltip: true },
    render: fundingAccountName,
  },
  rowActionsColumn<InvestmentTransactionRow>((e, row) => rowMenu.open(e, row)),
]);

const scrollX = computed(() => sumFixedColumnWidths(columns.value));

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

const emptyDescription = computed(() =>
  filtersActive.value ? t("investments.ledger.emptyFiltered") : t("investments.ledger.empty"),
);

const {
  intent: rawCreateIntent,
  seq: createSeq,
  open: openCreateModal,
  close: closeCreateModal,
} = useTransactionModalState();

const createIntent = computed(() =>
  rawCreateIntent.value?.type === "create" ? rawCreateIntent.value : null,
);

function openCreate(kind: "buy" | "sell") {
  void openCreateModal({ type: "create", kind });
}

const createTitle = computed(() =>
  createIntent.value
    ? t("transactions.create.titleWithKind", {
        kind: t(`transactions.kind.${createIntent.value.kind}`),
      })
    : t("transactions.create.title"),
);

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
    <!-- 订单区面（范围外修复 #1862 缺口）：带订单号的 buy/sell 行点详情落 order 意图
         （编排 open 与主列表同一单源），本页签弹窗此前缺分支、呈现空弹窗——补齐与
         主列表同构的 OrderDetail 渲染（purchases 恒空不落对应意图，不设死分支） -->
    <OrderDetail
      :key="seq"
      v-else-if="detailIntent?.detail.kind === 'order'"
      :row="detailIntent.row"
    />
  </AppModal>
</template>
