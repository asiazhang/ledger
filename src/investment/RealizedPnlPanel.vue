<script setup lang="ts">
import { computed, h, type VNodeChild, type WritableComputedRef } from "vue";
import { NCard, NDataTable, NEmpty, NGi, NGrid, NSpace, NSpin, NText } from "naive-ui";
import type { DataTableColumn } from "naive-ui";
import PinyinSelect from "@ledger/ui-kit/PinyinSelect.vue";
import { t } from "@ledger/i18n";
import { useAppStore } from "@/stores/app";
import { useReferenceStore } from "@/stores/reference";
import { useWindowTier } from "@ledger/window-tier";
import { kindSemanticColor, pnlSemanticColor } from "@ledger/theme/semantic-colors";
import { formatAmount } from "@ledger/money";
import { useRealizedPnl } from "@/investment/useRealizedPnl";
import { PNL_PAGE_SIZE, useInvestmentsSessionStore } from "@/investment/investments-session";
import ConceptLabel from "@/investment/ConceptLabel.vue";
import { subLine } from "@/investment/pnl-cell.css.ts";
import { renderMwrRateCell, useMoneyWeightedReturn } from "@/investment/useMoneyWeightedReturn";
import type { AccountPnl, MwrBasis, YearPnl } from "@ledger/types";

const reference = useReferenceStore();
const appStore = useAppStore();
const windowTier = useWindowTier();
const isMobileTier = computed(() => windowTier.value === "mobile");
const {
  loading,
  summary,
  selectedAccountId,
  selectedInstrumentId,
  accountOptions,
  pnlInstrumentOptions,
  searchingInstruments,
  refresh,
  searchInstruments,
  onSelectInstrument,
} = useRealizedPnl();
const session = useInvestmentsSessionStore();

const yearRows = computed(() =>
  [...(summary.value?.by_year ?? [])].sort((a, b) => Number(b.year) - Number(a.year)),
);
const accountPnlRows = computed(() => summary.value?.by_account ?? []);

function clampedPage(stored: () => number, rowCount: () => number, commit: (n: number) => void) {
  return computed<number>({
    get: () => {
      const valid = Math.max(1, Math.ceil(rowCount() / PNL_PAGE_SIZE));
      const current = stored();
      if (current > valid) {
        commit(valid);
        return valid;
      }
      return current;
    },
    set: (next) => commit(next),
  });
}

const yearPage = clampedPage(
  () => session.pnlYearPage,
  () => yearRows.value.length,
  session.setPnlYearPage,
);
const accountPage = clampedPage(
  () => session.pnlAccountPage,
  () => accountPnlRows.value.length,
  session.setPnlAccountPage,
);

function paginationFor(page: WritableComputedRef<number>) {
  return computed(() => ({
    page: page.value,
    pageSize: PNL_PAGE_SIZE,
    onChange: (next: number) => {
      page.value = next;
    },
  }));
}
const yearPagination = paginationFor(yearPage);
const accountPagination = paginationFor(accountPage);
function realizedGainColumn(title: string | (() => VNodeChild)): DataTableColumn {
  return {
    title,
    key: "realized_gain_cents",
    align: "right",
    className: "tabular-nums",
    render(rowData) {
      const row = rowData as unknown as YearPnl | AccountPnl;
      const { realized_gain_cents, realized_pnl_cents, dividend_cents } = row;
      if (realized_gain_cents === null || realized_pnl_cents === null || dividend_cents === null) {
        return h("span", t("investments.pnl.notComputable"));
      }
      const currency = reference.currencyMap.get(row.native_currency);
      return h("div", [
        h(
          "span",
          { style: { color: pnlSemanticColor(realized_gain_cents, appStore.theme) } },
          formatAmount(realized_gain_cents, currency),
        ),
        h("div", { class: subLine }, [
          `${t("investments.pnl.columns.realizedPnl")} `,
          h(
            "span",
            { style: { color: pnlSemanticColor(realized_pnl_cents, appStore.theme) } },
            formatAmount(realized_pnl_cents, currency),
          ),
          ` · ${t("investments.pnl.columns.dividend")} `,
          h(
            "span",
            { style: { color: kindSemanticColor("dividend", appStore.theme) } },
            formatAmount(dividend_cents, currency),
          ),
        ]),
      ]);
    },
  };
}
const realizedGainTitle = () =>
  h(ConceptLabel, {
    label: t("investments.pnl.columns.realizedGain"),
    concept: "realizedGain",
    testId: "pnl-realized-gain",
  });

const yearColumns: DataTableColumn[] = [
  { title: t("investments.pnl.columns.year"), key: "year" },
  realizedGainColumn(realizedGainTitle),
];

const accountCols: DataTableColumn[] = [
  { title: t("investments.pnl.columns.account"), key: "account_name" },
  realizedGainColumn(realizedGainTitle),
];

const { summary: mwr } = useMoneyWeightedReturn();

interface MwrRow {
  scope: string;
  currency_code: string;
  basis: MwrBasis;
  rate: number | null;
  testid: string;
}

