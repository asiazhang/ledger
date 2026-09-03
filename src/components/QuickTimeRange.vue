<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { NButton, NButtonGroup, NIcon, NSpace } from 'naive-ui'
import { ChevronBack, ChevronDown, ChevronForward } from '@vicons/ionicons5'
import AppDatePicker from '@/components/AppDatePicker.vue'
import { api } from '@/api'
import { t } from '@/i18n'
import type { ReportDateRange } from '@/types'
import {
  TIME_PERIOD_PRESETS,
  canStepPeriod,
  derivePeriodBoundary,
  formatPeriodLabel,
  matchPreset,
  periodFromTimestamp,
  periodRange,
  periodStartTimestamp,
  presetRange,
  rangeToPeriod,
  isPeriodWithinBoundary,
  stepPeriod,
  type NullableDateRange,
  type PeriodUnit,
  type TimePeriodPreset,
} from '@/utils/time-period'

/**
 * 时间范围快捷选择共享受控组件（issue #410 / ADR-0057 决策 5，#409 接缝 2 唯一新缝）：
 * 预设芯片 ＋ 期间步进器 ＋ 期间直达面板整行承载，各消费页共用同一交互
 * （词汇表「时间范围快捷选择」）。
 *
 * 受控契约：快照区间经 v-model 进出，组件不持状态源——高亮（matchPreset）与步进
 * 游标（rangeToPeriod）纯由 prop 区间派生，选择产出只经 update:modelValue 回流调用方
 * （交易页唯一事实源仍是 TransactionFilter 日期维度，报表页自持期间状态）。
 * 组件内只持两类非选择状态：分钟级「今天」时钟（长驻跨期场景下预设定义与边界抬升
 * 随之翻转）与数据期间边界缓存（report_date_range 挂载拉取 + ledger:changed 失效重拉，
 * stale-while-revalidate；仅作钳制输入，不是选择状态）。
 *
 * 快照语义 / 游标派生 / 钳制边界与交易页既有行为逐字一致（#381/#382/#383/#391）；
 * 面板开/关经 AppDatePicker 上报弹层注册表（ADR-0035），打开期间纳入 Overlay
 * Suppression；芯片与步进器本身不是弹层。
 */
const props = withDefaults(
  defineProps<{
    /** 受控快照区间（唯一事实源在调用方，组件不持状态源；形状语义见
     * NullableDateRange）。 */
    modelValue: NullableDateRange
    /** 预设芯片闭集（渲染顺序即数组序）：缺省为交易页全闭集（含「全部」五枚）；
     * 报表页「日期闭集」消费形态传入不含「全部」的子集（ADR-0057，期间必有界）。 */
    presets?: readonly TimePeriodPreset[]
  }>(),
  { presets: () => TIME_PERIOD_PRESETS },
)

const emit = defineEmits<{
  'update:modelValue': [NullableDateRange]
}>()

// 今天以分钟级 tick 保持响应式（长驻跨期场景下预设定义随之翻转）；组件内私有时钟，
// 不属于选择状态源——快照区间的唯一事实源恒在调用方。
const nowTick = ref(Date.now())
let nowTicker: ReturnType<typeof setInterval> | undefined

/** 当前点亮芯片：当前区间恰为某预设定义（相对今天）时返回该预设，跨期自动熄灭。 */
const activePreset = computed(() =>
  matchPreset(props.modelValue.from, props.modelValue.to, nowTick.value),
)

/** 当前可步进游标：从日期区间唯一反推的自然周期；「全部」/任意区间为 null（置灰）。 */
const currentPeriod = computed(() => rangeToPeriod(props.modelValue.from, props.modelValue.to))

// 数据期间边界原始日期对（issue #391）：挂载拉取 + ledger:changed 失效重拉
//（AI 导入外扩历史、删除收窄边界即时跟随）。null = 在途或失败 → 钳制退化为
// 不钳制（不阻塞步进）；空库（双 null 日期对，非 null 对象）由派生单点回退为
// 单当前期间。重拉在途时沿用旧值到成功替换（stale-while-revalidate，与参考
// store 同形，不闪烁）；仅在失败时置空退化，静默不 toast（辅助钳制状态）。
// 不走 useLoadable（ADR-0040）：需持值 stale-while-revalidate + 刻意静默退化，
// 均在其形态之外，序号守卫为该形态最小实现。
const dateRange = ref<ReportDateRange | null>(null)
let dateRangeSeq = 0
let unlistenLedgerChanged: UnlistenFn | null = null
let ledgerListenerDisposed = false

