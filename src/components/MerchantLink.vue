<script setup lang="ts">
import { computed } from 'vue'
import { useRouter } from 'vue-router'
import { useAppStore } from '@/stores/app'
import { useReferenceStore } from '@/stores/reference'
import { darkOverrides, lightOverrides } from '@/theme/overrides'

/**
 * 可点击商户名（商户下钻，issue #191）。视觉与交互同 AccountLink（账户下钻）：
 * 点击跳转 `/transactions?merchant=<id>`，交易页自动按该商户过滤。
 *
 * 经 merchantMap（在用 + 软删显示映射）解析名称：软删商户的历史交易照常显示并可下钻；
 * 未知商户 id（参考数据晚到等）渲染为纯文本「-」，不可点击、不下钻。
 */
const props = defineProps<{
  /** 目标商户 id（在参考数据 merchantMap 中查找名称；查不到视为未知，渲染纯文本「-」） */
  merchantId: string
}>()

const reference = useReferenceStore()
const router = useRouter()
const app = useAppStore()

const merchant = computed(() => reference.merchantMap.get(props.merchantId))
const name = computed(() => merchant.value?.name ?? '-')
// 仅参考数据可解析的商户可点击下钻；未知 id 渲染为纯文本「-」。
const isLink = computed(() => !!merchant.value)

// 强调色与 AccountLink 同源：theme/overrides.ts 单一来源，按当前主题取值。
const accent = computed(() => {
  const common =
    app.theme === 'dark' ? darkOverrides.common : lightOverrides.common
  return {
    base: common?.primaryColor ?? '#F59E0B',
    hover: common?.primaryColorHover ?? '#FBBF24',
  }
})

function go() {
  router.push({ name: 'transactions', query: { merchant: props.merchantId } })
}
</script>

<template>
  <button
    v-if="isLink"
    type="button"
    class="merchant-link"
    :title="'查看该商户的交易'"
    :style="{ color: accent.base, '--accent-hover': accent.hover }"
    @click="go"
  >
    {{ name }}
  </button>
  <span v-else class="merchant-placeholder">{{ name }}</span>
</template>

<style scoped>
.merchant-link {
  /* 文本按钮：继承单元格字号，无边框无背景；颜色由主题强调色内联注入（见 accent） */
  border: none;
  padding: 0;
  background: none;
  font: inherit;
  cursor: pointer;
  border-radius: 4px;
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.merchant-link:hover {
  color: var(--accent-hover);
  background: rgba(255, 255, 255, 0.06);
  text-decoration: underline;
}
.merchant-link:focus-visible {
  outline: 2px solid var(--accent-hover);
  outline-offset: 2px;
}
.merchant-placeholder {
  /* 未知商户 id：纯文本「-」，无强调色、不可点击、不下钻 */
  color: inherit;
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
