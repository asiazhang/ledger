<script setup lang="ts">
import { h, ref, computed, watch } from "vue";
import {
  NButton,
  NCard,
  NCheckbox,
  NDataTable,
  NForm,
  NFormItem,
  NInput,
  NSpace,
  NTag,
  useMessage,
  type DataTableColumn,
  type PaginationProps,
} from "naive-ui";
import AppPopconfirm from "@ledger/ui-kit/AppPopconfirm.vue";
import MerchantEditModal from "@/merchants/MerchantEditModal.vue";
import { api } from "@ledger/api";
import { useRouter } from "vue-router";
import { useReferenceStore } from "@/stores/reference";
import { useModalIntent } from "@ledger/modal-intent";
import { useWindowTier } from "@ledger/window-tier";
import { matchLabel } from "@ledger/utils/pinyin-filter";
import { PAGE_SIZE_OPTIONS } from "@ledger/utils/pagination";
import { t } from "@ledger/i18n";
import { formatQuantity } from "@ledger/money";
import { sumFixedColumnWidths } from "@ledger/utils/table";
import type { Merchant, MerchantInput } from "@ledger/types";

interface MerchantRow extends Merchant {
  transactionCount: number;
}

const reference = useReferenceStore();
const message = useMessage();
const router = useRouter();

const windowTier = useWindowTier();
const isMobileTier = computed(() => windowTier.value === "mobile");

function goMerchantTransactions(m: MerchantRow) {
  router.push({ name: "transactions", query: { merchant: m.id } });
}

const transactionCounts = ref(new Map<string, number>());

async function loadTransactionCounts() {
  try {
    const rows = await api.listMerchantTransactionCounts();
    transactionCounts.value = new Map(rows.map((r) => [r.merchant_id, r.transaction_count]));
  } catch {}
}

watch(
  () => reference.version,
  () => {
    void loadTransactionCounts();
  },
  { immediate: true },
);

function toRow(m: Merchant): MerchantRow {
  return { ...m, transactionCount: transactionCounts.value.get(m.id) ?? 0 };
}

const rows = computed<MerchantRow[]>(() => reference.merchants.map(toRow));

const showDeleted = ref(false);

const deletedRows = computed<MerchantRow[]>(() =>
  [...reference.deletedMerchants.values()].map(toRow),
);

const searchTerm = ref("");

const displayRows = computed<MerchantRow[]>(() => {
  const base = showDeleted.value ? [...rows.value, ...deletedRows.value] : rows.value;
  return base.filter((m) => matchLabel(searchTerm.value, m.name));
});

const currentPage = ref(1);
const pageSize = ref(50);

function remainingOnPageAfterDelete(): number {
  const start = (currentPage.value - 1) * pageSize.value;
  const rowsOnPage = Math.max(0, displayRows.value.length - start);
  return showDeleted.value ? rowsOnPage : rowsOnPage - 1;
}

function afterRowDelete(remainingOnPage: number) {
  if (remainingOnPage === 0 && currentPage.value > 1) {
    currentPage.value -= 1;
  }
}

watch([searchTerm, showDeleted], () => {
  currentPage.value = 1;
});

function resetPageOnSort() {
  currentPage.value = 1;
}

const pagination = computed<PaginationProps>(() => ({
  page: currentPage.value,
  pageSize: pageSize.value,
  showSizePicker: true,
  showQuickJumper: true,
  pageSizes: PAGE_SIZE_OPTIONS,
  prefix: ({ itemCount }) =>
    h("span", null, () => t("settings.merchants.total", { n: itemCount ?? 0 })),
  onChange: (p: number) => {
    currentPage.value = p;
  },
  onUpdatePageSize: (size: number) => {
    pageSize.value = size;
    currentPage.value = 1;
  },
}));

const name = ref("");

async function addMerchant() {
  const trimmed = name.value.trim();
  if (!trimmed) {
    message.warning(t("settings.merchants.msg.nameRequired"));
    return;
  }
  const input: MerchantInput = { name: trimmed };
  try {
    await api.createMerchant(input);
    message.success(t("settings.merchants.msg.added"));
    name.value = "";
  } catch (e) {
    message.error(t("settings.merchants.msg.addFailed", { msg: e }));
  }
}