const mwrRows = computed<MwrRow[]>(() => {
  if (!mwr.value) return [];
  const accountRows: MwrRow[] = mwr.value.by_account
    .filter((a) => !selectedAccountId.value || a.account_id === selectedAccountId.value)
    .map((a) => ({
      scope: a.account_name,
      currency_code: a.currency_code,
      basis: a.basis,
      rate: a.rate,
      testid: `mwr-account-${a.account_id}`,
    }));
  const totalRows: MwrRow[] = mwr.value.total.map((g) => ({
    scope: t("investments.pnl.total"),
    currency_code: g.currency_code,
    basis: g.basis,
    rate: g.rate,
    testid: `mwr-total-${g.currency_code}`,
  }));
  return [...accountRows, ...totalRows];
});

const mwrColumns: DataTableColumn<MwrRow>[] = [
  { title: t("investments.pnl.columns.account"), key: "scope" },
  { title: t("investments.pnl.columns.currency"), key: "currency_code", width: 100 },
  {
    title: () =>
      h(ConceptLabel, {
        label: t("investments.pnl.columns.mwr"),
        concept: "mwr",
        testId: "pnl-mwr",
      }),
    key: "rate",
    align: "right",
    className: "tabular-nums",
    render: (row) => renderMwrRateCell(row.rate, appStore.theme, row.basis),
  },
];
</script>

<template>
  <NSpin :show="loading">
    <NSpace vertical :size="16">
      <NSpace align="center" :size="12">
        <PinyinSelect
          v-model:value="selectedAccountId"
          :options="accountOptions"
          :placeholder="t('investments.pnl.filterAccount')"
          clearable
          style="width: 180px"
          @update:value="refresh"
        />
        <!-- 远程搜索标的：拼音过滤由后端 list_instruments 统一语义（ADR-0027）
             承担，remote 下本地 filter 不生效，仅收口 filterable 保持载体一致。 -->
        <PinyinSelect
          v-model:value="selectedInstrumentId"
          :options="pnlInstrumentOptions"
          :placeholder="t('investments.pnl.filterInstrument')"
          remote
          clearable
          :loading="searchingInstruments"
          virtual-scroll
          style="width: 220px"
          @update:value="onSelectInstrument"
          @search="searchInstruments"
        />
      </NSpace>

      <!-- 页面收敛为 筛选 + 按年/按账户 两张单值表（ADR-0107 修订注记，2026-09-13；
#1845 起每行折本位币单值，total / by_instrument 随单值翻案从契约中删除）——
总口径在持仓页签合计（累计收益含已实现腿）可得，按标的信息在明细页签标的
筛选下钻可得。 -->
      <NEmpty v-if="!summary" :description="t('investments.pnl.empty')" />
      <template v-if="summary">
        <!-- 口径说明句（#1845）：披露折算近似等级（按实现时点所在周汇率、就近周
             兜底折本位币），不做逐格标注；两表共用一句。 -->
        <NText depth="3" data-testid="pnl-calibration-note">{{
          t("investments.pnl.calibrationNote")
        }}</NText>

        <!-- 列数用纯数字 + 窗口分级：NGrid 默认 responsive="self" 只认数字前缀，
             具名断点（s:）永不命中会静默退成 1 列（两表竖排）。 -->
        <NGrid :x-gap="16" :y-gap="16" :cols="isMobileTier ? 1 : 2">
          <NGi>
            <NCard :title="t('investments.pnl.byYear')" size="small">
              <NEmpty
                v-if="summary.by_year.length === 0"
                :description="t('investments.pnl.emptyTable')"
              />
              <NDataTable
                v-else
                :columns="yearColumns"
                :data="yearRows"
                :pagination="yearPagination"
                :paginate-single-page="false"
                :bordered="false"
                size="small"
              />
            </NCard>
          </NGi>
          <NGi>
            <NCard :title="t('investments.pnl.byAccount')" size="small">
              <NEmpty
                v-if="summary.by_account.length === 0"
                :description="t('investments.pnl.emptyTable')"
              />
              <NDataTable
                v-else
                :columns="accountCols"
                :data="accountPnlRows"
                :pagination="accountPagination"
                :paginate-single-page="false"
                :bordered="false"
                size="small"
              />
            </NCard>
          </NGi>
        </NGrid>

        <!-- 资金加权收益率（issue #1195 / ADR-0115）：账户级 + 全账级（按币种分组、
             不跨币种折算），与金额口径并列、互不换算；重拉由价格失效信号驱动
             （接缝内化），账户筛选变化时与已实现盈亏同路重查对齐行集 -->
        <NCard size="small">
          <template #header>
            <ConceptLabel
              :label="t('investments.pnl.byMwr')"
              concept="mwr"
              test-id="pnl-mwr-card"
            />
          </template>
          <NDataTable
            v-if="mwrRows.length > 0"
            :columns="mwrColumns"
            :data="mwrRows"
            :row-key="(r: MwrRow) => r.testid"
            :bordered="false"
            size="small"
          />
          <NEmpty v-else :description="t('investments.pnl.emptyTable')" />
        </NCard>
      </template>
    </NSpace>
  </NSpin>
</template>
