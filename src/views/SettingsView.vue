<script setup lang="ts">
import { NTabs, NTabPane, NIcon } from "naive-ui";
import {
  OptionsOutline,
  GridOutline,
  CashOutline,
  ServerOutline,
  RepeatOutline,
  ToggleOutline,
  InformationCircleOutline,
} from "@vicons/ionicons5";
import GeneralSettings from "@/settings/GeneralSettings.vue";
import CategoryManager from "@/categories/CategoryManager.vue";
import BaseCurrencySettings from "@/settings/BaseCurrencySettings.vue";
import DisplayCurrencySettings from "@/settings/DisplayCurrencySettings.vue";
import ExchangeRateSyncSettings from "@/settings/ExchangeRateSyncSettings.vue";
import BackupSettings from "@/settings/BackupSettings.vue";
import DataLocationSettings from "@/settings/DataLocationSettings.vue";
import EncryptionSettings from "@/settings/EncryptionSettings.vue";
import SyncSettings from "@/settings/SyncSettings.vue";
import BalanceCacheSettings from "@/settings/BalanceCacheSettings.vue";
import ScheduledSettings from "@/settings/ScheduledSettings.vue";
import FeatureToggleSettings from "@/settings/FeatureToggleSettings.vue";
import AboutSettings from "@/settings/AboutSettings.vue";
import { useFeatureToggleStore } from "@/settings/feature-toggles";
import {
  SETTINGS_CARD_STACK_CLASS,
  SETTINGS_COLUMN_CLASS,
} from "@/settings/settings-layout.css.ts";
import { t } from "@ledger/i18n";

const featureToggles = useFeatureToggleStore();
</script>

<template>
  <!-- 设置页内容列动态宽度（issue #651 修订）：单列，但卡片与宽表随窗口宽度
       铺满内容区（settings-layout.css.ts 承载 100% 宽度口径），不再固定 720px。 -->
  <div data-testid="settings-column" :class="SETTINGS_COLUMN_CLASS">
    <NTabs type="line">
      <NTabPane name="general" key="general">
        <template #tab
          ><span class="pane-tab"
            ><NIcon :component="OptionsOutline" />{{ t("settings.tabs.general") }}</span
          ></template
        >
        <GeneralSettings />
      </NTabPane>

      <NTabPane name="categories" key="categories">
        <template #tab
          ><span class="pane-tab"
            ><NIcon :component="GridOutline" />{{ t("settings.tabs.categories") }}</span
          ></template
        >
        <!-- 币种域卡片已迁「币种」页签（issue #1664，ADR-0022 修订注记），
             本页签回归纯分类维护、名实相符。 -->
        <CategoryManager />
      </NTabPane>

      <!-- 币种域维护面（issue #1664，ADR-0022 修订注记）：本位币基准（issue #858，
           账本级设置）、展示币种（轻量设备偏好，自「通用」迁入——与本位币基准相邻、
           各带作用域提示，正对本位币/展示币的混淆面）、汇率同步手动入口（issue #1545）
           按领域归属（ADR-0059）自「分类」「通用」迁入独立页签；同步语义全在后端命令。 -->
      <NTabPane name="currencies" key="currencies">
        <template #tab
          ><span class="pane-tab"
            ><NIcon :component="CashOutline" />{{ t("settings.tabs.currencies") }}</span
          ></template
        >
        <div :class="SETTINGS_CARD_STACK_CLASS">
          <BaseCurrencySettings />
          <DisplayCurrencySettings />
          <ExchangeRateSyncSettings />
        </div>
      </NTabPane>

      <NTabPane name="data" key="data" display-directive="show:lazy">
        <template #tab
          ><span class="pane-tab"
            ><NIcon :component="ServerOutline" />{{ t("settings.tabs.data") }}</span
          ></template
        >
        <NTabs type="line">
          <NTabPane
            name="backup"
            key="backup"
            :tab="t('settings.data.tabs.backup')"
            display-directive="show:lazy"
          >
            <BackupSettings />
          </NTabPane>
          <NTabPane
            name="location"
            key="location"
            :tab="t('settings.data.tabs.location')"
            display-directive="show:lazy"
          >
            <DataLocationSettings />
          </NTabPane>
          <NTabPane
            name="encryption"
            key="encryption"
            :tab="t('settings.data.tabs.encryption')"
            display-directive="show:lazy"
          >
            <EncryptionSettings />
          </NTabPane>
          <!-- 多端同步（issue #862）：与备份/加密同属数据安全与多端世界，随加密子页签之后。 -->
          <NTabPane
            name="sync"
            key="sync"
            :tab="t('settings.data.tabs.sync')"
            display-directive="show:lazy"
          >
            <SyncSettings />
          </NTabPane>
          <!-- 数据修复：余额缓存审计修复卡片（ADR-0067）。 -->
          <NTabPane
            name="repair"
            key="repair"
            :tab="t('settings.data.tabs.repair')"
            display-directive="show:lazy"
          >
            <div :class="SETTINGS_CARD_STACK_CLASS">
              <BalanceCacheSettings />
            </div>
          </NTabPane>
        </NTabs>
      </NTabPane>

      <NTabPane
        v-if="!featureToggles.isFeatureClosed('scheduled')"
        name="scheduled"
        key="scheduled"
      >
        <template #tab
          ><span class="pane-tab"
            ><NIcon :component="RepeatOutline" />{{ t("settings.tabs.scheduled") }}</span
          ></template
        >
        <ScheduledSettings />
      </NTabPane>

      <!-- 功能可见性开关（issue #1243 / ADR-0116 决策 7）：体量独立成页，插在「关于」之前。 -->
      <NTabPane name="features" key="features">
        <template #tab
          ><span class="pane-tab"
            ><NIcon :component="ToggleOutline" />{{ t("settings.tabs.features") }}</span
          ></template
        >
        <FeatureToggleSettings />
      </NTabPane>

      <NTabPane name="about" key="about">
        <template #tab
          ><span class="pane-tab"
            ><NIcon :component="InformationCircleOutline" />{{ t("settings.tabs.about") }}</span
          ></template
        >
        <AboutSettings />
      </NTabPane>
    </NTabs>
  </div>
</template>

<style scoped>
.pane-tab {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
</style>
