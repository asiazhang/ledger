<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import {
  NAlert,
  NButton,
  NCard,
  NEmpty,
  NGi,
  NGrid,
  NGridItem,
  NProgress,
  NSpace,
  NStatistic,
  NSpin,
  NTag,
  NText,
  NTooltip,
} from 'naive-ui'
import { InformationCircleOutline } from '@vicons/ionicons5'
import { NIcon } from 'naive-ui'
import { useRouter } from 'vue-router'
import { api } from '@/api'
import { useDashboardOverview } from '@/composables/useDashboardOverview'
import { useFinancialFreedom } from '@/composables/useFinancialFreedom'
import { useItemDailyTotal } from '@/composables/useItemDailyTotal'
import { useReferenceStore } from '@/stores/reference'
import { formatAmount } from '@/types'
import type { BudgetProgress, MonthlySummary } from '@/types'
import {
  formatCurrencyGroups,
  usePortfolioOverview,
} from '@/composables/usePortfolioOverview'

// 首页财务全貌仪表盘（issue #140）：净资产总览卡（issue #143）+ 投资概览卡（issue #145）
// + 本月收支与预算进度（issue #144）。
// 快速记账已迁至交易页「记一笔」弹窗、最近交易列表已移除（issue #141）；
// 逐账户余额卡已移除（逐账户明细归账户页，首页只呈现聚合全貌），
// 投资概览/预算进度两卡不再按数据隐匿，空态用 NEmpty 占位。
const reference = useReferenceStore()
const currentMonth = ref<MonthlySummary | null>(null)
const budgets = ref<BudgetProgress[]>([])

// 净资产总览卡（issue #143）：多币种折算与合计全部在后端 `dashboard_overview` 完成，
// 前端只做装配渲染；缺汇率等报错显示提示文案而非空数字。
const { overview, loading, error } = useDashboardOverview()

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

// 投资概览卡（issue #145）：复用持仓概览 composable 的分组求和结果，
// 无任何持仓时整卡隐藏；无行情标的不以零计入合计（sumByCurrency 跳过空值）。
const {
  rows: holdingRows,
  totalMarketValueGroups,
  totalUnrealizedPnlGroups,
} = usePortfolioOverview()

// 物品使用成本卡（issue #122）：全部在用物品每天成本合计，后端 `item_daily_total`
// 聚合（折算与求和全在后端），失效复用物品 store 的重拉节奏（监听 version）。
const {
  total: itemDailyTotal,
  loading: itemDailyTotalLoading,
  error: itemDailyTotalError,
} = useItemDailyTotal()

// 财务自由度卡（issue #344；口径 ADR-0048）：后端 financial_freedom 聚合
//（分子折算、分母年化、3% 提取率），前端只做装配渲染与阶段标签派生。
const router = useRouter()
const {
  data: freedom,
  loading: freedomLoading,
  error: freedomError,
  refresh: refreshFreedom,
} = useFinancialFreedom()

// 折算基准币种（分子/分母同币种展示用）
const freedomCurrency = computed(() =>
  freedom.value ? reference.getCurrency(freedom.value.native_currency) : undefined,
)

// 阶段标签前端派生（ADR-0048 决策 5：阈值与文案归 UI，不做后端枚举）：
// <30% 积累期 / 30–100% 接近自由 / ≥100% 财务自由
const freedomStage = computed(() => {
  if (!freedom.value) return null
  if (freedom.value.ratio >= 100) return { label: '财务自由', type: 'success' as const }
  if (freedom.value.ratio >= 30) return { label: '接近自由', type: 'info' as const }
  return { label: '积累期', type: 'default' as const }
})

// 进度条：>100% 封顶展示，≥100% 转成功状态（达成时刻的视觉确认）
const freedomProgress = computed(() => {
  if (!freedom.value) return { percentage: 0, success: false }
  return {
    percentage: Math.min(100, freedom.value.ratio),
    success: freedom.value.ratio >= 100,
  }
})

function goBudget() {
  void router.push({ name: 'budget' })
}

onMounted(async () => {
  const now = new Date()
  const year = now.getFullYear()
  const monthKey = `${year}-${String(now.getMonth() + 1).padStart(2, '0')}`
  const [monthly, progress] = await Promise.all([api.monthlySummary(year), api.budgetProgress()])
  currentMonth.value = monthly.find((m) => m.month === monthKey) ?? null
  budgets.value = progress
})
</script>

