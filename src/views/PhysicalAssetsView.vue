<script setup lang="ts">
import { h, computed, onMounted } from "vue";
import {
  NCard,
  NButton,
  NDataTable,
  NRadioButton,
  NRadioGroup,
  NSpace,
  NTag,
  type DataTableColumns,
} from "naive-ui";
import { formatAmount } from "@ledger/money";
import { t } from "@ledger/i18n";
import PhysicalAssetFormModal from "@/physical-asset/PhysicalAssetFormModal.vue";
import PhysicalAssetValuationModal from "@/physical-asset/PhysicalAssetValuationModal.vue";
import PhysicalAssetDisposeModal from "@/physical-asset/PhysicalAssetDisposeModal.vue";
import AppPopconfirm from "@ledger/ui-kit/AppPopconfirm.vue";
import { useModalIntent } from "@ledger/modal-intent";
import { useWindowTier } from "@ledger/window-tier";
import { usePhysicalAssetsStore } from "@/physical-asset/physicalAssets";
import { useReferenceStore } from "@/stores/reference";
import { sumFixedColumnWidths } from "@ledger/utils/table";
import type { PhysicalAsset } from "@ledger/types";

const physicalAssetsStore = usePhysicalAssetsStore();
const reference = useReferenceStore();

const windowTier = useWindowTier();
const isMobileTier = computed(() => windowTier.value === "mobile");

type PhysicalAssetFormIntent = { mode: "create" } | { mode: "edit"; asset: PhysicalAsset };

const {
  intent: formIntent,
  seq: formSeq,
  open: openFormIntent,
  close: closeForm,
} = useModalIntent<PhysicalAssetFormIntent>();

interface PhysicalAssetValuationIntent {
  asset: PhysicalAsset;
}

const {
  intent: valuationIntent,
  seq: valuationSeq,
  open: openValuationIntent,
  close: closeValuation,
} = useModalIntent<PhysicalAssetValuationIntent>();

interface PhysicalAssetDisposeIntent {
  asset: PhysicalAsset;
}

const {
  intent: disposeIntent,
  seq: disposeSeq,
  open: openDisposeIntent,
  close: closeDispose,
} = useModalIntent<PhysicalAssetDisposeIntent>();

function openCreate() {
  openFormIntent({ mode: "create" });
}

function openEdit(asset: PhysicalAsset) {
  openFormIntent({ mode: "edit", asset });
}

function openUpdateValuation(asset: PhysicalAsset) {
  openValuationIntent({ asset });
}

function openDispose(asset: PhysicalAsset) {
  openDisposeIntent({ asset });
}

async function removeAsset(id: string) {
  try {
    await physicalAssetsStore.remove(id);
  } catch {}
}

const holdingTotalText = computed(() =>
  formatAmount(
    physicalAssetsStore.holdingTotalNativeCents,
    reference.getCurrency(physicalAssetsStore.nativeCurrency),
  ),
);

const columns: DataTableColumns<PhysicalAsset> = [
  { title: () => t("physicalAssets.columns.name"), key: "name" },
  {
    title: () => t("physicalAssets.columns.valuation"),
    key: "current_valuation_native_cents",
    render: (row) =>
      row.current_valuation_native_cents !== null
        ? formatAmount(
            row.current_valuation_native_cents,
            reference.getCurrency(row.native_currency),
          )
        : "—",
  },
  {
    title: () => t("physicalAssets.columns.valuationDate"),
    key: "current_valuation_date",
    width: 120,
    render: (row) => row.current_valuation_date,
  },
  {
    title: () => t("physicalAssets.columns.status"),
    key: "status",
    width: 90,
    render: (row) =>
      h(
        NTag,
        { size: "small", type: row.status === "holding" ? "success" : "default", bordered: false },
        () =>
          row.status === "holding"
            ? t("physicalAssets.status.holding")
            : t("physicalAssets.status.disposed"),
      ),
  },
  {
    title: () => t("physicalAssets.columns.disposal"),
    key: "disposal_date",
    width: 200,
    render: (row) => {
      if (row.status !== "disposed") return "—";
      if (row.disposal_price_cents === null) return row.disposal_date ?? "—";
      const amount = formatAmount(
        row.disposal_price_cents,
        reference.getCurrency(row.disposal_currency_code ?? row.native_currency),
      );
      return `${row.disposal_date ?? "—"} / ${amount}`;
    },
  },
  {
    title: () => t("physicalAssets.columns.actions"),
    key: "actions",
    width: 240,
    render: (row) =>
      h(NSpace, { size: 4, wrap: false }, () => [
        h(
          NButton,
          {
            size: "tiny",
            quaternary: true,
            type: "primary",
            "data-testid": "physical-asset-update-valuation",
            onClick: () => openUpdateValuation(row),
          },
          () => t("physicalAssets.actions.updateValuation"),
        ),
        h(
          NButton,
          {
            size: "tiny",
            quaternary: true,
            "data-testid": "physical-asset-edit",
            onClick: () => openEdit(row),
          },
          () => t("physicalAssets.actions.edit"),
        ),
        row.status === "holding"
          ? h(
              NButton,
              {
                size: "tiny",
                quaternary: true,
                type: "warning",
                "data-testid": "physical-asset-dispose",
                onClick: () => openDispose(row),
              },
              () => t("physicalAssets.actions.dispose"),
            )
          : null,
        h(
          AppPopconfirm,
          { onPositiveClick: () => removeAsset(row.id) },
          {
            default: () => t("physicalAssets.deleteConfirm"),
            trigger: () =>
              h(
                NButton,
                {
                  size: "tiny",
                  quaternary: true,
                  type: "error",
                  "data-testid": "physical-asset-delete",
                },
                () => t("physicalAssets.actions.delete"),
              ),
          },
        ),
      ]),
  },
];