async function loadDateRange() {
  const seq = ++dateRangeSeq
  try {
    const range = await api.reportDateRange()
    if (seq === dateRangeSeq) dateRange.value = range
  } catch {
    if (seq === dateRangeSeq) dateRange.value = null
  }
}

/** 当前游标单位下的数据期间边界；「全部」无游标或边界未知（在途/失败）时为 null。 */
const periodBoundary = computed(() => {
  const p = currentPeriod.value
  if (!p || !dateRange.value) return null
  return derivePeriodBoundary(p.unit, dateRange.value, nowTick.value)
})

/** 步进可达性（#391）：边界末端对应箭头置灰；边界未知时 canStepPeriod
 * 退化为不钳制（恒 true）。公式 [最早交易期间, max(当前期间, 最新交易期间)]
 * 由期间数学单点派生，「与今天更晚者」的抬升随分钟级 nowTick 推移。 */
const canStepPrev = computed(() => {
  const p = currentPeriod.value
  return p !== null && canStepPeriod(p, -1, periodBoundary.value)
})
const canStepNext = computed(() => {
  const p = currentPeriod.value
  return p !== null && canStepPeriod(p, 1, periodBoundary.value)
})

/** 期间直达面板的单位：选中期间随当前游标单位，全部态约定从月面板开始。 */
const periodPanelUnit = computed<PeriodUnit>(() => currentPeriod.value?.unit ?? 'month')
const periodPanelValue = computed(() => {
  const p = currentPeriod.value
  if (p) return periodStartTimestamp(p)
  return periodStartTimestamp(periodFromTimestamp('month', nowTick.value))
})
const periodPanelFormat = computed(() => t(`quickTimeRange.periodPicker.format.${periodPanelUnit.value}`))
const periodPanelBoundary = computed(() => {
  if (!dateRange.value) return null
  return derivePeriodBoundary(periodPanelUnit.value, dateRange.value, nowTick.value)
})
const periodPanelYearRange = computed<[number, number]>(() => {
  const boundary = periodPanelBoundary.value
  if (boundary) return [boundary.earliest.year, boundary.latest.year]
  const year = new Date(nowTick.value).getFullYear()
  return [year - 100, year + 100]
})

/** 面板只允许选择数据期间边界内的月/季/年；边界在途或失败时按步进器约定不钳制。 */
type PeriodDatePickerDetail =
  | { type: 'date'; year: number; month: number; date: number }
  | { type: 'month'; year: number; month: number }
  | { type: 'quarter'; year: number; quarter: number }
  | { type: 'year'; year: number }
  | { type: 'input' }

const isPeriodDateDisabled = (_timestamp: number, detail: PeriodDatePickerDetail): boolean => {
  const boundary = periodPanelBoundary.value
  if (!boundary || detail.type === 'input') return false
  let period
  if (periodPanelUnit.value === 'month' && detail.type === 'month') {
    // Naive UI 的月份 detail.month 是 0 起月份。
    period = { unit: 'month' as const, year: detail.year, index: detail.month }
  } else if (periodPanelUnit.value === 'quarter' && detail.type === 'quarter') {
    // Naive UI 的季度 detail.quarter 是 1 起季度号。
    period = { unit: 'quarter' as const, year: detail.year, index: detail.quarter - 1 }
  } else if (periodPanelUnit.value === 'year' && detail.type === 'year') {
    period = { unit: 'year' as const, year: detail.year, index: 0 }
  } else {
    return false
  }
  return !isPeriodWithinBoundary(period, boundary)
}

const periodPanelOpen = ref(false)
const periodLabelText = computed(() =>
  currentPeriod.value
    ? formatPeriodLabel(currentPeriod.value)
    : t('quickTimeRange.periodLabel.none'),
)

/** 面板点选写精确自然周期快照并关闭面板；全部态仅在确认选定后才离开无过滤默认态。 */
function onPeriodPanelSelect(timestamp: number | null) {
  if (timestamp === null) return
  const period = periodFromTimestamp(periodPanelUnit.value, timestamp)
  emit('update:modelValue', periodRange(period))
  periodPanelOpen.value = false
}

/** 步进：< / > 按当前区间单位步进到上一个/下一个自然周期并经 v-model 写回快照。
 * 钳制守卫双保险：按钮置灰为主，边界在点击派发间隙到达时在此拦下。 */
function onStepPeriod(delta: 1 | -1) {
  const p = currentPeriod.value
  if (!p || !canStepPeriod(p, delta, periodBoundary.value)) return
  emit('update:modelValue', periodRange(stepPeriod(p, delta)))
}

/** 点芯片：换算含边界日期快照经 v-model 回流调用方（「全部」= 双空区间）。 */
function onPresetSelect(preset: TimePeriodPreset) {
  if (preset === 'all') {
    emit('update:modelValue', { from: null, to: null })
    return
  }
  emit('update:modelValue', presetRange(preset, nowTick.value))
}

