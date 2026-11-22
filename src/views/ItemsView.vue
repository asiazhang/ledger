<script setup lang="ts">
import { errorMessage } from "@ledger/utils/errors";
import { h, computed, nextTick, onMounted, ref } from "vue";
import { useRouter, useRoute } from "vue-router";
import {
  NCard,
  NButton,
  NDataTable,
  NForm,
  NFormItem,
  NInput,
  NAlert,
  NSpace,
  NDescriptions,
  NDescriptionsItem,
  useMessage,
  type DataTableColumns,
} from "naive-ui";
import { formatAmount, yuanToCents, centsToYuan } from "@ledger/money";
import { todayStr } from "@ledger/utils/date";
import type {
  ItemDailyCost,
  ItemDisposeInput,
  ItemInput,
  ItemWithDailyCost,
  Transaction,
} from "@ledger/types";
import { api } from "@ledger/api";
import AppModal from "@ledger/ui-kit/AppModal.vue";
import AppDatePicker from "@ledger/ui-kit/AppDatePicker.vue";
import AppPopconfirm from "@ledger/ui-kit/AppPopconfirm.vue";
import PinyinSelect from "@ledger/ui-kit/PinyinSelect.vue";
import { useModalIntent } from "@ledger/modal-intent";
import { useFocusParam } from "@/composables/useFocusParam";
import { useWindowTier } from "@ledger/window-tier";
import { sumFixedColumnWidths } from "@ledger/utils/table";
import { useReferenceStore } from "@/stores/reference";
import { useItemsStore } from "@/item/items";
import { t } from "@ledger/i18n";

const reference = useReferenceStore();

const windowTier = useWindowTier();
const isMobileTier = computed(() => windowTier.value === "mobile");
const itemsStore = useItemsStore();
const message = useMessage();
const router = useRouter();
const route = useRoute();

const baseCurrencyCode = ref<string>("CNY");

function goTransactions() {
  router.push({ name: "transactions" });
}

const expenseTxs = ref<Transaction[]>([]);
const editLinkTxId = ref<string | null>(null);
const editOrigLink = ref<string | null>(null);

const linkTxOptions = () =>
  expenseTxs.value.map((t) => ({
    label: `${t.date} · ${formatAmount(t.amount_cents, reference.getCurrency(t.currency_code))}${t.note ? ` · ${t.note}` : ""}`,
    value: t.id,
  }));

function findLinkedTx(txId: string | null): Transaction | undefined {
  return expenseTxs.value.find((t) => t.id === txId);
}

function applyLinkedTxToEdit(txId: string | null) {
  const tx = findLinkedTx(txId);
  if (!tx) return;
  editPurchaseDate.value = tx.date;
  editCostYuan.value = String(centsToYuan(tx.amount_cents));
}

const editRelinking = computed(() => editLinkTxId.value !== editOrigLink.value);

interface ItemEditIntent {
  row: ItemWithDailyCost;
}

const {
  intent: editIntent,
  seq: editSeq,
  open: openEditIntent,
  close: closeEdit,
} = useModalIntent<ItemEditIntent>();

const editName = ref("");
const editPurchaseDate = ref("");
const editCostYuan = ref("");
const editNote = ref("");

function openEdit(row: ItemWithDailyCost) {
  editName.value = row.name;
  editPurchaseDate.value = row.purchase_date;
  editCostYuan.value = String(centsToYuan(row.total_cost_cents));
  editNote.value = row.note ?? "";
  editLinkTxId.value = row.purchase_transaction_id;
  editOrigLink.value = row.purchase_transaction_id;
  openEditIntent({ row });
}

