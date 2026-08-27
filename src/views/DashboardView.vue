<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { NCard, NGrid, NGridItem, NProgress, NSpace, NTag, NText } from 'naive-ui'
import { api } from '@/api'
import { useReferenceStore } from '@/stores/reference'
import { formatAmount } from '@/types'
import type { AccountBalance, BudgetProgress, MonthlySummary } from '@/types'

// 首页信息卡（issue #144）：逐账户余额 + 本月收支 + 预算进度。
// 快速记账已迁至交易页「记一笔」弹窗、最近交易列表已移除（issue #141）。
const reference = useReferenceStore()
const balances = ref<AccountBalance[]>([])
const currentMonth = ref<MonthlySummary | null>(null)
const budgets = ref<BudgetProgress[]>([])

// 本月收支口径：收入用后端净收入列（income_net）；净支出为展示层计算
// 毛支出 − 退款（与预算消耗、分类占比的 expense_net 口径一致，退款不单列）；
// 结余 = 收入 − 净支出。当月无交易行时全部显示 0。
const netIncomeCents = computed(() => currentMonth.value?.income_cents ?? 0)
const netExpenseCents = computed(
  () => (currentMonth.value?.expense_cents ?? 0) - (currentMonth.value?.refund_cents ?? 0),
)
const balanceCents = computed(() => netIncomeCents.value - netExpenseCents.value)

// 预算进度复用 budget_progress 命令现行统计窗口行为，前端只做展示；无预算时整卡隐藏。
const budgetRows = computed(() =>
  budgets.value.map((b) => ({
    ...b,
    percentage:
      b.budget.amount_cents > 0
        ? Math.min(100, Math.round((b.spent_cents / b.budget.amount_cents) * 100))
        : 0,
  })),
)

onMounted(async () => {
  const now = new Date()
  const year = now.getFullYear()
  const monthKey = `${year}-${String(now.getMonth() + 1).padStart(2, '0')}`
  const [bals, monthly, progress] = await Promise.all([
    api.listAccountBalances(),
    api.monthlySummary(year),
    api.budgetProgress(),
  ])
  balances.value = bals
  currentMonth.value = monthly.find((m) => m.month === monthKey) ?? null
  budgets.value = progress
})
</script>

<template>
  <NSpace vertical :size="16">
    <NGrid :cols="3" :x-gap="16" :y-gap="16" responsive="screen">
      <NGridItem v-for="b in balances" :key="b.account.id">
        <NCard size="small">
          <NSpace vertical :size="4">
            <NText depth="3" style="font-size: 12px">
              {{ b.account.name }}
            </NText>
            <NText strong style="font-size: 22px">
              {{ formatAmount(b.balance_cents, reference.getCurrency(b.account.currency_code)) }}
            </NText>
          </NSpace>
        </NCard>
      </NGridItem>
    </NGrid>

    <NCard title="本月收支" size="small">
      <NGrid :cols="3" :x-gap="16" responsive="screen">
        <NGridItem>
          <NSpace vertical :size="4">
            <NText depth="3" style="font-size: 12px">收入</NText>
            <NText strong style="font-size: 20px">{{ formatAmount(netIncomeCents) }}</NText>
          </NSpace>
        </NGridItem>
        <NGridItem>
          <NSpace vertical :size="4">
            <NText depth="3" style="font-size: 12px">净支出</NText>
            <NText strong style="font-size: 20px">{{ formatAmount(netExpenseCents) }}</NText>
          </NSpace>
        </NGridItem>
        <NGridItem>
          <NSpace vertical :size="4">
            <NText depth="3" style="font-size: 12px">结余</NText>
            <NText strong style="font-size: 20px">{{ formatAmount(balanceCents) }}</NText>
          </NSpace>
        </NGridItem>
      </NGrid>
    </NCard>

    <NCard v-if="budgetRows.length > 0" title="预算进度" size="small">
      <NSpace vertical :size="12">
        <div v-for="row in budgetRows" :key="row.budget.id" class="budget-row">
          <NSpace align="center" justify="space-between" style="width: 100%">
            <NText :type="row.over_budget ? 'error' : 'default'">
              {{ row.category_name }}
            </NText>
            <NSpace align="center" :size="8">
              <NText :type="row.over_budget ? 'error' : 'default'" style="font-size: 12px">
                {{ formatAmount(row.spent_cents) }} / {{ formatAmount(row.budget.amount_cents) }}
              </NText>
              <NTag v-if="row.over_budget" type="error" size="small">超支</NTag>
            </NSpace>
          </NSpace>
          <NProgress
            type="line"
            :percentage="row.percentage"
            :status="row.over_budget ? 'error' : 'success'"
            :show-indicator="false"
            style="margin-top: 4px"
          />
        </div>
      </NSpace>
    </NCard>
  </NSpace>
</template>