/** 芯片文案 key：闭集枚举 → i18n（quickTimeRange.period.*，随组件收口）。 */
function presetLabel(preset: TimePeriodPreset): string {
  return t(`quickTimeRange.period.${preset}`)
}

onMounted(() => {
  // 数据期间边界首拉（issue #391）
  void loadDateRange()
  // 订阅 ledger:changed：数据写入/删除后边界重拉，即时外扩/收窄。注册为异步，
  // 注册完成前到达的信号会丢失（窗口极窄，与参考 store 订阅同形）。
  void listen('ledger:changed', () => {
    void loadDateRange()
  })
    .then((fn) => {
      if (ledgerListenerDisposed) {
        fn()
        return
      }
      unlistenLedgerChanged = fn
    })
    .catch(() => {
      /* 监听注册失败不阻塞视图（本地事件，极少发生） */
    })
  // 今天 tick：分钟级刷新响应式「今天」，驱动预设定义、高亮派生与边界抬升跨期翻转
  nowTicker = setInterval(() => {
    nowTick.value = Date.now()
  }, 60_000)
})

onBeforeUnmount(() => {
  if (nowTicker !== undefined) clearInterval(nowTicker)
  ledgerListenerDisposed = true
  unlistenLedgerChanged?.()
  unlistenLedgerChanged = null
})
</script>

<template>
  <NSpace :size="8" align="center" :wrap="true">
    <NButtonGroup size="small">
      <NButton
        v-for="p in presets"
        :key="p"
        size="small"
        :type="activePreset === p ? 'primary' : 'default'"
        :quaternary="activePreset !== p"
        :aria-pressed="activePreset === p"
        @click="onPresetSelect(p)"
      >
        {{ presetLabel(p) }}
      </NButton>
    </NButtonGroup>
    <NButtonGroup size="small">
      <NButton
        size="small"
        quaternary
        :disabled="!canStepPrev"
        :aria-label="t('quickTimeRange.period.prev')"
        @click="onStepPeriod(-1)"
      >
        <NIcon><ChevronBack /></NIcon>
      </NButton>
      <!-- 期间标签按钮（issue #425）：与步进箭头同款 quaternary small + ChevronDown
           「文字 + ▾」下拉通用信号，全部态占位文案同款可点（默认月档）；键盘 Tab 聚焦后
           Enter/Space 打开（原生按钮键盘激活 + keydown 显式双路），aria-haspopup/
           aria-expanded 随面板开合。透明面板载体 pointer-events 关闭，hover/点击
           均落在按钮自身；载体仅作面板锚点。 -->
      <span class="period-label" @click="periodPanelOpen = true">
        <NButton
          size="small"
          quaternary
          aria-haspopup="dialog"
          :aria-expanded="periodPanelOpen"
          @keydown.enter.prevent="periodPanelOpen = true"
          @keydown.space.prevent="periodPanelOpen = true"
        >
          <span class="period-label-text">{{ periodLabelText }}</span>
          <NIcon class="period-label-chevron"><ChevronDown /></NIcon>
        </NButton>
        <AppDatePicker
          class="period-picker"
          :show="periodPanelOpen"
          :value="currentPeriod ? periodPanelValue : null"
          :type="periodPanelUnit"
          :format="periodPanelFormat"
          :year-range="periodPanelYearRange"
          :is-date-disabled="isPeriodDateDisabled"
          :bordered="false"
          :clearable="false"
          :input-readonly="true"
          @update:show="(show: boolean) => (periodPanelOpen = show)"
          @update:value="onPeriodPanelSelect"
        />
      </span>
      <NButton
        size="small"
        quaternary
        :disabled="!canStepNext"
        :aria-label="t('quickTimeRange.period.next')"
        @click="onStepPeriod(1)"
      >
        <NIcon><ChevronForward /></NIcon>
      </NButton>
    </NButtonGroup>
  </NSpace>
</template>

<style scoped>
/* 期间标签：常驻步进器中央（按钮形态，issue #425），min-width 抑制不同期间文案宽度抖动 */
.period-label {
  position: relative;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 96px;
}

/* 隐形直达面板载体只作面板锚点，指针事件交给按钮（hover 底色/点击可达） */
.period-label :deep(.period-picker) {
  position: absolute;
  inset: 0;
  opacity: 0;
  pointer-events: none;
}

.period-label :deep(.n-input) {
  min-width: 96px;
}

/* 「文字 + ▾」下拉信号：箭头与文案的间距 */
.period-label-chevron {
  margin-left: 4px;
}
</style>