interface MerchantEditIntent {
  merchant: Merchant;
}

const {
  intent: editIntent,
  seq: editSeq,
  open: openEditIntent,
  close: closeEdit,
} = useModalIntent<MerchantEditIntent>();

function openEdit(m: Merchant) {
  openEditIntent({ merchant: m });
}

async function removeMerchant(id: string) {
  try {
    await api.deleteMerchant(id);
    afterRowDelete(remainingOnPageAfterDelete());
    message.success(t("settings.merchants.msg.deleted"));
  } catch (e) {
    message.error(t("settings.merchants.msg.deleteFailed", { msg: e }));
  }
}

const columns: DataTableColumn<MerchantRow>[] = [
  {
    title: () => t("settings.merchants.columns.name"),
    key: "name",
    width: 200,
    ellipsis: { tooltip: true },
    render: (m) =>
      m.is_deleted
        ? h(NSpace, { size: "small", align: "center", wrap: false }, () => [
            h("span", m.name),
            h(NTag, { size: "small", bordered: false }, () => t("settings.merchants.deletedTag")),
          ])
        : m.name,
  },
  {
    title: () => t("settings.merchants.columns.transactionCount"),
    key: "transactionCount",
    width: 110,
    sorter: (a, b) => a.transactionCount - b.transactionCount,
    render: (m) =>
      h(
        NButton,
        {
          text: true,
          type: "primary",
          title: t("common.link.viewMerchant"),
          onClick: () => goMerchantTransactions(m),
        },
        () => formatQuantity(m.transactionCount),
      ),
  },
  {
    title: () => t("settings.merchants.columns.actions"),
    key: "actions",
    width: 140,
    render: (m) =>
      m.is_deleted
        ? null
        : h(NSpace, { size: "small" }, () => [
            h(
              NButton,
              { size: "tiny", quaternary: true, type: "primary", onClick: () => openEdit(m) },
              () => t("settings.merchants.rowActions.edit"),
            ),
            h(
              AppPopconfirm,
              { onPositiveClick: () => removeMerchant(m.id) },
              {
                default: () => t("settings.merchants.deleteConfirm"),
                trigger: () =>
                  h(NButton, { size: "tiny", type: "error", quaternary: true }, () =>
                    t("settings.merchants.rowActions.delete"),
                  ),
              },
            ),
          ]),
  },
];

const tableScrollX = sumFixedColumnWidths(columns);
</script>

<template>
  <NSpace vertical :size="16">
    <NCard :title="t('settings.merchants.addTitle')" size="small">
      <NForm label-placement="left" :show-feedback="false" inline size="small">
        <NFormItem :label="t('settings.merchants.form.name')">
          <NInput
            v-model:value="name"
            :placeholder="t('settings.merchants.form.namePlaceholder')"
            style="width: 160px"
          />
        </NFormItem>
        <NButton type="primary" @click="addMerchant">{{
          t("settings.merchants.form.add")
        }}</NButton>
      </NForm>
    </NCard>

    <NCard :title="t('settings.merchants.listTitle')" size="small">
      <NSpace vertical :size="12">
        <NSpace align="center">
          <NInput
            v-model:value="searchTerm"
            clearable
            :placeholder="t('settings.merchants.searchPlaceholder')"
            style="width: 240px"
          />
          <NCheckbox v-model:checked="showDeleted">
            {{ t("settings.merchants.showDeleted") }}
          </NCheckbox>
        </NSpace>
        <NDataTable
          :columns="columns"
          :data="displayRows"
          :bordered="false"
          size="small"
          :row-key="(m: MerchantRow) => m.id"
          :scroll-x="isMobileTier ? tableScrollX : undefined"
          :pagination="pagination"
          @update:sorter="resetPageOnSort"
        />
      </NSpace>
    </NCard>

    <!-- 商户编辑弹窗。显示由「意图非空」派生（无独立 show 布尔），关闭（✕ / ESC /
         取消 / 保存成功）统一经工厂清回 null 终态；序号作 key 强制重建（ADR-0072）。 -->
    <MerchantEditModal
      :key="editSeq"
      :show="editIntent !== null"
      :merchant="editIntent?.merchant ?? null"
      @update:show="(v: boolean) => (v ? undefined : closeEdit())"
    />
  </NSpace>
</template>
