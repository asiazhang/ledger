<script setup lang="ts">
import { computed } from 'vue'
import { useRouter } from 'vue-router'
import { useAppStore } from '@/stores/app'
import { darkOverrides, lightOverrides } from '@/theme/overrides'
import { t } from '@ledger/i18n'
import {
  INSTRUMENT_LINK_CLASS,
  INSTRUMENT_PLACEHOLDER_CLASS,
} from '@/components/instrument-link.css'

/**
 * 可点击标的代码（标的前提下钻，ADR-0107 决策 4/5）：点击跳转交易页按标的过滤——
 * 持仓页签行携带 accountId（`/transactions?account=&instrument=`，该账户该标的的
 * 交易历史）。盈亏页「按标的汇总」行退役后（ADR-0107 修订注记，2026-09-13），
 * 不携带 accountId 的用法暂无调用方；`?instrument=` 维度本身保留（URL 可直达）。
 *
 * 视觉与交互同 AccountLink / MerchantLink 先例：主题强调色文字、hover 提亮 +
 * 下划线 + 微亮背景；用真实 <button> 保证键盘可达（Tab 聚焦 + Enter 触发）。
 *
 * 标的不在参考数据字典（无 instrumentMap 可查），组件不校验 id 存在性——标的字典
 * 无软删（被流水引用的标的不可删），跳转恒可达、不提供落空的跳转（决策 5）。
 */
const props = defineProps<{
  /** 目标标的 id（跳转载荷核心） */
  instrumentId: string
  /** 展示文本（标的代码）；空值渲染纯文本「-」，不可点击 */
  label: string | null
  /** 可选同游账户 id：在场时跳转载荷带 ?account=（现仅持仓页签行场景） */
  accountId?: string | null
}>()

const router = useRouter()
const app = useAppStore()

// 强调色与 AccountLink / MerchantLink 同源：theme/overrides.ts 单一来源，按当前主题取值。
const accent = computed(() => {
  const common =
    app.theme === 'dark' ? darkOverrides.common : lightOverrides.common
  return {
    base: common?.primaryColor ?? '#F59E0B',
    hover: common?.primaryColorHover ?? '#FBBF24',
  }
})

function go() {
  router.push({
    name: 'transactions',
    query: props.accountId
      ? { account: props.accountId, instrument: props.instrumentId }
      : { instrument: props.instrumentId },
  })
}
</script>

<template>
  <button
    v-if="label"
    type="button"
    :class="INSTRUMENT_LINK_CLASS"
    :title="t('common.link.viewInstrument')"
    :style="{ color: accent.base, '--accent-hover': accent.hover }"
    @click="go"
  >
    {{ label }}
  </button>
  <span v-else :class="INSTRUMENT_PLACEHOLDER_CLASS">-</span>
</template>
