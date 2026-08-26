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
 *
 * 黑洞/隐藏账户（如「无(CNY)」「无(HKD)」，is_hidden=1）不在参考数据 accountMap 中
 * （list_accounts 不含隐藏账户），渲染为纯文本「-」：无强调色、不可点击、不下钻
 * （issue #96/#97 修订）。真实可见账户才渲染为可点击的主题强调色按钮。
 */
const props = defineProps<{
  /** 目标账户 id（在参考数据中查找名称；查不到视为黑洞/隐藏账户，渲染纯文本「-」） */
  accountId: string
}>()

const reference = useReferenceStore()
const router = useRouter()
const app = useAppStore()

const account = computed(() => reference.accountMap.get(props.accountId))
const name = computed(() => account.value?.name ?? '-')
// 仅真实可见账户可点击下钻；黑洞/隐藏账户渲染为纯文本「-」。
const isLink = computed(() => !!account.value)

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
    v-if="isLink"
    type="button"
    class="account-link"
    :title="'查看该账户的交易'"
    :style="{ color: accent.base, '--accent-hover': accent.hover }"
    @click="go"
  >
    {{ name }}
  </button>
  <span v-else class="account-placeholder">{{ name }}</span>
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
.account-placeholder {
  /* 黑洞/隐藏账户：纯文本「-」，无强调色、不可点击、不下钻 */
  color: inherit;
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
