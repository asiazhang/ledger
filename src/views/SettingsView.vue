<script setup lang="ts">
import { NTabs, NTabPane } from 'naive-ui'
import CategoryManager from '@/components/CategoryManager.vue'
import CurrencySettings from '@/components/settings/CurrencySettings.vue'
import BackupSettings from '@/components/settings/BackupSettings.vue'
import AppearanceSettings from '@/components/settings/AppearanceSettings.vue'
import AboutSettings from '@/components/settings/AboutSettings.vue'
</script>

<template>
  <NTabs type="line">
    <NTabPane name="categories" tab="分类">
      <CategoryManager />
    </NTabPane>

    <NTabPane name="currencies" tab="币种">
      <CurrencySettings />
    </NTabPane>

    <!-- backup pane 用 display-directive='show'：内容保持挂载，备份列表在 tab 切换间
         保留缓存；useBackup 的 onMounted 于视图挂载时刷新，与原行为一致。 -->
    <NTabPane name="backup" tab="备份与恢复" display-directive="show">
      <BackupSettings />
    </NTabPane>

    <NTabPane name="appearance" tab="外观">
      <AppearanceSettings />
    </NTabPane>

    <NTabPane name="about" tab="关于">
      <AboutSettings />
    </NTabPane>
  </NTabs>
</template>