async function saveEdit() {
  if (!editIntent.value) return;
  if (!editName.value.trim()) {
    message.warning(t("items.msg.nameRequired"));
    return;
  }
  if (!editPurchaseDate.value) {
    message.warning(t("items.msg.dateRequired"));
    return;
  }
  const costCents = yuanToCents(editCostYuan.value);
  if (costCents === null || costCents <= 0) {
    message.warning(t("items.msg.costInvalid"));
    return;
  }
  const input: ItemInput = {
    name: editName.value.trim(),
    purchase_date: editPurchaseDate.value,
    total_cost_cents: costCents,
    currency_code: editIntent.value.row.currency_code,
    note: editNote.value.trim() || null,
    purchase_transaction_id: editLinkTxId.value,
  };
  try {
    await itemsStore.update(editIntent.value.row.id, input);
    message.success(t("items.msg.saved"));
    closeEdit();
  } catch (e) {
    message.error(t("items.msg.saveFailed", { msg: errorMessage(e) }));
  }
}

const detail = ref<ItemWithDailyCost | null>(null);

const detailRefDate = ref<string | null>(null);
const detailCost = ref<ItemDailyCost | null>(null);

const detailCostView = computed(() => ({
  days: detailCost.value?.used_days ?? detail.value?.used_days ?? 0,
  numeratorCents: detailCost.value?.numerator_cents ?? detail.value?.numerator_cents ?? 0,
  perDayCents: detailCost.value?.per_day_cents ?? detail.value?.per_day_cents ?? 0,
}));

async function recalcDetail(date: string | null) {
  if (!detail.value) return;
  detailRefDate.value = date;
  try {
    detailCost.value = await api.calculateItemCost(detail.value.id, date);
  } catch (e) {
    message.error(t("items.msg.recalcFailed", { msg: errorMessage(e) }));
  }
}

function openDetail(row: ItemWithDailyCost) {
  detail.value = row;
  detailRefDate.value = null;
  detailCost.value = null;
}

function detailAmount(cents: number): string {
  return detail.value ? formatAmount(cents, reference.getCurrency(detail.value.currency_code)) : "";
}

const disposing = ref<ItemWithDailyCost | null>(null);
const disposeDate = ref("");
const disposeResidualYuan = ref("");

function openDispose(row: ItemWithDailyCost) {
  disposing.value = row;
  disposeDate.value = row.disposal_date ?? todayStr();
  disposeResidualYuan.value =
    row.residual_value_cents != null ? String(centsToYuan(row.residual_value_cents)) : "";
}

function closeDispose() {
  disposing.value = null;
}

async function confirmDispose() {
  if (!disposing.value) return;
  if (!disposeDate.value) {
    message.warning(t("items.msg.disposeDateRequired"));
    return;
  }
  let residualCents: number | null = null;
  if (disposeResidualYuan.value.trim()) {
    const cents = yuanToCents(disposeResidualYuan.value);
    if (cents === null || cents < 0) {
      message.warning(t("items.msg.residualInvalid"));
      return;
    }
    residualCents = cents;
  }
  const input: ItemDisposeInput = {
    disposal_date: disposeDate.value,
    residual_value_cents: residualCents,
  };
  try {
    await itemsStore.dispose(disposing.value.id, input);
    message.success(
      disposing.value.status === "in_use" ? t("items.msg.disposed") : t("items.msg.disposeUpdated"),
    );
    closeDispose();
  } catch (e) {
    message.error(t("items.msg.disposeFailed", { msg: errorMessage(e) }));
  }
}

async function removeItem(id: string) {
  try {
    await itemsStore.remove(id);
    message.success(t("items.msg.deleted"));
  } catch (e) {
    message.error(t("items.msg.deleteFailed", { msg: errorMessage(e) }));
  }
}

const highlightedItemId = ref<string | null>(null);

const tableRef = ref<{ $el?: HTMLElement } | null>(null);

const focusParam = useFocusParam({
  query: () => route.query,
  onFocus: (itemId) => {
    highlightedItemId.value = itemId;
  },
});

function rowProps(row: ItemWithDailyCost): Record<string, unknown> {
  return {
    "data-item-id": row.id,
    class: row.id === highlightedItemId.value ? "item-row-focus" : undefined,
  };
}

