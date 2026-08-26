<script setup lang="ts">
import { computed } from 'vue'
import { useRouter } from 'vue-router'
import { useAppStore } from '@/stores/app'
import { useReferenceStore } from '@/stores/reference'
import { darkOverrides, lightOverrides } from '@/theme/overrides'

/**
 * 可点击账户名（账户名下钻，issue #96/#97）。
 *
 * 视觉特征（Raycast 风格，已与用户确认）：默认态主题强调色（琥珀）文字、hover 亮琥珀
 * + 下划线浮现 + cursor pointer + 微亮背景（rgba 白 0.06，与侧边栏菜单激活态层级一致）；
 * 悬停 title 提示"查看该账户的交易"；用真实 <button>（非裸点击容器）保证键盘可达
 * （Tab 聚焦 + Enter 触发）。
 *
 * 点击跳转 `/transactions?account=<id>`，交易页按涉及账户语义自动过滤。
 */
const props = defineProps<{
  /** 目标账户 id（在参考数据中查找名称；查不到时仍可点击，名称回退「无」） */
  accountId: string
}>()

const reference = useReferenceStore()
const router = useRouter()
const app = useAppStore()

const name = computed(() => reference.accountMap.get(props.accountId)?.name ?? '无')

// 强调色取自 theme/overrides.ts 单一来源（Naive 不暴露全局 --primary-color CSS 变量，
// 组件内颜色显式按主题取值：暗色琥珀 / 亮色同色相加深版，与 overrides 一致）。
const accent = computed(() => {
  const common =
    app.theme === 'dark' ? darkOverrides.common : lightOverrides.common
  return {
    base: common?.primaryColor ?? '#F59E0B',
    hover: common?.primaryColorHover ?? '#FBBF24',
  }
})

function go() {
  router.push({ name: 'transactions', query: { account: props.accountId } })
}
</script>

<template>
  <button
    type="button"
    class="account-link"
    :title="'查看该账户的交易'"
    :style="{ color: accent.base, '--accent-hover': accent.hover }"
    @click="go"
  >
    {{ name }}
  </button>
</template>

<style scoped>
.account-link {
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
.account-link:hover {
  color: var(--accent-hover);
  background: rgba(255, 255, 255, 0.06);
  text-decoration: underline;
}
.account-link:focus-visible {
  outline: 2px solid var(--accent-hover);
  outline-offset: 2px;
}
</style>
