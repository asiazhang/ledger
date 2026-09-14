<script setup lang="ts">
import { computed } from 'vue'
import { NAlert, NButton, NCard, NTag, NText } from 'naive-ui'
import { CheckmarkCircleOutline, LockClosedOutline } from '@vicons/ionicons5'
import { NIcon } from 'naive-ui'
import { t } from '@ledger/i18n'
import { formatAmount } from '@ledger/money'
import { errorMessage } from '@ledger/utils/errors'
import { useCrossBookSummary } from '@/composables/useCrossBookSummary'
import { useReferenceStore } from '@/stores/reference'
import type { CrossBookBookStatus } from '@ledger/types'
import {
  bookName,
  bookRow,
  cardAmount,
  cardLabel,
  cardsGrid,
  statusIcon,
  summaryRoot,
} from './CrossBookSummaryView.css.ts'

// 跨账本投资汇总页（issue #1196 / ADR-0114）：只读合计视图——折算、逐本状态与
// 口径标注全部由后端 `cross_book_investment_summary` 产出，本组件只做装配渲染；
// 无任何编辑入口（ADR-0114 决策 6，一切写入回到单账本语境）。
const reference = useReferenceStore()
const { summary, error, refresh } = useCrossBookSummary()

// 币种标注（ADR-0114 决策 3 的界面义务）：发生过折算须显式说明口径。
const currencyNote = computed(() => {
  if (!summary.value) return ''
  return summary.value.converted
    ? t('crossBook.convertedNote', { currency: summary.value.target_currency })
    : t('crossBook.sameCurrencyNote', { currency: summary.value.target_currency })
})

// 部分合计警示（ADR-0114 决策 5）：存在未计入本时合计是部分合计，须显式说明。
const hasExcluded = computed(() =>
  (summary.value?.books ?? []).some((b) => b.status !== 'included'),
)

const cards = computed(() => {
  const s = summary.value
  if (!s) return []
  return [
    { key: 'marketValue', cents: s.market_value_cents },
    { key: 'unrealizedPnl', cents: s.unrealized_pnl_cents },
    { key: 'cumulativePnl', cents: s.cumulative_pnl_cents },
    { key: 'investableAssets', cents: s.investable_assets_cents },
  ].map(({ key, cents }) => ({
    key,
    label: t(`crossBook.totals.${key}`),
    amount: formatAmount(cents, reference.currencyMap.get(s.target_currency)),
  }))
})

function statusText(status: CrossBookBookStatus): string {
  return t(`crossBook.status.${status}`)
}
</script>

<template>
  <div :class="summaryRoot" data-testid="cross-book-summary">
    <NAlert v-if="error" type="error" :show-icon="true" class="summary-alert">
      <NText strong>{{ t('crossBook.loadFailed') }}</NText>
      <div>{{ errorMessage(error) }}</div>
    </NAlert>

    <template v-if="summary">
      <NAlert v-if="hasExcluded" type="warning" :show-icon="true">
        {{ t('crossBook.partialNote') }}
      </NAlert>
      <NAlert v-else type="default" :show-icon="false">
        {{ currencyNote }}
      </NAlert>

      <div :class="cardsGrid">
        <NCard v-for="card in cards" :key="card.key" size="small">
          <NText depth="3" :class="cardLabel">{{ card.label }}</NText>
          <div :class="cardAmount" :data-testid="`summary-${card.key}`">
            {{ card.amount }}
          </div>
        </NCard>
      </div>

      <NCard size="small" :title="t('crossBook.booksTitle')">
        <template #header-extra>
          <NText depth="3">{{ currencyNote }}</NText>
        </template>
        <div
          v-for="book in summary.books"
          :key="book.id"
          :class="bookRow"
          :data-testid="`summary-book-${book.id}`"
        >
          <span :class="bookName">
            {{ book.name }}
            <NTag
              v-if="book.status === 'included'"
              size="small"
              round
              :bordered="false"
              type="primary"
            >
              {{ t('crossBook.activeTag') }}
            </NTag>
          </span>
          <NTag
            v-if="book.status === 'included'"
            size="small"
            round
            :bordered="false"
            type="success"
            :title="statusText(book.status)"
          >
            <NIcon :size="12" :class="statusIcon"><CheckmarkCircleOutline /></NIcon>
            {{ statusText(book.status) }}
          </NTag>
          <NTag v-else size="small" round :bordered="false" type="warning">
            <NIcon v-if="book.status === 'locked'" :size="12" :class="statusIcon">
              <LockClosedOutline />
            </NIcon>
            {{ statusText(book.status) }}
          </NTag>
        </div>
      </NCard>
    </template>

    <NButton v-if="error" size="small" type="primary" @click="() => void refresh()">
      {{ t('crossBook.retry') }}
    </NButton>
  </div>
</template>
