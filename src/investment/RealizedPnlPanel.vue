<script setup lang="ts">
import { computed, h, type VNodeChild } from "vue";
import { NCard, NDataTable, NEmpty, NGi, NGrid, NSpace, NSpin } from "naive-ui";
import type { DataTableColumn } from "naive-ui";
import PinyinSelect from "@ledger/ui-kit/PinyinSelect.vue";
import { t } from "@ledger/i18n";
import { useAppStore } from "@/stores/app";
import { useReferenceStore } from "@/stores/reference";
import { useWindowTier } from "@ledger/window-tier";
import { kindSemanticColor, pnlSemanticColor } from "@ledger/theme/semantic-colors";
import { formatAmount } from "@ledger/money";
import { useRealizedPnl } from "@/investment/useRealizedPnl";
import ConceptLabel from "@/investment/ConceptLabel.vue";
import { subLine } from "@/investment/pnl-cell.css.ts";
import { renderMwrRateCell, useMoneyWeightedReturn } from "@/investment/useMoneyWeightedReturn";
import type { MwrBasis } from "@ledger/types";

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

// 汇总表通用「已实现收益」列（ADR-0129 决策 1）：主值 = 域内算好的合计
// （已实现盈亏 + 现金分红），副行拆出两腿。金额按行币种格式化（ADR-0107 决策 6：
// 汇总行随交易行币种），数值列右对齐 + 等宽数字（词汇表「表格列形态」，两表同一
// 单点收口）；主值与已实现腿着盈亏涨跌色（红涨绿跌，与持仓页签「持仓收益」列同源），
// 分红腿着分红 kind 色（与交易列表的分红金额同源）——拆解项与结果一眼可分。
function realizedGainColumn(title: string | (() => VNodeChild)): DataTableColumn {
  return {
    title,
    key: "realized_gain_cents",
    align: "right",
    className: "tabular-nums",
    render(row: any) {
      const currency = reference.currencyMap.get(row.currency_code);
      return h("div", [
        h(
          "span",
          { style: { color: pnlSemanticColor(row.realized_gain_cents, appStore.theme) } },
          formatAmount(row.realized_gain_cents, currency),
        ),
        h("div", { class: subLine }, [
          `${t("investments.pnl.columns.realizedPnl")} `,
          h(
            "span",
            { style: { color: pnlSemanticColor(row.realized_pnl_cents, appStore.theme) } },
            formatAmount(row.realized_pnl_cents, currency),
          ),
          ` · ${t("investments.pnl.columns.dividend")} `,
          h(
            "span",
            { style: { color: kindSemanticColor("dividend", appStore.theme) } },
            formatAmount(row.dividend_cents, currency),
          ),
        ]),
      ]);
    },
  };
}

// 已实现收益口径（ADR-0129 / issue #1533）：已实现盈亏 + 现金分红，不含浮动盈亏
const realizedGainTitle = () =>
  h(ConceptLabel, {
    label: t("investments.pnl.columns.realizedGain"),
    concept: "realizedGain",
    testId: "pnl-realized-gain",
  });

// 年度收益口径（ADR-0132 / issue #1535）：三腿合计，与且慢年度收益可对照
const annualReturnTitle = () =>
  h(ConceptLabel, {
    label: t("investments.pnl.columns.annualReturn"),
    concept: "annualReturn",
    testId: "pnl-annual-return",
  });

// 完整年度收益两腿列（ADR-0132）：值由域内算好，前端只格式化。三态——
// 不可算（null：年初或年末仍有持仓而缺价 / 缺汇率）显式标注「无法计算」，
// 不按 0 计；可算时按行币种格式化并着盈亏涨跌色（与已实现收益主值同源）。
function annualLegColumn(title: string | (() => VNodeChild), key: string): DataTableColumn {
  return {
    title,
    key,
    align: "right",
    className: "tabular-nums",
    render(row: any) {
      const value: number | null = row[key];
      if (value === null || value === undefined) {
        return h("span", { class: subLine }, t("investments.pnl.notComputable"));
      }
      const currency = reference.currencyMap.get(row.currency_code);
      return h(
        "span",
        { style: { color: pnlSemanticColor(value, appStore.theme) } },
        formatAmount(value, currency),
      );
    },
  };
}

const yearColumns: DataTableColumn[] = [
  { title: t("investments.pnl.columns.year"), key: "year" },
  realizedGainColumn(realizedGainTitle),
  annualLegColumn(t("investments.pnl.columns.unrealizedChange"), "unrealized_change_cents"),
  annualLegColumn(annualReturnTitle, "annual_return_cents"),
];

const accountCols: DataTableColumn[] = [
  { title: t("investments.pnl.columns.account"), key: "account_name" },
  realizedGainColumn(realizedGainTitle),
];

// 资金加权收益率（issue #1195 / ADR-0115）：账户级与全账级两个粒度与金额口径
// 并列、互不换算。账户行随账户筛选收窄（客户端过滤，行集本就全量返回）；
// 全账行恒为全账本口径、不随筛选收窄（与持仓页签累计收益合计同一先例）；
// 价格失效信号驱动的重拉内化在接缝，无需调用方手动刷新。
const { summary: mwr } = useMoneyWeightedReturn();

interface MwrRow {
  scope: string;
  currency_code: string;
  /** 本行口径（issue #1346）：合集含期初存量的行由后端标为未年化 */
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
    // 三态与口径标注都收口在 renderMwrRateCell 单点（与持仓页收益率列同款形态；
    // issue #1346：合集含期初存量的行按 basis 标未年化角标）。
    // 收益率口径（issue #1369）：三态与「不随筛选/标的收窄」需在场说明；
    // 「不随筛选收窄」是该口径自身的属性（按完整历史计算），写在 mwrTip 正文里，
    // 故不挂作用域变体——变体只承担随页面语境变化的作用域差异
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

      <!-- 页面收敛为 筛选 + 按年/按账户 两张汇总表（ADR-0107 修订注记，2026-09-13）：
           「已实现盈亏概览」卡与「按标的汇总」表退役——总口径在持仓页签合计（累计收益
           含已实现腿）可得，按标的信息在交易页标的筛选下钻可得。后端 realized_pnl_summary
           的 total / by_instrument 读取保留（只减 UI 面，不动 IPC 形状）。 -->
      <NEmpty v-if="!summary" :description="t('investments.pnl.empty')" />
      <template v-if="summary">
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
                :data="summary.by_year"
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
                :data="summary.by_account"
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