const tableScrollX = sumFixedColumnWidths(columns);

const listTitle = computed(() => t("physicalAssets.listTitle"));
const totalLabel = computed(() => t("physicalAssets.holdingTotal"));

function onFilterChange(value: string) {
  void physicalAssetsStore.setStatusFilter(value as "holding" | "disposed").catch(() => {});
}

onMounted(() => {
  void physicalAssetsStore.refresh().catch(() => {});
});
</script>

<template>
  <div class="viewport-table-view viewport-gap-16">
    <NCard size="small">
      <NSpace justify="space-between" align="center">
        <span>
          {{ totalLabel }}：<strong data-testid="physical-asset-holding-total">{{
            holdingTotalText
          }}</strong>
        </span>
        <NButton type="primary" data-testid="physical-asset-new" @click="openCreate">
          {{ t("physicalAssets.newButton") }}
        </NButton>
      </NSpace>
    </NCard>

    <NCard :title="listTitle" size="small" class="viewport-table-card">
      <template #header-extra>
        <NRadioGroup
          :value="physicalAssetsStore.statusFilter"
          size="small"
          data-testid="physical-asset-status-filter"
          @update:value="onFilterChange"
        >
          <NRadioButton value="holding">
            {{ t("physicalAssets.status.holding") }}
          </NRadioButton>
          <NRadioButton value="disposed">
            {{ t("physicalAssets.status.disposed") }}
          </NRadioButton>
        </NRadioGroup>
      </template>
      <NDataTable
        :columns="columns"
        :data="physicalAssetsStore.assets"
        :bordered="false"
        size="small"
        :flex-height="!isMobileTier"
        :scroll-x="isMobileTier ? tableScrollX : undefined"
      >
        <template #empty>
          <span data-testid="physical-asset-empty-guide">{{ t("physicalAssets.emptyGuide") }}</span>
        </template>
      </NDataTable>
    </NCard>

    <!-- 新建 / 编辑弹窗（同一建档表单双模式，编辑态无估值字段，T2）。
         显示由「意图非空」派生（无独立 show 布尔），关闭（✕ / ESC / 取消 /
         保存成功）统一经工厂清回 null 终态；序号作 key 强制重建（ADR-0072）。 -->
    <PhysicalAssetFormModal
      :key="formSeq"
      :show="formIntent !== null"
      :editing="formIntent?.mode === 'edit' ? formIntent.asset : null"
      @update:show="(v: boolean) => (v ? undefined : closeForm())"
    />
    <!-- 更新估值弹窗（T2：追加历史行，旧值保留）。显示由「意图非空」派生
         （无独立 show 布尔），关闭统一经工厂清回 null 终态；序号作 key 强制
         重建（ADR-0072）。 -->
    <PhysicalAssetValuationModal
      :key="valuationSeq"
      :show="valuationIntent !== null"
      :asset="valuationIntent?.asset ?? null"
      @update:show="(v: boolean) => (v ? undefined : closeValuation())"
    />
    <!-- 处置弹窗（T3：状态标记，处置日期必填、处置价可选纯记录）。显示由
         「意图非空」派生（无独立 show 布尔），关闭统一经工厂清回 null 终态；
         序号作 key 强制重建（ADR-0072）。 -->
    <PhysicalAssetDisposeModal
      :key="disposeSeq"
      :show="disposeIntent !== null"
      :asset="disposeIntent?.asset ?? null"
      @update:show="(v: boolean) => (v ? undefined : closeDispose())"
    />
  </div>
</template>
