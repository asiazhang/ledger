<script setup lang="ts">
import { computed } from 'vue'
import { NButton, NIcon, NTooltip } from 'naive-ui'
import { InformationCircleOutline } from '@vicons/ionicons5'
import { t } from '@ledger/i18n'
import AppPopover from '@ledger/ui-kit/AppPopover.vue'
import { useInputMode } from '@/composables/useInputMode'

/**
 * 投资域口径说明标签（issue #1369）：标签 + 常驻 `ⓘ`，指针轴悬停即现
 * （裸 NTooltip）、触控轴点按弹出（经 AppPopover 入弹层注册表），两轴文案同源。
 *
 * 入参是**概念键**而不是文案字符串：tip 一律按 `investments.concepts.<concept>Tip`
 * 现取（ADR-0049 文案按域归口），调用方给不出第二份措辞——「文案唯一源」由此成为
 * 结构约束而非评审纪律。标签仍由调用方传入（本表位命名空间），因为同一口径在不同
 * 表位的展示词可以不同（合计三卡「总市值」与持仓表「市值」是同一口径、两个标签）；
 * aria 由标签拼出，读屏用户听到的是他在屏幕上看到的那个词。
 *
 * 作用域变体（`scope`）是必填的显式选择而非默认值：同一份概念文案在持仓页是
 * **过滤后子集**、在首页是**全量**、在跨账本页是**逐本折算合并**，作用域句因此
 * 不能写进概念文案本身，否则必有一处失真。调用方必须指定变体，缺省即有 bug。
 *
 * 与 ADR-0035 的关系：commit bb2c2239 曾裁定「不建 AppTooltip 封装（Speculative
 * Generality）」，其前提是单站点、单一悬停语义。本组件封装的是**双轴形态 + 文案
 * 单源**，站点数已达个位数，该裁定不再适用；ADR-0035 正文（弹层注册表枚举）不变——
 * 触控轴仍走注册过的 AppPopover，裸 NTooltip 仍是注册表外的悬停件。
 * 组件落域内而非 ui-kit：ui-kit 是成员闭集（ADR-0118 决策 5），双轴件先例
 * AmountCell 同样住域内（src/transaction/AmountCell.vue）。
 */

/** 作用域变体闭集：概念文案在三类页面语境下的作用域句 */
export type ConceptScope = 'filtered' | 'wholeLedger' | 'crossBook' | 'mwr'

const props = defineProps<{
  /** 展示标签（取自本表位命名空间的现有键，如 holdings.columns.cost） */
  label: string
  /** 概念键：tip 取 `investments.concepts.<concept>Tip` */
  concept: string
  /** 作用域变体句；概念文案不随语境变化时省略 */
  scope?: ConceptScope
  /** `ⓘ` 触发器的 data-testid（省略即不挂测试钩子） */
  testId?: string
}>()

const inputMode = useInputMode()
const isTouch = computed(() => inputMode.value === 'touch')

const ariaLabel = computed(() => t('investments.concepts.tipAria', { concept: props.label }))

const tip = computed(() => {
  const body = t(`investments.concepts.${props.concept}Tip`)
  if (!props.scope) return body
  return `${body}${t(`investments.concepts.scope${scopeKey(props.scope)}`)}`
})

/** 变体 → 键名后缀（闭集映射，避免调用方拼字符串） */
function scopeKey(scope: ConceptScope): string {
  switch (scope) {
    case 'filtered':
      return 'Filtered'
    case 'wholeLedger':
      return 'WholeLedger'
    case 'crossBook':
      return 'CrossBook'
    case 'mwr':
      return 'Mwr'
  }
}
</script>

<template>
  <span class="concept-label">
    <!-- 概念名自成一个元素：与触发器之间的纯空白节点被编译器去除，标签文案不因
         加图标而多出空格（消费方 .text() 逐字口径不变，合计三卡先例） -->
    <span>{{ label }}</span>
    <NTooltip v-if="!isTouch" placement="top" :style="{ maxWidth: '320px' }">
      <template #trigger>
        <NButton text :aria-label="ariaLabel" :data-testid="testId ? `${testId}-info` : undefined">
          <NIcon :size="14" color="var(--n-label-text-color, #999)">
            <InformationCircleOutline />
          </NIcon>
        </NButton>
      </template>
      {{ tip }}
    </NTooltip>
    <AppPopover v-else trigger="click" placement="top" :style="{ maxWidth: '320px' }">
      <template #trigger>
        <NButton
          text
          class="touch-hit-area"
          :style="{ '--touch-hit-inset': '-10px -10px' }"
          :aria-label="ariaLabel"
          :data-testid="testId ? `${testId}-info` : undefined"
        >
          <NIcon :size="14" color="var(--n-label-text-color, #999)">
            <InformationCircleOutline />
          </NIcon>
        </NButton>
      </template>
      {{ tip }}
    </AppPopover>
  </span>
</template>