<template>
  <NSpace vertical :size="16">
    <!-- 净资产总览卡（issue #143）：置顶呈现本位币单一主数字，第一眼回答「总共有多少钱」 -->
    <NCard size="small" data-testid="net-worth-card">
      <NSpin :show="loading">
        <NSpace vertical :size="4">
          <NText depth="3" style="font-size: 12px">净资产</NText>
          <template v-if="overview">
            <NText strong style="font-size: 28px">
              {{
                formatAmount(
                  overview.net_worth_cents,
                  reference.getCurrency(overview.native_currency),
                )
              }}
            </NText>
          </template>
          <NAlert v-else-if="error" type="warning" :bordered="false">
            {{ error }}
          </NAlert>
        </NSpace>
      </NSpin>
    </NCard>

    <!-- 本月收支卡（issue #144）：紧随净资产之后，先看本月现金流再看投资/预算 -->
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

    <!-- 投资概览卡（issue #145）：始终展示，无持仓时空态占位 -->
    <NCard title="投资概览" size="small" data-testid="investment-overview-card">
      <NGrid v-if="holdingRows.length > 0" :x-gap="16" cols="1 s:2">
        <NGi>
          <NStatistic label="总市值" data-testid="dashboard-total-market-value">
            {{ formatCurrencyGroups(totalMarketValueGroups, reference.currencyMap) }}
          </NStatistic>
        </NGi>
        <NGi>
          <NStatistic label="未实现盈亏合计" data-testid="dashboard-total-unrealized-pnl">
            {{ formatCurrencyGroups(totalUnrealizedPnlGroups, reference.currencyMap) }}
          </NStatistic>
        </NGi>
      </NGrid>
      <NEmpty v-else description="暂无持仓" />
    </NCard>

    <!-- 财务自由度卡（issue #344）：投资概览之后、物品使用成本之前——
         先看资产再看「资产够不够躺」，最后才看单品与预算执行 -->
    <NCard title="财务自由度" size="small" data-testid="financial-freedom-card">
      <template #header-extra>
        <!-- 计算口径悬停提示：3% 乘数使百分比无法从已展示的分子/分母直接推出，
             不解释会被当成算错（ADR-0048）。悬停即现、移开即关、不拦交互，
             不在 ADR-0035 弹层注册表枚举内，用裸 NTooltip -->
        <NTooltip placement="top" :style="{ maxWidth: '320px' }">
          <template #trigger>
            <NButton text aria-label="计算口径说明" data-testid="financial-freedom-info">
              <NIcon :size="14" color="var(--n-title-text-color, #999)">
                <InformationCircleOutline />
              </NIcon>
            </NButton>
          </template>
          <NSpace vertical :size="2">
            <span>自由度 = 可投资资产 × 3% ÷ 年度预算总额</span>
            <span>分子 = 持仓市值 + 投资账户余额（折算为本位币，生活现金与负债不计入）</span>
            <span>分母 = 月度预算 × 12 + 年度预算（按计划节奏年化，不回退实际支出）</span>
            <span>3% 为保守安全提取率，≥100% 即财务自由</span>
          </NSpace>
        </NTooltip>
      </template>
      <NSpin :show="freedomLoading">
        <!-- 零分母（未设预算）：占位引导跳预算页，口径不回退实际支出；
             按钮放 #extra——NEmpty 的 default slot 会顶掉 description -->
        <NEmpty v-if="freedom && freedom.denominator_cents === 0" description="设置预算后解锁财务自由度">
          <template #extra>
            <NButton size="small" type="primary" @click="goBudget">去设置预算</NButton>
          </template>
        </NEmpty>
        <!-- 缺汇率等报错：卡内警告（沿物品使用成本卡的警告先例）+ 重试
             （重试为本卡按验收新增，先例仅警告无重试；重试即 refresh） -->
        <NAlert v-else-if="freedomError" type="warning" :bordered="false">
          <NSpace align="center" :size="8">
            <span>{{ freedomError }}</span>
            <NButton size="tiny" quaternary type="warning" @click="refreshFreedom">重试</NButton>
          </NSpace>
        </NAlert>
        <NSpace v-else-if="freedom && freedomStage" vertical :size="4">
          <NSpace align="center" justify="space-between" style="width: 100%">
            <NText strong style="font-size: 28px" data-testid="financial-freedom-ratio">
              {{ freedom.ratio }}%
            </NText>
            <NTag :type="freedomStage.type" size="small" data-testid="financial-freedom-stage">
              {{ freedomStage.label }}
            </NTag>
          </NSpace>
          <NProgress
            type="line"
            :percentage="freedomProgress.percentage"
            :status="freedomProgress.success ? 'success' : undefined"
            :show-indicator="false"
          />
          <NText depth="3" style="font-size: 12px">
            可投资资产 {{ formatAmount(freedom.numerator_cents, freedomCurrency) }}
            · 年度预算总额 {{ formatAmount(freedom.denominator_cents, freedomCurrency) }}
          </NText>
          <NText depth="3" style="font-size: 12px">可覆盖 {{ freedom.coverage_years }} 年</NText>
        </NSpace>
      </NSpin>
    </NCard>

    <!-- 物品使用成本卡（issue #122）：全部在用物品每天成本合计（默认币种，后端聚合），
         无在用物品时空态占位，缺汇率等报错显示提示文案 -->
    <NCard title="物品使用成本" size="small" data-testid="item-daily-cost-card">
      <NSpin :show="itemDailyTotalLoading">
        <NAlert v-if="itemDailyTotalError" type="warning" :bordered="false">
          {{ itemDailyTotalError }}
        </NAlert>
        <NEmpty
          v-else-if="itemDailyTotal && itemDailyTotal.item_count === 0"
          description="暂无在用物品"
        />
        <NSpace v-else-if="itemDailyTotal" vertical :size="4">
          <NText depth="3" style="font-size: 12px">全部在用物品每天成本合计</NText>
          <NText strong style="font-size: 20px">
            {{
              formatAmount(
                itemDailyTotal.per_day_cents,
                reference.getCurrency(itemDailyTotal.native_currency),
              )
            }}/天
          </NText>
          <NText depth="3" style="font-size: 12px">
            共 {{ itemDailyTotal.item_count }} 件在用物品
          </NText>
        </NSpace>
      </NSpin>
    </NCard>

    <NCard title="预算进度" size="small" data-testid="budget-progress-card">
      <NEmpty v-if="budgetRows.length === 0" description="未设置预算" />
      <NSpace v-else vertical :size="12">
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
