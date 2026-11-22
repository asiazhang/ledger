<script setup lang="ts">
import { t } from "@ledger/i18n";
import { errorMessage } from "@ledger/utils/errors";
import { computed, ref, shallowRef, watch } from "vue";
import { useRoute } from "vue-router";
import {
  NDataTable,
  NButton,
  NButtonGroup,
  NIcon,
  NEmpty,
  NPagination,
  NSpace,
  NSpin,
  NSwitch,
  useMessage,
  useThemeVars,
  type DataTableColumn,
  type DropdownOption,
} from "naive-ui";
import { ChevronDown } from "@vicons/ionicons5";
import AppModal from "@ledger/ui-kit/AppModal.vue";
import AppDropdown from "@ledger/ui-kit/AppDropdown.vue";
import AppSelect from "@ledger/ui-kit/AppSelect.vue";
import TransactionCardList from "@/transaction/TransactionCardList.vue";
import CreateFab from "@/components/CreateFab.vue";
import { useAppDialog } from "@/composables/useAppDialog";
import { useWindowTier } from "@ledger/window-tier";
import TransactionForm from "@/transaction/TransactionForm.vue";
import QuickTimeRange from "@/components/QuickTimeRange.vue";
import PinyinSelect from "@ledger/ui-kit/PinyinSelect.vue";
import RefundForm from "@/transaction/RefundForm.vue";
import AddItemForm from "@/item/AddItemForm.vue";
import ConvertDetail from "@/investment/ConvertDetail.vue";
import SplitDetail from "@/investment/SplitDetail.vue";
import DividendDetail from "@/investment/DividendDetail.vue";
import FundingDetail from "@/transaction/FundingDetail.vue";
import PurchaseDetail from "@/transaction/PurchaseDetail.vue";
import OrderDetail from "@/transaction/OrderDetail.vue";
import {
  buildRowMenuOptions,
  supportsRowDetail,
  supportsRowEdit,
} from "@/transaction/transaction-row-menu";
import { useCreateShortcuts, CREATE_KIND_KEYS } from "@/composables/useCreateShortcuts";
import { useInputMode } from "@/composables/useInputMode";
import { usePaginationShortcuts } from "@/composables/usePaginationShortcuts";
import { useTimePeriodShortcuts, PERIOD_STEP_KEYS } from "@/composables/useTimePeriodShortcuts";
import { useRowContextMenu } from "@ledger/row-context-menu";
import { useTransactionTableCellActions } from "@/transaction/table-cell-actions";
import { paginationHint, paginationRow } from "./TransactionsView.css.ts";
import {
  useTransactionFilter,
  UNCATEGORIZED_ONLY,
  MAIN_LIST_KINDS,
  resolveRequestKinds,
  normalizeKindSelection,
} from "@/transaction/useTransactionFilter";
import { registerViewReset } from "@/composables/viewResetRegistry";
import { useTransactionModalState } from "@ledger/transaction-modal-state";
import { api } from "@ledger/api";
import { useReferenceStore } from "@/stores/reference";
import { usePinnedAccountOptions } from "@/composables/useFormShared";
import { useItemsStore } from "@/item/items";
import { useTransactionViewPreferencesStore } from "@/transaction/transaction-view-preferences";
import {
  buildTransactionColumns,
  displayRowKey,
  precomputeTransactionDisplayRows,
  purchaseIndexOf,
  purchaseRowClass,
} from "@/transaction/transaction-columns";
import { sumFixedColumnWidths } from "@ledger/utils/table";
import { PAGE_SIZE_OPTIONS } from "@ledger/utils/pagination";
import { availableCreateKinds, isCreateKindAvailable } from "@ledger/utils/create-entry-kinds";
import { isLendingEntryKind } from "@/transaction/lending";
import { type NullableDateRange } from "@ledger/utils/time-period";
import {
  LENDING_CREATE_DIRECTIONS,
  type CreateFormKind,
  type Transaction,
  type TransactionKind,
  type TransactionListFilter,
} from "@ledger/types";

const reference = useReferenceStore();
const tier = useWindowTier();
const isMobile = computed(() => tier.value === "mobile");
const itemsStore = useItemsStore();
const viewPreferences = useTransactionViewPreferencesStore();
const message = useMessage();
const dialog = useAppDialog();
const route = useRoute();
const data = shallowRef<Transaction[]>([]);
const total = ref(0);
const loading = ref(false);

