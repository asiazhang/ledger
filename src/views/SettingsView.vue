<script setup lang="ts">
import { NSpace, NTabs, NTabPane } from 'naive-ui'
import GeneralSettings from '@/components/settings/GeneralSettings.vue'
import CategoryManager from '@/components/CategoryManager.vue'
import MerchantManager from '@/components/MerchantManager.vue'
import CurrencySettings from '@/components/settings/CurrencySettings.vue'
import BackupSettings from '@/components/settings/BackupSettings.vue'
import DataLocationSettings from '@/components/settings/DataLocationSettings.vue'
import AboutSettings from '@/components/settings/AboutSettings.vue'
</script>

<template>
  <!-- Tab 分域（issue #157 / ADR-0022）：通用（轻量设备偏好）→ 分类与币种（参考数据）
       → 数据（备份与存储位置）→ 关于（恒在末位）。
       「数据」pane 用 display-directive='show:lazy'：首次激活挂载后保持挂载，
       备份列表在 tab 切换间保留缓存；useBackup 的 onMounted 于首次激活时刷新。 -->
  <NTabs type="line">
    <NTabPane name="general" tab="通用">
      <GeneralSettings />
    </NTabPane>

    <NTabPane name="categories-currencies" tab="分类与币种">
      <NSpace vertical :size="16">
        <CategoryManager />
        <CurrencySettings />
      </NSpace>
    </NTabPane>

    <NTabPane name="merchants" tab="商户">
      <MerchantManager />
    </NTabPane>

    <NTabPane name="data" tab="数据" display-directive="show:lazy">
      <NSpace vertical :size="16">
        <BackupSettings />
        <DataLocationSettings />
      </NSpace>
    </NTabPane>

    <NTabPane name="about" tab="关于">
      <AboutSettings />
    </NTabPane>
  </NTabs>
</template>
