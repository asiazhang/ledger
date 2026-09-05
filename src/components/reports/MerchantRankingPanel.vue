<script setup lang="ts">
import { computed, h } from 'vue'
import { NCard, NDataTable, NEmpty, NRadioButton, NRadioGroup } from 'naive-ui'
import type { DataTableColumns } from 'naive-ui'
import { t } from '@/i18n'
import { MERCHANT_TOP_N_OPTIONS } from '@/stores/reports-session'
import { formatAmount } from '@/types'
import { merchantTableRows } from '@/utils/merchant-chart'
import type { MerchantTableRow } from '@/utils/merchant-chart'
import MerchantLink from '@/components/MerchantLink.vue'
import type { MerchantSharesReport } from '@/types'

// 商户消费排行面板（issue #192 → #588 柱图化 → #618 表格化）：支出与商户的
// 「列表化度量」以表格呈现——列为 商户名 | 金额分布（内嵌降序条）| 金额数字 |
// 占比% | 交易笔数，无排名序号列。口径、排序与 topN 截断全部在后端
// `merchant_shares` 收口；行构建（条长比例 / 占比 / 笔数透传 / 负值处理）收口
// merchantTableRows 纯函数，本组件零口径逻辑、只做渲染与交互接线。
//
// 内嵌条：条长 ∝ 该行金额 ÷ 显示区最大金额（topN 下即第一名），负净额不画条
// （比例归 0，金额与占比照实——退款大于支出如实可见）；条色沿用分类色板按名次
// 取色（merchantTableRows 单一来源），「基线淡出 → 条端实色」以 CSS 渐变呈现
// （柱图 canvas 插件 softBarFillPlugin 的表格等价物）。
//
// 点商户名下钻（issue #589 → #618）：商户名列复用记账页面可点击商户名入口
// （MerchantLink 受控下钻模式），点击只上报 drilldown 事件（携带 merchant_id），
// 由父组件（报表视图）构造跳转载荷（merchant + 日期边界 + 收支类型集合）。
// 面板保持受控不持状态源：跳转逻辑不在本组件，与分类下钻的「载荷由视图显式构造」
// 同一接缝。
//
// TopN 控件：卡片头部 Top 5 / Top 10 两枚选项（档位闭集二，默认 5），选择归
// 报表页会话 store（会话内保留、冷启动回默认，ADR-0061 同粒度）；本组件受控
// 不持状态源，v-model:topN 进出。
const props = defineProps<{ report: MerchantSharesReport; topN: number }>()
const emit = defineEmits<{
  (e: 'update:topN', value: number): void
  (e: 'drilldown', merchantId: string): void
}>()

const tableRows = computed(() =>
  merchantTableRows(props.report.rows, props.report.total_cents),
)

// 行内下钻接线：MerchantLink 受控模式只上报意图，面板转发为 drilldown 事件，
// 跳转载荷（期间边界 + 类型集合）由报表视图显式构造。
function onDrill(merchantId: string) {
  emit('drilldown', merchantId)
}

const columns = computed<DataTableColumns<MerchantTableRow>>(() => [
  {
    title: t('reports.merchant.columns.name'),
    key: 'name',
    render: (row) =>
      h(MerchantLink, {
        merchantId: row.merchant_id,
        drillIntent: true,
        'data-testid': 'merchant-name',
        onDrill,
      }),
  },
  {
    title: t('reports.merchant.columns.bar'),
    key: 'bar',
    render: (row) =>
      h('div', { class: 'merchant-bar-track', 'data-testid': 'merchant-bar-track' }, [
        h('div', {
          class: 'merchant-bar-fill',
          'data-testid': 'merchant-bar',
          style: {
            width: `${row.barPct}%`,
            // 名次色「淡入渐变 → 实色」：与柱图 softBarFillPlugin 视觉同源
            background: `linear-gradient(90deg, ${row.color}66, ${row.color})`,
          },
        }),
      ]),
  },
  {
    title: t('reports.merchant.columns.amount'),
    key: 'amount',
    align: 'right',
    render: (row) =>
      h(
        'span',
        { class: 'merchant-amount', 'data-testid': 'merchant-amount' },
        formatAmount(row.amount_cents),
      ),
  },
  {
    title: t('reports.merchant.columns.share'),
    key: 'share',
    align: 'right',
    render: (row) =>
      h(
        'span',
        { 'data-testid': 'merchant-share' },
        `${row.sharePct}%`,
      ),
  },
  {
    title: t('reports.merchant.columns.count'),
    key: 'count',
    align: 'right',
    render: (row) =>
      h(
        'span',
        { 'data-testid': 'merchant-count' },
        String(row.transactionCount),
      ),
  },
])
</script>

<template>
  <NCard size="small">
    <template #header>
      <div class="merchant-card-header">
        <span>{{ t('reports.merchant.title') }}</span>
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
            :data-testid="`merchant-topn-${n}`"
          >
            {{ t('reports.merchant.topOption', { n }) }}
          </NRadioButton>
        </NRadioGroup>
      </div>
    </template>
    <NEmpty
      v-if="report.rows.length === 0"
      :description="t('reports.merchant.empty')"
      data-testid="merchant-empty"
    />
    <div
      v-else
      data-testid="merchant-table-scroll"
      style="max-height: 320px; overflow-y: auto"
    >
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
/* 商户卡头部：标题与 TopN 档位控件同行（分类卡头部面包屑同构） */
.merchant-card-header {
  display: flex;
  align-items: center;
  gap: 12px;
}

/* 金额分布内嵌条：轨道占满单元格、细条居中（柱图 barThickness 的表格等价物） */
.merchant-bar-track {
  width: 100%;
  min-width: 96px;
  display: flex;
  align-items: center;
}

.merchant-bar-fill {
  height: 10px;
  border-radius: 5px;
  /* 条长为 0（负净额）时也不可见（宽度 0），金额与占比照实见邻列 */
}
</style>