const {
  filters,
  page,
  pageSize,
  refreshVersion,
  setFilter,
  resetFilters,
  refresh,
  afterRowDelete,
  syncUrlQuery,
} = useTransactionFilter();

registerViewReset(resetFilters);

const { intent, seq, open: openModal, close: closeModal } = useTransactionModalState();

const filtersActive = computed(() => Object.values(filters).some((v) => v !== null));

const detailIntent = computed(() => (intent.value?.type === "detail" ? intent.value : null));

const quickRange = computed<NullableDateRange>({
  get: () => ({ from: filters.dateFrom, to: filters.dateTo }),
  set: (range) => setFilter({ dateFrom: range.from, dateTo: range.to }),
});

const accountOptions = usePinnedAccountOptions(
  computed(() => filters.involvingAccountId),
  computed(() => reference.selectableAccountOptions),
);

const merchantOptions = computed(() =>
  [...reference.merchantMap.values()]
    .sort((a, b) => a.name.localeCompare(b.name, "zh"))
    .map((m) => ({ label: m.name, value: m.id })),
);

const kindOptions = computed<Array<{ label: string; value: TransactionKind }>>(() =>
  MAIN_LIST_KINDS.map((value) => ({
    label: t(`transactions.kind.${value}`),
    value,
  })),
);

const kindValue = computed<TransactionKind[] | null>(() => {
  if (!filters.kinds?.length) return null;
  const rank = new Map<TransactionKind, number>(MAIN_LIST_KINDS.map((k, i) => [k, i]));
  return [...filters.kinds].sort((a, b) => (rank.get(a) ?? 0) - (rank.get(b) ?? 0));
});

async function load() {
  loading.value = true;
  try {
    const filter: TransactionListFilter = {
      page: page.value,
      page_size: pageSize.value,
    };
    if (filters.dateFrom) filter.from = filters.dateFrom;
    if (filters.dateTo) filter.to = filters.dateTo;
    if (filters.involvingAccountId) filter.involving_account_id = filters.involvingAccountId;
    if (filters.merchantId) filter.merchant_id = filters.merchantId;
    if (filters.categoryId === UNCATEGORIZED_ONLY) filter.uncategorized_only = true;
    else if (filters.categoryId) filter.category_id = filters.categoryId;
    filter.kinds = resolveRequestKinds(filters.kinds);
    if (viewPreferences.hideInvestmentRelated) filter.hide_investment_related = true;
    const res = await api.listTransactions(filter);
    if (res.items.length === 0 && res.total > 0 && page.value > 1) {
      afterRowDelete(0);
      return;
    }
    data.value = res.items;
    total.value = res.total;
  } catch (e) {
    message.error(t("transactions.list.loadFailed", { msg: errorMessage(e) }));
  } finally {
    loading.value = false;
  }
}

watch(
  () => route.query,
  (query) => syncUrlQuery(query),
  { immediate: true },
);

watch(
  refreshVersion,
  () => {
    void load();
  },
  { immediate: true },
);

function createKindLabel(kind: CreateFormKind): string {
  return isLendingEntryKind(kind)
    ? t(`transactions.lending.${kind}`)
    : t(`transactions.kind.${kind}`);
}

const inputMode = useInputMode();

const createKinds = computed(() => availableCreateKinds());
const createKindOptions = computed<DropdownOption[]>(() => [
  ...createKinds.value.map((k) => ({
    label:
      inputMode.value === "pointer"
        ? t("transactions.create.kindWithKey", {
            kind: t(`transactions.kind.${k}`),
            key: CREATE_KIND_KEYS[k],
          })
        : t(`transactions.kind.${k}`),
    key: k,
  })),
  { type: "divider", key: "create-lending-divider" },
  ...LENDING_CREATE_DIRECTIONS.map((d) => ({
    label: t(`transactions.lending.${d}`),
    key: d,
  })),
]);

const createTitle = computed(() => {
  const current = intent.value;
  return current?.type === "create"
    ? t("transactions.create.titleWithKind", { kind: createKindLabel(current.kind) })
    : t("transactions.create.title");
});

function openCreate(k: CreateFormKind) {
  void openModal({ type: "create", kind: k });
}

useCreateShortcuts(openCreate, isCreateKindAvailable);
function onFormCreated() {
  closeModal();
  refresh();
}

function onAccountFilterChange(id: string | null) {
  setFilter({ involvingAccountId: id });
}

function onMerchantFilterChange(id: string | null) {
  setFilter({ merchantId: id });
}

