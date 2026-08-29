<script setup lang="ts">
import { NCard, NEmpty } from 'naive-ui'
import { formatAmount } from '@/types'
import type { MerchantShare } from '@/types'

// 商户消费排行面板（issue #192）：展示本期各商户净支出（毛支出 − 退款，本位币）。
// 口径与排序全部在后端 `merchant_shares` 收口，前端零口径逻辑只渲染；
// 商户回归「名字字典」（issue #223）：纯名称行（名称 + 金额），无色块与图标。
defineProps<{ shares: MerchantShare[] }>()
</script>

<template>
  <NCard title="商户消费排行" size="small">
    <NEmpty
      v-if="shares.length === 0"
      description="本期暂无商户消费"
      data-testid="merchant-empty"
    />
    <div v-else class="rank-list">
      <div
        v-for="s in shares"
        :key="s.merchant_id"
        class="rank-row"
        data-testid="merchant-rank-row"
      >
        <span class="rank-name" data-testid="merchant-rank-name">{{ s.merchant_name }}</span>
        <span class="rank-amount" data-testid="merchant-rank-amount">
          {{ formatAmount(s.amount_cents) }}
        </span>
      </div>
    </div>
  </NCard>
</template>

<style scoped>
.rank-list {
  display: flex;
  flex-direction: column;
}

.rank-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 0;
  border-bottom: 1px solid var(--n-border-color, #efeff5);
  font-size: 14px;
}

.rank-row:last-child {
  border-bottom: none;
}

.rank-name {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.rank-amount {
  font-variant-numeric: tabular-nums;
  font-weight: 500;
}
</style>
