<script setup lang="ts">
/**
 * Tab 分域（issue #157 / ADR-0022；ADR-0034 移除币种只读展示并更名；#308 新增定时；
 * #444 移除商户 Tab——商户管理迁入「更多」聚合页，入口全应用唯一，ADR-0055）：
 * 通用（轻量设备偏好）→ 分类（参考数据）→ 数据（备份 / 存储位置 / 数据修复 子页签）
 * → 定时（定时计划域设备偏好）→ 关于（恒在末位，新增 Tab 一律插在它之前）。
 *
 * 「数据」pane 与其内部子页签均用 display-directive='show:lazy'：首次激活挂载后保持挂载。
 * key 必填：naive-ui ≥2.45（vapor 编译产物）对无 key 的 pane 列表按 index patch，
 * 混用默认 if 与 show:lazy pane 时 show:lazy pane 会在切换时被卸载重建（缓存失效），
 * 显式 key 让 Vue 按 key 复用实例。
 *
 * 「数据」pane 内部子页签（issue #568）：备份 / 存储位置 / 数据修复——纯信息架构重组，
 * 三个组件原样迁入、功能项一个不少；备份目录随备份走（ADR-0022 既有归属裁决）；
 * 「数据修复」是伞形标签（现仅 SearchDataSettings 搜索派生数据一键修复，issue #513，
 * 后续修复工具归入）。子页签同用 show:lazy + 显式 key：切子页签备份列表不卸载重拉
 * （缓存语义与顶级「数据」pane 同法）；子页签选中态不持久化（无 v-model，下级页签
 * 不持久化原则，与顶级页签现状一致），离开设置页再回来默认回「备份」；
 * 纯文字无图标，「分类」页签支出/收入子 Tab 先例。
 * （搜索修复卡片标题以「拼音搜索数据」开头，若需在模板内注释，避免使用组件会渲染的
 * 文案字样——dev 编译保留模板注释，会被测试的 html 断言读到。）
 */
import { NTabs, NTabPane, NIcon } from 'naive-ui'
import {
  OptionsOutline,
  GridOutline,
  ServerOutline,
  RepeatOutline,
  InformationCircleOutline,
} from '@vicons/ionicons5'
import GeneralSettings from '@/components/settings/GeneralSettings.vue'
import CategoryManager from '@/components/CategoryManager.vue'
import BackupSettings from '@/components/settings/BackupSettings.vue'
import DataLocationSettings from '@/components/settings/DataLocationSettings.vue'
import EncryptionSettings from '@/components/settings/EncryptionSettings.vue'
import SearchDataSettings from '@/components/settings/SearchDataSettings.vue'
import ScheduledSettings from '@/components/settings/ScheduledSettings.vue'
import AboutSettings from '@/components/settings/AboutSettings.vue'
import { t } from '@/i18n'
</script>

<template>
  <!-- 设置页内容列限宽约 720px、左对齐不居中（issue #651）：宽窗口下说明文字
       保持舒适行宽、表单控件不被拉满；无 margin auto，列停靠左侧。 -->
  <div data-testid="settings-column" style="max-width: 720px">
    <NTabs type="line">
      <NTabPane name="general" key="general">
        <template #tab><span class="pane-tab"><NIcon :component="OptionsOutline" />{{ t('settings.tabs.general') }}</span></template>
        <GeneralSettings />
      </NTabPane>

      <NTabPane name="categories" key="categories">
        <template #tab><span class="pane-tab"><NIcon :component="GridOutline" />{{ t('settings.tabs.categories') }}</span></template>
        <CategoryManager />
      </NTabPane>

      <NTabPane name="data" key="data" display-directive="show:lazy">
        <template #tab><span class="pane-tab"><NIcon :component="ServerOutline" />{{ t('settings.tabs.data') }}</span></template>
        <NTabs type="line">
          <NTabPane name="backup" key="backup" :tab="t('settings.data.tabs.backup')" display-directive="show:lazy">
            <BackupSettings />
          </NTabPane>
          <NTabPane name="location" key="location" :tab="t('settings.data.tabs.location')" display-directive="show:lazy">
            <DataLocationSettings />
          </NTabPane>
          <NTabPane name="encryption" key="encryption" :tab="t('settings.data.tabs.encryption')" display-directive="show:lazy">
            <EncryptionSettings />
          </NTabPane>
          <NTabPane name="repair" key="repair" :tab="t('settings.data.tabs.repair')" display-directive="show:lazy">
            <SearchDataSettings />
          </NTabPane>
        </NTabs>
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
  </div>
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
