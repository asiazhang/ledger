<script setup lang="ts">
/**
 * Tab 分域（issue #157 / ADR-0022；ADR-0034 移除币种只读展示并更名；#308 新增定时；
 * #444 移除商户 Tab——商户管理迁入「更多」聚合页，入口全应用唯一，ADR-0063；
 * issue #930「通用」定义放宽为不归属业务域页签的应用级偏好，日志卡片迁入；
 * issue #1243 / ADR-0116 新增「功能」Tab——功能可见性开关体量独立成页，插在「关于」
 * 之前；issue #1664「币种」页签自「分类」拆出——本位币基准、展示币种与汇率同步卡
 * 按领域归属（展示币种自「通用」迁入：#930 后通用定义是"不归属业务域页签"，
 * 币种域页签成立后展示币种有了归口）整体聚簇，「分类」回归纯分类维护，ADR-0022
 * 修订注记）：
 * 通用（应用级偏好：轻量设备偏好为主 + 日志卡片）→ 分类（分类维护）→ 币种（本位币
 * 基准 + 展示币种 + 汇率同步）→ 数据（备份 /
 * 存储位置 / 数据修复 子页签）→ 定时（定时计划域设备偏好，可经「功能」Tab 关闭后隐藏）
 * → 功能（功能可见性开关）→ 关于（纯元信息，恒在末位，新增 Tab 一律插在它之前）。
 *
 * 设置面联动（issue #1243 范围 3 / ADR-0116 决策 4）：「功能」Tab 关闭「定时」后本页
 * 「定时」pane 立即隐藏（v-if），重新打开即回原位置——关闭只隐藏入口，不改写任何清单。
 *
 * 「数据」pane 与其内部子页签均用 display-directive='show:lazy'：首次激活挂载后保持挂载。
 * key 必填：naive-ui ≥2.45（vapor 编译产物）对无 key 的 pane 列表按 index patch，
 * 混用默认 if 与 show:lazy pane 时 show:lazy pane 会在切换时被卸载重建（缓存失效），
 * 显式 key 让 Vue 按 key 复用实例。
 *
 * 「数据」pane 内部子页签（issue #568）：备份 / 存储位置 / 加密 / 同步（issue #862）/
 * 数据修复——纯信息架构重组，
 * 三个组件原样迁入、功能项一个不少；备份目录随备份走（ADR-0022 既有归属裁决）；
 * 「数据修复」子页签住余额缓存审计修复卡片（BalanceCacheSettings，ADR-0067 决策 5）。子页签同用 show:lazy + 显式 key：切子页签备份列表不卸载重拉
 * （缓存语义与顶级「数据」pane 同法）；子页签选中态不持久化（无 v-model，下级页签
 * 不持久化原则，与顶级页签现状一致），离开设置页再回来默认回「备份」；
 * 纯文字无图标，「分类」页签支出/收入子 Tab 先例。
 */
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

// 「功能」Tab 的关闭集合读路径：设置面联动（「定时」Tab 隐藏）由本页消费。
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
/* 页签图标 + 文字：gap 负责间距，文字与图标间不落空白，
   保证测试/无障碍按文本定位页签时拿到纯标签文字 */
.pane-tab {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
</style>
