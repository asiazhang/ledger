<script setup lang="ts">
import { NSpace, NTabs, NTabPane, NIcon } from 'naive-ui'
import {
  OptionsOutline,
  GridOutline,
  StorefrontOutline,
  ServerOutline,
  InformationCircleOutline,
} from '@vicons/ionicons5'
import GeneralSettings from '@/components/settings/GeneralSettings.vue'
import CategoryManager from '@/components/CategoryManager.vue'
import MerchantManager from '@/components/MerchantManager.vue'
import BackupSettings from '@/components/settings/BackupSettings.vue'
import DataLocationSettings from '@/components/settings/DataLocationSettings.vue'
import AboutSettings from '@/components/settings/AboutSettings.vue'
</script>

<template>
  <!-- Tab 分域（issue #157 / ADR-0022；ADR-0034 移除币种只读展示并更名）：
       通用（轻量设备偏好）→ 分类（参考数据）→ 数据（备份与存储位置）→ 关于（恒在末位）。
       「数据」pane 用 display-directive='show:lazy'：首次激活挂载后保持挂载，
       备份列表在 tab 切换间保留缓存；useBackup 的 onMounted 于首次激活时刷新。 -->
  <NTabs type="line">
    <NTabPane name="general">
      <template #tab><span class="pane-tab"><NIcon :component="OptionsOutline" />通用</span></template>
      <GeneralSettings />
    </NTabPane>

    <NTabPane name="categories">
      <template #tab><span class="pane-tab"><NIcon :component="GridOutline" />分类</span></template>
      <CategoryManager />
    </NTabPane>

    <NTabPane name="merchants">
      <template #tab><span class="pane-tab"><NIcon :component="StorefrontOutline" />商户</span></template>
      <MerchantManager />
    </NTabPane>

    <NTabPane name="data" display-directive="show:lazy">
      <template #tab><span class="pane-tab"><NIcon :component="ServerOutline" />数据</span></template>
      <NSpace vertical :size="16">
        <BackupSettings />
        <DataLocationSettings />
      </NSpace>
    </NTabPane>

    <NTabPane name="about">
      <template #tab><span class="pane-tab"><NIcon :component="InformationCircleOutline" />关于</span></template>
      <AboutSettings />
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
