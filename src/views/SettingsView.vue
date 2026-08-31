<script setup lang="ts">
import { NSpace, NTabs, NTabPane, NIcon } from 'naive-ui'
import {
  OptionsOutline,
  GridOutline,
  StorefrontOutline,
  ServerOutline,
  RepeatOutline,
  InformationCircleOutline,
} from '@vicons/ionicons5'
import GeneralSettings from '@/components/settings/GeneralSettings.vue'
import CategoryManager from '@/components/CategoryManager.vue'
import MerchantManager from '@/components/MerchantManager.vue'
import BackupSettings from '@/components/settings/BackupSettings.vue'
import DataLocationSettings from '@/components/settings/DataLocationSettings.vue'
import ScheduledSettings from '@/components/settings/ScheduledSettings.vue'
import AboutSettings from '@/components/settings/AboutSettings.vue'
import { t } from '@/i18n'
</script>

<template>
  <!-- Tab 分域（issue #157 / ADR-0022；ADR-0034 移除币种只读展示并更名；#308 新增定时）：
       通用（轻量设备偏好）→ 分类（参考数据）→ 商户（参考数据）→ 数据（备份与存储位置）
       → 定时（定时计划域设备偏好）→ 关于（恒在末位，新增 Tab 一律插在它之前）。
       「数据」pane 用 display-directive='show:lazy'：首次激活挂载后保持挂载，
       备份列表在 tab 切换间保留缓存；useBackup 的 onMounted 于首次激活时刷新。
       key 必填：naive-ui ≥2.45（vapor 编译产物）对无 key 的 pane 列表按 index patch，
       混用默认 if 与 show:lazy pane 时 show:lazy pane 会在切换时被卸载重建（缓存失效），
       显式 key 让 Vue 按 key 复用实例。 -->
  <NTabs type="line">
    <NTabPane name="general" key="general">
      <template #tab><span class="pane-tab"><NIcon :component="OptionsOutline" />{{ t('settings.tabs.general') }}</span></template>
      <GeneralSettings />
    </NTabPane>

    <NTabPane name="categories" key="categories">
      <template #tab><span class="pane-tab"><NIcon :component="GridOutline" />{{ t('settings.tabs.categories') }}</span></template>
      <CategoryManager />
    </NTabPane>

    <NTabPane name="merchants" key="merchants">
      <template #tab><span class="pane-tab"><NIcon :component="StorefrontOutline" />{{ t('settings.tabs.merchants') }}</span></template>
      <MerchantManager />
    </NTabPane>

    <NTabPane name="data" key="data" display-directive="show:lazy">
      <template #tab><span class="pane-tab"><NIcon :component="ServerOutline" />{{ t('settings.tabs.data') }}</span></template>
      <NSpace vertical :size="16">
        <BackupSettings />
        <DataLocationSettings />
      </NSpace>
    </NTabPane>

    <NTabPane name="scheduled" key="scheduled">
      <template #tab><span class="pane-tab"><NIcon :component="RepeatOutline" />{{ t('settings.tabs.scheduled') }}</span></template>
      <ScheduledSettings />
    </NTabPane>

    <NTabPane name="about" key="about">
      <template #tab><span class="pane-tab"><NIcon :component="InformationCircleOutline" />{{ t('settings.tabs.about') }}</span></template>
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