function onKindFilterChange(values: TransactionKind[] | null) {
  setFilter({ kinds: normalizeKindSelection(values) });
}

function onHideInvestmentChange(value: boolean) {
  viewPreferences.setHideInvestmentRelated(value);
  refresh();
}

async function remove(id: string) {
  try {
    await api.deleteTransaction(id);
    message.success(t("transactions.list.deleted"));
    afterRowDelete(data.value.length - 1);
  } catch (e) {
    message.error(t("transactions.list.deleteFailed", { msg: errorMessage(e) }));
  }
}

function confirmDelete(row: Transaction) {
  dialog.warning({
    title: t("transactions.deleteDialog.title"),
    content: t("transactions.deleteDialog.content"),
    positiveText: t("transactions.deleteDialog.confirm"),
    negativeText: t("transactions.deleteDialog.cancel"),
    maskClosable: false,
    onPositiveClick: () => remove(row.id),
  });
}

function openRefundFromRow(row: Transaction) {
  void openModal({ type: "refund", row });
}

function onRefundCreated() {
  closeModal();
  refresh();
}

function openEditFromRow(row: Transaction) {
  void openModal({ type: "edit", row });
}

function openDetailFromRow(row: Transaction) {
  void openModal({ type: "detail", row });
}

function onEditSaved() {
  closeModal();
  void load();
}

function openAddItemFromRow(row: Transaction) {
  void openModal({ type: "add-item", row });
}

function closeAddItem() {
  closeModal();
}

function onModalShowUpdate(show: boolean) {
  if (!show) closeModal();
}

const rowMenu = useRowContextMenu<Transaction>((key, row) => {
  if (key === "detail") openDetailFromRow(row);
  else if (key === "edit") openEditFromRow(row);
  else if (key === "refund") openRefundFromRow(row);
  else if (key === "add-item") openAddItemFromRow(row);
  else if (key === "delete") confirmDelete(row);
});

const menuShow = computed(() => rowMenu.state.value !== null);
const menuX = computed(() => rowMenu.position.value.x);
const menuY = computed(() => rowMenu.position.value.y);

const linkedTxIds = computed(
  () =>
    new Set(
      itemsStore.items
        .map((i) => i.purchase_transaction_id)
        .filter((id): id is string => id !== null),
    ),
);

const themeVars = useThemeVars();

const menuOptions = computed(() => {
  const row = rowMenu.state.value?.row;
  return row
    ? buildRowMenuOptions(row, {
        hasItem: linkedTxIds.value.has(row.id),
        errorColor: themeVars.value.errorColor,
      })
    : [];
});

const rowProps = (row: Transaction) => ({
  class: purchaseRowClass(row),
  onContextmenu: (e: MouseEvent) => {
    if (purchaseIndexOf(row) > 0) return;
    rowMenu.open(e, row);
  },
});

function onPageChange(p: number): void {
  page.value = p;
  void load();
}

const shortcutPageCount = computed(() => Math.max(1, Math.ceil(total.value / pageSize.value)));
usePaginationShortcuts((dir) => {
  const target = dir === "prev" ? page.value - 1 : page.value + 1;
  if (target < 1 || target > shortcutPageCount.value) return;
  onPageChange(target);
});

interface PeriodStepperHandle {
  stepPrev: () => void;
  stepNext: () => void;
}

const periodStepper = ref<PeriodStepperHandle | null>(null);
useTimePeriodShortcuts((dir) => {
  const stepper = periodStepper.value;
  if (!stepper) return;
  if (dir === "prev") stepper.stepPrev();
  else stepper.stepNext();
});

function onPageSizeChange(size: number): void {
  pageSize.value = size;
  refresh();
}

const columns = computed<DataTableColumn<Transaction>[]>(() => [
  ...buildTransactionColumns(reference, {
    rowActions: true,
    inputMode,
    expandPurchases: true,
  }),
]);

const displayRows = computed(() =>
  precomputeTransactionDisplayRows(reference, data.value, { expandPurchases: true }),
);

const scrollX = computed(() => sumFixedColumnWidths(columns.value));

const rowByKey = computed(() => new Map(displayRows.value.map((row) => [displayRowKey(row), row])));
const onTableCellClick = useTransactionTableCellActions({
  message,
  onRowMenu: (event, key) => {
    const row = rowByKey.value.get(key);
    if (row) rowMenu.open(event, row);
  },
});

const emptyDescription = computed(() =>
  filtersActive.value ? t("transactions.list.emptyFiltered") : t("transactions.list.empty"),
);

