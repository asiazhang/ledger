<script setup lang="ts">
import { computed, h } from "vue";
import { NCard, NDataTable, NEmpty, NRadioButton, NRadioGroup } from "naive-ui";
import type { DataTableColumns } from "naive-ui";
import { t } from "@ledger/i18n";
import { useInputMode } from "@/composables/useInputMode";
import { useWindowTier } from "@ledger/window-tier";
import { MERCHANT_TOP_N_OPTIONS } from "@/reports/reports-session";
import { formatAmount } from "@ledger/money";
import { merchantTableRows } from "@ledger/utils/merchant-chart";
import type { MerchantTableRow } from "@ledger/utils/merchant-chart";
import MerchantLink from "@/merchants/MerchantLink.vue";
import type { MerchantSharesReport } from "@ledger/types";

const props = defineProps<{ report: MerchantSharesReport; topN: number }>();
const emit = defineEmits<{
  (e: "update:topN", value: number): void;
  (e: "drilldown", merchantId: string): void;
}>();

const windowTier = useWindowTier();
const isMobileTier = computed(() => windowTier.value === "mobile");
const inputMode = useInputMode();
const isTouch = computed(() => inputMode.value === "touch");
const TOUCH_TARGET_STYLE = { minHeight: "48px" };

const tableRows = computed(() => merchantTableRows(props.report.rows, props.report.total_cents));

function onDrill(merchantId: string) {
  emit("drilldown", merchantId);
}

const renderNameCell = (row: MerchantTableRow) =>
  h(MerchantLink, {
    merchantId: row.merchant_id,
    drillIntent: true,
    "data-testid": "merchant-name",
    onDrill,
  });

const renderBarCell = (row: MerchantTableRow) =>
  h("div", { class: "merchant-bar-track", "data-testid": "merchant-bar-track" }, [
    h("div", {
      class: "merchant-bar-fill",
      "data-testid": "merchant-bar",
      style: {
        width: `${row.barPct}%`,
        background: `linear-gradient(90deg, ${row.color}66, ${row.color})`,
      },
    }),
  ]);

const renderAmountCell = (row: MerchantTableRow) =>
  h(
    "span",
    { class: "merchant-amount", "data-testid": "merchant-amount" },
    formatAmount(row.amount_cents),
  );

const renderShareCell = (row: MerchantTableRow) =>
  h("span", { "data-testid": "merchant-share" }, `${row.sharePct}%`);

const renderCountCell = (row: MerchantTableRow) =>
  h("span", { "data-testid": "merchant-count" }, String(row.transactionCount));

const MOBILE_STACK_STYLE = "display: flex; flex-direction: column; gap: 2px; min-width: 0;";
const MOBILE_AMOUNT_STACK_STYLE =
  "display: flex; flex-direction: column; gap: 2px; align-items: flex-end;";
const MOBILE_SUB_STYLE = "font-size: 12px; opacity: 0.65;";

const columns = computed<DataTableColumns<MerchantTableRow>>(() => {
  if (isMobileTier.value) {
    return [
      {
        title: t("reports.merchant.columns.name"),
        key: "name",
        render: (row) =>
          h("div", { style: MOBILE_STACK_STYLE }, [renderNameCell(row), renderBarCell(row)]),
      },
      {
        title: t("reports.merchant.columns.amount"),
        key: "amount",
        align: "right" as const,
        render: (row) =>
          h("div", { style: MOBILE_AMOUNT_STACK_STYLE }, [
            renderAmountCell(row),
            h("div", { style: MOBILE_SUB_STYLE }, [
              renderShareCell(row),
              " · ",
              renderCountCell(row),
            ]),
          ]),
      },
    ];
  }
  return [
    {
      title: t("reports.merchant.columns.name"),
      key: "name",
      render: renderNameCell,
    },
    {
      title: t("reports.merchant.columns.bar"),
      key: "bar",
      render: renderBarCell,
    },
    {
      title: t("reports.merchant.columns.amount"),
      key: "amount",
      align: "right",
      render: renderAmountCell,
    },
    {
      title: t("reports.merchant.columns.share"),
      key: "share",
      align: "right",
      render: renderShareCell,
    },
    {
      title: t("reports.merchant.columns.count"),
      key: "count",
      align: "right",
      render: renderCountCell,
    },
  ];
});
</script>

<template>
  <NCard size="small">
    <template #header>
      <div class="merchant-card-header">
        <span>{{ t("reports.merchant.title") }}</span>
        <!-- TopN 档位（issue #588）：Top 5 / Top 10 两枚选项，受控进出会话 store -->
        <NRadioGroup
          :value="topN"
          size="small"
          data-testid="merchant-topn"
          @update:value="(v: number) => emit('update:topN', v)"
        >
          <NRadioButton
            v-for="n in MERCHANT_TOP_N_OPTIONS"
            :key="n"
            :value="n"
            :style="isTouch ? TOUCH_TARGET_STYLE : undefined"
            :data-testid="`merchant-topn-${n}`"
          >
            {{ t("reports.merchant.topOption", { n }) }}
          </NRadioButton>
        </NRadioGroup>
      </div>
    </template>
    <NEmpty
      v-if="report.rows.length === 0"
      :description="t('reports.merchant.empty')"
      data-testid="merchant-empty"
    />
    <div v-else data-testid="merchant-table-scroll" style="max-height: 320px; overflow-y: auto">
      <NDataTable
        data-testid="merchant-table"
        :columns="columns"
        :data="tableRows"
        :bordered="false"
        size="small"
        :row-key="(row: MerchantTableRow) => row.merchant_id"
      />
    </div>
  </NCard>
</template>

<style scoped>
.merchant-card-header {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 12px;
}

.merchant-bar-track {
  width: 100%;
  min-width: 96px;
  display: flex;
  align-items: center;
}

.merchant-bar-fill {
  height: 10px;
  border-radius: 5px;
}
</style>
