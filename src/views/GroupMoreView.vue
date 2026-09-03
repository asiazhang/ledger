<script setup lang="ts">
import { computed, type Component } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { t } from '@/i18n'
import { NTabs, NTabPane, NIcon } from 'naive-ui'
import { ShieldCheckmarkOutline, CubeOutline } from '@vicons/ionicons5'
import PoliciesView from '@/views/PoliciesView.vue'
import PhysicalAssetsView from '@/views/PhysicalAssetsView.vue'
import { sidebarContainment } from '@/composables/useViewShortcuts'
import type { SidebarGroupId } from '@/composables/useViewShortcuts'

/**
 * 组内「更多」聚合页（issue #472 / ADR-0063 决策 1/5）：收纳单位 = 侧栏分组。
 * 页签 = 该组收纳清单序（顺序源模块响应式读出，页签序 = 清单序、成员资格与顺序同源），
 * 默认页签 = 清单首位；页签状态收敛在路由 query.tab（可深链），
 * 切页签 replace 写回（定时页既有约定），无/非法 tab 回默认页签（展示层回退，不写回 query）。
 * 容器零业务逻辑，被收视图整体装载；页签不跨启动持久化（ViewState 只存视图名）。
 * 折叠态不渲染组标题，「更多」链接不出现，本页仅经链接/深链/恢复到达。
 */

const props = defineProps<{ group: SidebarGroupId }>()

/** 收纳成员 → 视图组件（呈现层装配；后续票迁入商户/定时在此追加） */
const CONTAINED_VIEW_COMPONENTS: Record<string, Component> = {
  policies: PoliciesView,
  physicalAssets: PhysicalAssetsView,
}
const CONTAINED_VIEW_ICONS: Record<string, Component> = {
  policies: ShieldCheckmarkOutline,
  physicalAssets: CubeOutline,
}

const route = useRoute()
const router = useRouter()

/** 该组收纳清单（响应式）：空清单 = 无页签（出厂无成员的预建组）。 */
const tabs = computed(() => sidebarContainment.value[props.group])

/** 页签合法性收窄：query.tab 必须是当前清单成员。 */
function isLegalTab(v: unknown): v is string {
  return typeof v === 'string' && (tabs.value as readonly string[]).includes(v)
}

const activeTab = computed(() => (isLegalTab(route.query.tab) ? route.query.tab : tabs.value[0]))

/** 页签切换走 replace：不产生多余历史记录，深链语义（每页签一条 URL）；
 *  展开既有 query——保留路由上未来可能出现的其他参数。 */
function onTabChange(key: string | number) {
  const tab = String(key)
  if (isLegalTab(tab) && tab !== activeTab.value) {
    void router.replace({ query: { ...route.query, tab } })
  }
}
</script>

<template>
  <NTabs type="line" :value="activeTab" @update:value="onTabChange">
    <NTabPane v-for="name in tabs" :key="name" :name="name">
      <template #tab><span class="pane-tab"><NIcon :component="CONTAINED_VIEW_ICONS[name]" />{{ t(`common.nav.${name}`) }}</span></template>
      <component :is="CONTAINED_VIEW_COMPONENTS[name]" />
    </NTabPane>
  </NTabs>
</template>

<style scoped>
/* 页签图标 + 文字：gap 负责间距，文字与图标间不落空白，
   保证测试/无障碍按文本定位页签时拿到纯标签文字 */
.pane-tab {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
</style>