function activateCard(row: Transaction): void {
  if (supportsRowDetail(row)) openDetailFromRow(row);
  else if (supportsRowEdit(row)) openEditFromRow(row);
}
</script>

<template>
  <!-- wrap-item=false（根因修复，2026-09-25 交易页大空白）：NSpace 默认把每个组件子项包进
       一层 role=none 的 div；关闭态弹层（5 个 AppModal + 行右键 AppDropdown）渲染为注释
       节点，包裹 div 0 高却仍是 flex 子项，各占一个 12px gap 槽位——表格上方凭空多出
       6×12+12 ≈ 84px 空白。关闭弹层经 teleport 挂 body，本就不参与布局流；
       wrap-item=false 后子项直接作 flex 子元素，幻影 div 不再产生。 -->
  <div class="viewport-table-view viewport-gap-12">
    <!-- 过滤行（第一行）：账户（涉及账户语义，可清除）+ 商户（可清除，issue #191）+ 类型（可清除）+ 清除筛选。
         任意起止日期筛选已自交易页移除（搜索页保留，issue #381）；
         任一条件变化即重新查询并回到第 1 页；手动改动不同步回 URL
         （组件状态是唯一事实源，issue #96 决策 3/4），分页/页大小切换保持过滤条件。 -->
    <NSpace :size="8" align="center" :wrap="true">
      <PinyinSelect
        :value="filters.involvingAccountId"
        :options="accountOptions"
        :placeholder="t('transactions.filter.account')"
        clearable
        style="width: 160px"
        @update:value="onAccountFilterChange"
      />
      <PinyinSelect
        :value="filters.merchantId"
        :options="merchantOptions"
        :placeholder="t('transactions.filter.merchant')"
        clearable
        style="width: 160px"
        @update:value="onMerchantFilterChange"
      />
      <AppSelect
        :value="kindValue"
        :options="kindOptions"
        :placeholder="t('transactions.filter.kind')"
        multiple
        clearable
        :max-tag-count="1"
        style="width: 160px"
        @update:value="onKindFilterChange"
      />
      <!-- 视图偏好开关（ADR-0136 / issue #1811）：设备级持久裁决落筛选栏，本行在断点
           分档点之上，桌面 / 移动共用同一渲染（同构，不做双渲染）；偏好与筛选分家——
           写偏好 store 而非 setFilter，清除筛选与 ESC 复位不触达，也不参与清除按钮禁用态。 -->
      <span style="display: inline-flex; align-items: center; gap: 6px; user-select: none">
        <NSwitch
          size="small"
          :value="viewPreferences.hideInvestmentRelated"
          :aria-label="t('transactions.filter.hideInvestmentRelated')"
          @update:value="onHideInvestmentChange"
        />
        <span>{{ t("transactions.filter.hideInvestmentRelated") }}</span>
      </span>
      <NButton
        size="tiny"
        quaternary
        type="primary"
        :disabled="!filtersActive"
        @click="resetFilters"
      >
        {{ t("transactions.filter.clear") }}
      </NButton>
      <!-- 分裂按钮（桌面档）：主体直开支出弹窗，箭头展开 5 项类型菜单（issue #150；创建闭集收窄后 = 支出/收入/转账 + 借贷两项，ADR-0135 决策 5 / issue #1782）。
           移动档不渲染：记一笔入口分档，FAB 是移动档唯一记一笔入口（ADR-0088 决策 5）。 -->
      <NButtonGroup v-if="!isMobile">
        <NButton type="primary" @click="openCreate('expense')">{{
          t("transactions.create.button")
        }}</NButton>
        <AppDropdown
          trigger="click"
          :options="createKindOptions"
          @select="(k: string | number) => openCreate(k as CreateFormKind)"
        >
          <NButton type="primary" :aria-label="t('transactions.create.moreTypes')">
            <NIcon><ChevronDown /></NIcon>
          </NButton>
        </AppDropdown>
      </NButtonGroup>
    </NSpace>
    <!-- 时间维度行（第二行，issue #381/#382/#383，#410 起由共享受控组件承载）：
         芯片「全部 | 当月 | 当季 | 当年 | 去年」＋期间步进器＋期间直达面板整行由
         QuickTimeRange 渲染；快照区间 v-model 进出（唯一事实源仍是 TransactionFilter
         日期维度），交互、文案、弹层注册表上报零变化（ADR-0057 决策 5）。 -->
    <QuickTimeRange
      ref="periodStepper"
      v-model="quickRange"
      :step-shortcut-keys="PERIOD_STEP_KEYS"
    />
    <!-- 快速记账弹窗：标题标明入口选定类型，内嵌收窄后的 TransactionForm（无类型单选），
         提交成功关闭并刷新列表；显示开关由模块意图派生，序号作表单 key 强制重建（ADR-0045） -->
    <AppModal
      :show="intent?.type === 'create'"
      :title="createTitle"
      preset="card"
      display-directive="if"
      card-size="md"
      @update:show="
        (show: boolean) => {
          if (!show) closeModal();
        }
      "
    >
      <TransactionForm
        v-if="intent?.type === 'create'"
        :key="seq"
        :kind="intent.kind"
        @created="onFormCreated"
      />
    </AppModal>
    <!-- 行内退款弹窗：原交易由右键所在行固定（fixed-target），账户/币种锁定继承，
         金额默认原交易金额（可改）；提交走现有 kind=refund 写路径。
         开启/关闭经 TransactionModalState 编排（目标行由意图携带，序号作表单 key）。 -->
    <AppModal
      :show="intent?.type === 'refund'"
      :title="t('transactions.refund.title')"
      preset="card"
      display-directive="if"
      card-size="md"
      @update:show="onModalShowUpdate"
    >
      <RefundForm
        :key="seq"
        v-if="intent?.type === 'refund'"
        :fixed-target="intent.row"
        @created="onRefundCreated"
      />
    </AppModal>
    <!-- 「加入物品」确认弹窗（issue #119）：原交易由右键所在行固定，自动带出只读展示。
         开启/关闭经 TransactionModalState 编排（目标行由意图携带，序号作表单 key）。 -->
    <AppModal
      :show="intent?.type === 'add-item'"
      :title="t('transactions.addItem.title')"
      preset="card"
      display-directive="if"
      card-size="sm"
      @update:show="onModalShowUpdate"
    >
      <AddItemForm
        :key="seq"
        v-if="intent?.type === 'add-item'"
        :transaction="intent.row"
        @created="closeAddItem"
        @cancel="closeAddItem"
      />
    </AppModal>
    <!-- 编辑弹窗（issue #178）：回填既有交易全部业务字段，kind 锁死；提交走全字段更新命令，
         成功关窗并刷新列表（保持当前页与筛选）。开启/关闭经 TransactionModalState 编排
         （目标行与买卖明细由意图携带，序号作表单 key 强制重建） -->
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
    <!-- 只读详情弹窗（ADR-0106 决策 10 / #1048、#1052；ADR-0109 / #1078；#1861 / #1862 / #1884 / ADR-0138）：
         界面只读 kind（convert / split / dividend）不体现任何写操作（无创建/编辑/软删），
         带非空出资分解的交易行（fundings 非空）、带非空购买项的行（purchases 非空，决策 15）
         与来源订单号行同为只读详情（#1862 / ADR-0138 决策 9）——convert 只读呈现「A → B」
         两侧标的、份额、金额、手续费与结转成本，split 只读呈现标的、带符号份额变动、调整日
         与账户，dividend 只读呈现归属标的、金额、到账账户与日期，funding 只读呈现出资项
         列表（账户 / 标签 / 金额）与 Σ 合计，purchases 只读呈现购买项清单（名称 / 分类 / 件数）；
         行激活与菜单「详情」进本入口，扩展明细的取数
         时序内化在编排模块 -->
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
      <FundingDetail
        :key="seq"
        v-else-if="detailIntent?.detail.kind === 'funding'"
        :row="detailIntent.row"
      />
      <PurchaseDetail
        :key="seq"
        v-else-if="detailIntent?.detail.kind === 'purchases'"
        :row="detailIntent.row"
      />
      <OrderDetail
        :key="seq"
        v-else-if="detailIntent?.detail.kind === 'order'"
        :row="detailIntent.row"
      />
    </AppModal>
    <!-- 行右键菜单（issue #151 / #119 / #550）：expense 行「退款」「加入物品」+ 可编辑行「编辑」
         + convert 行「详情」（无现金腿 kind 只读，ADR-0106 决策 10 / #1048）+ 其余行「删除」，
         手动定位弹出；开合上报经薄封装 attrs watch 自动生效（`:show` 绑定照旧） -->
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
    <!-- 断点双渲染（ADR-0088 决策 9，issue #846）：同一列表状态，移动档卡片列表
         （分页语义不变：20/页、页大小选择与桌面同一出口），桌面档表格一字不动 -->
    <template v-if="isMobile">
      <NSpin :show="loading">
        <TransactionCardList
          v-if="data.length > 0"
          :rows="data"
          :open-row-menu="rowMenu.open"
          :activate-row="activateCard"
        />
        <!-- 空态：与桌面表格空槽同文案同动作（过滤无结果提示 + 清除按钮）；
             加载期间不渲染空态节点（桌面 loading 时空态隐藏同规） -->
        <NEmpty v-else-if="!loading" :description="emptyDescription" size="small">
          <template v-if="filtersActive" #extra>
            <NButton size="tiny" quaternary type="primary" @click="resetFilters">
              {{ t("transactions.filter.clear") }}
            </NButton>
          </template>
        </NEmpty>
      </NSpin>
      <!-- 移动档分页：与桌面表格 pagination 同一出口（翻页/页大小/共 N 条） -->
      <NPagination
        :page="page"
        :page-size="pageSize"
        :item-count="total"
        :page-sizes="PAGE_SIZE_OPTIONS"
        show-size-picker
        @update:page="onPageChange"
        @update:page-size="onPageSizeChange"
      >
        <template #prefix>
          <span>{{ t("transactions.list.total", { n: total }) }}</span>
        </template>
        <template #suffix>
          <span
            v-if="inputMode === 'pointer'"
            :class="paginationHint"
            :style="{ color: themeVars.textColor3 }"
          >
            {{ t("transactions.list.paginationHint") }}
          </span>
        </template>
      </NPagination>
    </template>
    <!-- 备注列为弹性列（transaction-columns 中不设 width），表格始终铺满容器；
         窄窗口时备注先收缩，scroll-x（固定列宽总和）作为横向滚动下限。
         表格容器承载单元格动作的表级事件委托（issue #2189，见 onTableCellClick） -->
    <div v-else class="viewport-table-slot" @click="onTableCellClick">
      <NDataTable
        :columns="columns"
        :data="displayRows"
        :row-key="displayRowKey"
        :loading="loading"
        :bordered="false"
        size="small"
        remote
        :flex-height="!isMobile"
        :row-props="rowProps"
        :scroll-x="scrollX"
        :pagination="false"
      >
        <!-- 空态：过滤无结果时展示明确提示（与加载态区分：loading 时空态节点隐藏）；
           无过滤时为默认「暂无数据」文案 -->
        <template #empty>
          <NEmpty :description="emptyDescription" size="small">
            <template v-if="filtersActive" #extra>
              <NButton size="tiny" quaternary type="primary" @click="resetFilters">
                {{ t("transactions.filter.clear") }}
              </NButton>
            </template>
          </NEmpty>
        </template>
      </NDataTable>
    </div>
    <!-- 桌面档外置分页（issue #1929）：内建分页无插槽是键位提示游离成独立行的根因，
         改用与移动档同款 NPagination 置于表格下方同一行右端，键位提示走 #suffix 槽
         两档同源（仅指针轴渲染，触控轴不渲染，ADR-0088）；翻页/页大小/跳页语义与
         原内建分页一致（ADR-0140 决策 3 修订注记） -->
    <div v-if="!isMobile" :class="paginationRow">
      <NPagination
        :page="page"
        :page-size="pageSize"
        :item-count="total"
        :page-sizes="PAGE_SIZE_OPTIONS"
        show-size-picker
        show-quick-jumper
        @update:page="onPageChange"
        @update:page-size="onPageSizeChange"
      >
        <template #prefix>
          <span>{{ t("transactions.list.total", { n: total }) }}</span>
        </template>
        <template #suffix>
          <span
            v-if="inputMode === 'pointer'"
            :class="paginationHint"
            :style="{ color: themeVars.textColor3 }"
          >
            {{ t("transactions.list.paginationHint") }}
          </span>
        </template>
      </NPagination>
    </div>
  </div>
  <!-- 记一笔悬浮按钮（移动档交易页右下，ADR-0088 决策 5）：点开大号类型选择
       轻弹层（可用类型 = 创建闭集单源 availableCreateKinds，ADR-0135 决策 5 / issue #1782
       收窄为支出/收入/转账），经弹窗意图编排的记一笔意图（携带类型）进对应表单；零表单内部改造。
       独立根节点渲染：固定定位不参与 NSpace 布局流 -->
  <CreateFab v-if="isMobile" :kinds="createKinds" @select="openCreate" />
</template>