async function scrollToHighlighted(): Promise<void> {
  if (!highlightedItemId.value) return;
  for (let hop = 0; hop < 5; hop += 1) {
    await nextTick();
    const root = tableRef.value?.$el as HTMLElement | undefined;
    const anchor = root
      ? Array.from(root.querySelectorAll<HTMLElement>("[data-item-id]")).find(
          (el) => el.dataset.itemId === highlightedItemId.value,
        )
      : undefined;
    if (anchor) {
      anchor.scrollIntoView({ block: "center" });
      return;
    }
  }
}

const columns: DataTableColumns<ItemWithDailyCost> = [
  { title: () => t("items.columns.name"), key: "name" },
  { title: () => t("items.columns.purchaseDate"), key: "purchase_date" },
  {
    title: () => t("items.columns.status"),
    key: "status",
    render: (row) =>
      row.status === "in_use"
        ? t("items.status.inUse")
        : t("items.status.disposedOn", { date: row.disposal_date ?? "" }),
  },
  {
    title: () => t("items.columns.totalCost"),
    key: "total_cost_cents",
    render: (row) => formatAmount(row.total_cost_cents, reference.getCurrency(row.currency_code)),
  },
  { title: () => t("items.columns.usedDays"), key: "used_days" },
  {
    title: () => t("items.columns.perDayCost"),
    key: "per_day_cents",
    render: (row) => formatAmount(row.per_day_cents, reference.getCurrency(row.currency_code)),
  },
  {
    title: () => t("items.columns.actions"),
    key: "actions",
    width: 200,
    render: (row) =>
      h(NSpace, { size: 4 }, () => [
        h(NButton, { size: "tiny", onClick: () => openDetail(row) }, () =>
          t("items.rowActions.detail"),
        ),
        h(NButton, { size: "tiny", onClick: () => openEdit(row) }, () =>
          t("items.rowActions.edit"),
        ),
        h(NButton, { size: "tiny", onClick: () => openDispose(row) }, () =>
          row.status === "in_use"
            ? t("items.rowActions.dispose")
            : t("items.rowActions.disposeInfo"),
        ),
        h(
          AppPopconfirm,
          { onPositiveClick: () => removeItem(row.id) },
          {
            default: () => t("items.deleteConfirm"),
            trigger: () =>
              h(NButton, { size: "tiny", type: "error", quaternary: true }, () =>
                t("items.rowActions.delete"),
              ),
          },
        ),
      ]),
  },
];

const tableScrollX = sumFixedColumnWidths(columns);

onMounted(() => {
  focusParam.consume();
  void itemsStore
    .refresh()
    .catch(() => {})
    .then(scrollToHighlighted);
  api
    .listTransactions({ kinds: ["expense"], limit: 100 })
    .then((r) => (expenseTxs.value = r.items))
    .catch(() => {});
  api
    .getBaseCurrency()
    .then((s) => (baseCurrencyCode.value = s.code))
    .catch(() => {});
});
</script>

<template>
  <div class="viewport-table-view viewport-gap-16">
    <!-- 创建唯一入口提示（issue #207）：本页不提供手动新增，物品只能经交易右键「加入物品」创建 -->
    <NAlert type="info" :show-icon="true" data-testid="item-create-hint">
      {{ t("items.createHint") }}
      <NButton
        size="tiny"
        type="primary"
        secondary
        style="margin-left: 8px"
        data-testid="item-go-transactions"
        @click="goTransactions"
      >
        {{ t("items.goTransactions") }}
      </NButton>
    </NAlert>

    <NCard :title="t('items.listTitle')" size="small" class="viewport-table-card">
      <NDataTable
        ref="tableRef"
        :columns="columns"
        :data="itemsStore.items"
        :row-props="rowProps"
        :bordered="false"
        size="small"
        :flex-height="!isMobileTier"
        :scroll-x="isMobileTier ? tableScrollX : undefined"
      >
        <template #empty>
          <span data-testid="item-empty-guide">
            {{ t("items.emptyGuide") }}
          </span>
        </template>
      </NDataTable>
    </NCard>

    <!-- 编辑弹窗（issue #117）：币种不可改，沿用行内币种。
         显示由「意图非空」派生（无独立 show 布尔），关闭（✕ / ESC / 取消 / 提交成功）
         统一经工厂清回 null 终态；序号作表单 key 强制重建（ADR-0072）。 -->
    <AppModal
      :show="editIntent !== null"
      preset="card"
      :title="t('items.edit.title')"
      card-size="sm"
      data-testid="item-edit-modal"
      @update:show="(v: boolean) => (v ? undefined : closeEdit())"
    >
      <NForm
        v-if="editIntent"
        :key="editSeq"
        label-placement="left"
        :show-feedback="false"
        size="small"
      >
        <!-- 行距节奏容器：NFormItem 默认零行距，表单项与按钮行同包（ADR-0079 决策 4 / issue #804） -->
        <NSpace vertical :size="12">
          <NFormItem :label="t('items.edit.label.name')">
            <NInput v-model:value="editName" :placeholder="t('items.edit.placeholder.name')" />
          </NFormItem>
          <NFormItem :label="t('items.edit.label.purchaseDate')">
            <AppDatePicker
              v-model:formatted-value="editPurchaseDate"
              :disabled="editRelinking"
              type="date"
              value-format="yyyy-MM-dd"
              style="width: 160px"
            />
          </NFormItem>
          <NFormItem :label="t('items.edit.label.totalCost')">
            <NInput
              v-model:value="editCostYuan"
              :disabled="editRelinking"
              :placeholder="t('items.edit.placeholder.totalCost')"
              style="width: 160px"
            />
          </NFormItem>
          <NFormItem :label="t('items.edit.label.linkedTx')">
            <PinyinSelect
              v-model:value="editLinkTxId"
              :options="linkTxOptions()"
              :placeholder="t('items.edit.placeholder.linkedTx')"
              @update:value="applyLinkedTxToEdit"
            />
          </NFormItem>
          <NFormItem :label="t('items.edit.label.currency')">
            <span>{{ editIntent.row.currency_code }}{{ t("items.edit.currencyFixed") }}</span>
          </NFormItem>
          <NFormItem :label="t('items.edit.label.note')">
            <NInput v-model:value="editNote" :placeholder="t('items.edit.placeholder.note')" />
          </NFormItem>
          <NSpace justify="end">
            <NButton @click="closeEdit">{{ t("items.rowActions.cancel") }}</NButton>
            <NButton type="primary" @click="saveEdit">{{ t("items.rowActions.save") }}</NButton>
          </NSpace>
        </NSpace>
      </NForm>
    </AppModal>

    <!-- 处置弹窗（issue #120）：处置日期必填，残值可选；已处置物品可修正处置信息 -->
    <AppModal
      :show="disposing !== null"
      preset="card"
      :title="
        disposing?.status === 'in_use'
          ? t('items.dispose.titleInUse')
          : t('items.dispose.titleInfo')
      "
      card-size="sm"
      data-testid="item-dispose-modal"
      @update:show="(v: boolean) => (v ? undefined : closeDispose())"
    >
      <NForm v-if="disposing" label-placement="left" :show-feedback="false" size="small">
        <!-- 行距节奏容器：NFormItem 默认零行距，表单项与按钮行同包（ADR-0079 决策 4 / issue #804） -->
        <NSpace vertical :size="12">
          <NFormItem :label="t('items.dispose.label.item')">
            <span>{{ disposing.name }}</span>
          </NFormItem>
          <NFormItem :label="t('items.dispose.label.date')">
            <AppDatePicker
              v-model:formatted-value="disposeDate"
              type="date"
              value-format="yyyy-MM-dd"
              style="width: 160px"
            />
          </NFormItem>
          <NFormItem :label="t('items.dispose.label.residual')">
            <NInput
              v-model:value="disposeResidualYuan"
              :placeholder="t('items.dispose.residualPlaceholder')"
              style="width: 160px"
            />
          </NFormItem>
          <NSpace justify="end">
            <NButton @click="closeDispose">{{ t("items.rowActions.cancel") }}</NButton>
            <NButton type="primary" data-testid="item-dispose-confirm" @click="confirmDispose">
              {{
                disposing.status === "in_use"
                  ? t("items.dispose.confirm")
                  : t("items.rowActions.save")
              }}
            </NButton>
          </NSpace>
        </NSpace>
      </NForm>
    </AppModal>

    <!-- 详情弹窗（issue #117）：展示成本分解 = 分子 ÷ 已用天数 = 每天成本 -->
    <AppModal
      :show="detail !== null"
      preset="card"
      :title="t('items.detail.title')"
      card-size="md"
      data-testid="item-detail-modal"
      @update:show="(v: boolean) => (v ? undefined : (detail = null))"
    >
      <NDescriptions v-if="detail" :column="1" size="small" label-placement="left" bordered>
        <NDescriptionsItem :label="t('items.detail.label.name')">{{
          detail.name
        }}</NDescriptionsItem>
        <NDescriptionsItem :label="t('items.detail.label.status')">
          {{ detail.status === "in_use" ? t("items.status.inUse") : t("items.status.disposed") }}
        </NDescriptionsItem>
        <NDescriptionsItem :label="t('items.detail.label.purchaseDate')">{{
          detail.purchase_date
        }}</NDescriptionsItem>
        <NDescriptionsItem
          v-if="detail.status === 'disposed'"
          :label="t('items.detail.label.disposalDate')"
        >
          {{ detail.disposal_date }}
        </NDescriptionsItem>
        <NDescriptionsItem
          v-if="detail.status === 'disposed'"
          :label="t('items.detail.label.residual')"
        >
          {{
            detail.residual_value_cents != null ? detailAmount(detail.residual_value_cents) : "—"
          }}
        </NDescriptionsItem>
        <NDescriptionsItem :label="t('items.detail.label.totalCost')">
          {{ detailAmount(detail.total_cost_cents)
          }}{{ t("items.currencySuffix", { code: detail.currency_code }) }}
        </NDescriptionsItem>
        <NDescriptionsItem :label="t('items.detail.label.native')">
          {{ formatAmount(detail.cost_native_cents, reference.getCurrency(baseCurrencyCode))
          }}{{ t("items.currencySuffix", { code: baseCurrencyCode }) }}
        </NDescriptionsItem>
        <NDescriptionsItem :label="t('items.detail.label.note')">{{
          detail.note ?? "—"
        }}</NDescriptionsItem>
        <NDescriptionsItem :label="t('items.detail.label.linkedTx')">
          {{ detail.purchase_transaction_id ? t("items.detail.linkedYes") : "—" }}
        </NDescriptionsItem>
        <NDescriptionsItem :label="t('items.detail.label.refDate')">
          <AppDatePicker
            :formatted-value="detailRefDate"
            clearable
            type="date"
            value-format="yyyy-MM-dd"
            :placeholder="t('items.detail.refDatePlaceholder')"
            style="width: 160px"
            @update:formatted-value="recalcDetail"
          />
        </NDescriptionsItem>
        <NDescriptionsItem :label="t('items.detail.label.usedDays')">
          {{ t("items.detail.usedDays", { n: detailCostView.days }) }}
        </NDescriptionsItem>
        <NDescriptionsItem :label="t('items.detail.label.perDayBreakdown')">
          {{
            t("items.detail.breakdown", {
              cost: detailAmount(detailCostView.numeratorCents),
              days: detailCostView.days,
              perDay: detailAmount(detailCostView.perDayCents),
            })
          }}
        </NDescriptionsItem>
      </NDescriptions>
    </AppModal>
  </div>
</template>

<style>
tr.item-row-focus > td {
  background-color: rgba(245, 158, 11, 0.16);
}
</style>
